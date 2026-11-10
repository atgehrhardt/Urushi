//! Writing changes to Jira boards through to Jira (ADR 0020).
//!
//! A change to a task on a Jira board is made in Jira first: a field edit, a
//! transition, a new issue, a new rank, a deletion. Then the issue is read back and
//! merged (or, once deleted, forgotten), so the board shows what Jira did (automation,
//! resolutions, a status the transition chose). If Jira refuses, the request fails and
//! nothing changes here. Only a task's color is Urushi's own, and changes without Jira.
//!
//! Creating issues is in `jira_create.rs`. Nesting issues and changing their type are
//! left to Jira. Deleting one is confirmed by the client first (ADR 0033).

use urushi_core::board::{
    JiraBoard, StatusCategory, SyncedIssue, task_notes, task_tags, task_title,
};
use urushi_core::{BoardId, ColumnId, Task, TaskId, TaskPatch};

use super::{Service, ServiceError, ServiceResult, State, blocking, new_uuid};
use crate::jira::{IssueEdit, IssueFields, JiraError, Rank};

/// A task on a Jira board, with what writing to Jira needs.
pub(super) struct JiraTask {
    pub(super) board: BoardId,
    pub(super) jira: JiraBoard,
    pub(super) task: Task,
    pub(super) key: String,
    /// It is a subtask under another issue.
    pub(super) nested: bool,
}

impl State {
    /// The task, if it is on a Jira board.
    pub(super) fn jira_task(&self, id: TaskId) -> ServiceResult<Option<JiraTask>> {
        let board = self
            .boards
            .iter()
            .find(|b| b.task(id).is_some())
            .ok_or_else(|| ServiceError::NotFound(format!("task {id}")))?;
        let Some(jira) = board.jira() else {
            return Ok(None);
        };
        let path = board.path_to(id).unwrap_or_default();
        let task = board
            .task(id)
            .ok_or_else(|| ServiceError::NotFound(format!("task {id}")))?;
        let key = task
            .issue
            .as_ref()
            .map(|i| i.key.clone())
            .ok_or_else(|| ServiceError::Precondition("that task isn't a Jira issue".into()))?;
        Ok(Some(JiraTask {
            board: board.id,
            jira: jira.clone(),
            task: task.clone(),
            key,
            nested: path.len() > 1,
        }))
    }
}

/// Tags as Jira labels: Jira refuses labels with spaces, so say so first.
pub(super) fn labels(tags: &[String]) -> ServiceResult<Vec<String>> {
    let tags = task_tags(tags)?;
    if let Some(bad) = tags.iter().find(|t| t.contains(char::is_whitespace)) {
        return Err(ServiceError::Invalid(format!(
            "Jira labels can't contain spaces: {bad:?}"
        )));
    }
    Ok(tags)
}

pub(super) fn refused(what: &str) -> ServiceError {
    ServiceError::Precondition(format!("{what} on a Jira board: do it in Jira"))
}

impl Service {
    /// Merges issues read back after a write into their board.
    pub(super) fn merge_written(
        &self,
        board: BoardId,
        issues: Vec<SyncedIssue>,
    ) -> ServiceResult<()> {
        self.saw(board, &issues);
        self.mutate_if_changed(|state| {
            let b = state.board_mut(board)?;
            Ok(
                b.merge_issues(issues, false, || TaskId::from_uuid(new_uuid()))
                    .changed,
            )
        })?;
        // Jira's automation may have changed more (a parent's status, links): look again
        // at once.
        self.poke(board);
        Ok(())
    }

    /// Changes a Jira task: its fields and check mark in Jira, its color here.
    pub(super) async fn jira_update_task(
        &self,
        t: JiraTask,
        patch: &TaskPatch,
    ) -> ServiceResult<()> {
        if patch.kind.is_some() {
            return Err(refused("an issue's type is changed"));
        }
        let issue = t.task.issue.clone().unwrap_or_default();
        let mut edit = IssueEdit::default();
        if let Some(title) = patch.title.as_deref().map(task_title).transpose()? {
            edit.summary = (title != t.task.title).then_some(title);
        }
        if let Some(notes) = patch.notes.as_deref().map(task_notes).transpose()?
            && notes != t.task.notes
        {
            if !issue.notes_editable {
                return Err(ServiceError::Precondition(
                    "this description is too long to edit here: edit it in Jira".into(),
                ));
            }
            edit.notes = Some(notes);
        }
        if let Some(tags) = patch.tags.as_deref().map(labels).transpose()? {
            edit.labels_add = tags
                .iter()
                .filter(|l| !t.task.tags.contains(l))
                .cloned()
                .collect();
            edit.labels_remove = t
                .task
                .tags
                .iter()
                .filter(|l| !tags.contains(l))
                .cloned()
                .collect();
        }
        // A card's progress is its column; only a subtask is checked off.
        let done = patch.done.filter(|d| t.nested && *d != t.task.done);

        if !edit.is_empty() || done.is_some() {
            let account = self.jira_account()?;
            let guard = self.jira.lock(t.board);
            let _one_at_a_time = guard.lock().await;
            let api = self.jira.api.clone();
            let (key, fields) = (t.key.clone(), IssueFields::of(&t.jira));
            let read = blocking(move || {
                if !edit.is_empty() {
                    api.edit(&account, &key, &edit)?;
                }
                if let Some(done) = done {
                    let wanted: &[StatusCategory] = if done {
                        &[StatusCategory::Done]
                    } else {
                        &[StatusCategory::ToDo, StatusCategory::InProgress]
                    };
                    let transitions = api.transitions(&account, &key)?;
                    let to = wanted
                        .iter()
                        .find_map(|c| transitions.iter().find(|t| t.to_category == *c))
                        .ok_or_else(|| {
                            ServiceError::Precondition(format!(
                                "Jira's workflow doesn't let {key} be {} from here",
                                if done { "done" } else { "reopened" }
                            ))
                        })?;
                    api.transition(&account, &key, &to.id)?;
                }
                Ok(api.issue(&account, &key, &fields)?)
            })
            .await?;
            self.merge_written(t.board, vec![read])?;
        }
        if patch.color.is_some() {
            let local = TaskPatch {
                color: patch.color,
                ..TaskPatch::default()
            };
            self.mutate(|state| Ok(state.board_mut(t.board)?.update_task(t.task.id, &local)?))?;
        }
        Ok(())
    }

    /// Moves a Jira card: a transition to a status the column shows, then a rank among
    /// the column's cards.
    pub(super) async fn jira_move_task(
        &self,
        t: JiraTask,
        column: ColumnId,
        index: usize,
    ) -> ServiceResult<()> {
        if t.nested {
            return Err(refused("a subtask stays under its issue"));
        }
        let (name, statuses, others) = self.read(|state| {
            let board = state
                .boards
                .iter()
                .find(|b| b.id == t.board)
                .ok_or_else(|| ServiceError::NotFound(format!("board {}", t.board)))?;
            let c = board
                .column(column)
                .ok_or_else(|| ServiceError::NotFound(format!("column {column}")))?;
            let others: Vec<String> = c
                .tasks
                .iter()
                .filter(|x| x.id != t.task.id)
                .filter_map(|x| Some(x.issue.as_ref()?.key.clone()))
                .collect();
            Ok((c.name.clone(), c.statuses.clone(), others))
        })?;
        if statuses.is_empty() {
            return Err(ServiceError::Precondition(format!(
                "the column {name:?} shows no Jira status"
            )));
        }
        let issue = t.task.issue.clone().unwrap_or_default();
        let transition = !statuses.contains(&issue.status_id);
        let rank = match others.get(index) {
            Some(next) => Some(Rank::Before(next.clone())),
            None => others.last().map(|last| Rank::After(last.clone())),
        };
        let account = self.jira_account()?;
        let guard = self.jira.lock(t.board);
        let _one_at_a_time = guard.lock().await;
        // The card moves at once (ADR 0023); if Jira refuses, it goes back.
        let from = self.mutate(|state| {
            Ok(state
                .board_mut(t.board)?
                .show_jira_move(t.task.id, column, index)?)
        })?;
        let api = self.jira.api.clone();
        let (key, fields) = (t.key.clone(), IssueFields::of(&t.jira));
        let read = blocking(move || {
            if transition {
                let to = api
                    .transitions(&account, &key)?
                    .into_iter()
                    .find(|t| statuses.contains(&t.to_status))
                    .ok_or_else(|| {
                        ServiceError::Precondition(format!(
                            "Jira's workflow doesn't allow moving {key} from {} to {name}",
                            issue.status
                        ))
                    })?;
                api.transition(&account, &key, &to.id)?;
            }
            if let Some(rank) = rank {
                // Boards without ranking refuse; the card still moved.
                if let Err(e) = api.rank(&account, &key, &rank) {
                    tracing::debug!(%key, error = %e, "couldn't rank");
                }
            }
            Ok(api.issue(&account, &key, &fields)?)
        })
        .await;
        match read {
            Ok(read) => self.merge_written(t.board, vec![read]),
            Err(e) => {
                let _ = self.mutate(|state| {
                    Ok(state.board_mut(t.board)?.undo_jira_move(t.task.id, from)?)
                });
                Err(e)
            }
        }
    }

    /// Deletes a Jira issue (a card or a subtask) in Jira, with its sub-tasks, then
    /// takes it off the board. An issue Jira no longer has is taken off too. If Jira
    /// refuses (most often: the account may not delete issues), nothing changes here.
    pub(super) async fn jira_delete_task(&self, t: JiraTask) -> ServiceResult<()> {
        let account = self.jira_account()?;
        // Under the board's lock, a sync can't merge the issue back as it goes.
        let guard = self.jira.lock(t.board);
        let _one_at_a_time = guard.lock().await;
        let api = self.jira.api.clone();
        let key = t.key.clone();
        blocking(move || match api.delete_issue(&account, &key) {
            Ok(()) | Err(JiraError::NotFound(_)) => Ok(()),
            Err(e) => Err(e.into()),
        })
        .await?;
        let gone = self.mutate(|state| Ok(state.board_mut(t.board)?.forget_issue(t.task.id)?))?;
        let ids: Vec<String> = gone
            .subtree()
            .into_iter()
            .filter_map(|x| Some(x.issue.as_ref()?.id.clone()))
            .collect();
        self.unsee(t.board, &ids);
        // Its parent's progress and an epic's stories change in Jira too: look again.
        self.poke(t.board);
        Ok(())
    }
}
