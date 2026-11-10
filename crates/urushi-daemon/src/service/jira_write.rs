//! Writing changes to Jira boards through to Jira (ADR 0020).
//!
//! A change to a task on a Jira board is made in Jira first: a field edit, a
//! transition, a new issue, a new rank. Then the issue is read back and merged, so the
//! board shows what Jira did (automation, resolutions, a status the transition chose).
//! If Jira refuses, the request fails and nothing changes here. Only a task's color is
//! Urushi's own, and changes without Jira.
//!
//! Deleting issues, nesting them and changing their type are left to Jira.

use urushi_core::board::{
    JiraBoard, StatusCategory, SyncedIssue, task_notes, task_tags, task_title,
};
use urushi_core::{BoardId, ColumnId, Task, TaskId, TaskKind, TaskPatch};

use super::boards::NewTask;
use super::{Service, ServiceError, ServiceResult, State, blocking, new_uuid};
use crate::jira::details::FieldValue;
use crate::jira::{Account, IssueEdit, IssueFields, JiraApi, NewIssue, Rank};

/// A task on a Jira board, with what writing to Jira needs.
pub(super) struct JiraTask {
    board: BoardId,
    jira: JiraBoard,
    task: Task,
    key: String,
    /// It is a subtask under another issue.
    nested: bool,
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
fn labels(tags: &[String]) -> ServiceResult<Vec<String>> {
    let tags = task_tags(tags)?;
    if let Some(bad) = tags.iter().find(|t| t.contains(char::is_whitespace)) {
        return Err(ServiceError::Invalid(format!(
            "Jira labels can't contain spaces: {bad:?}"
        )));
    }
    Ok(tags)
}

fn refused(what: &str) -> ServiceError {
    ServiceError::Precondition(format!("{what} on a Jira board: do it in Jira"))
}

/// A new card's issue and subtasks, to create in Jira.
struct Creation {
    jira: JiraBoard,
    kind: TaskKind,
    summary: String,
    notes: String,
    labels: Vec<String>,
    subtasks: Vec<String>,
    /// The statuses of the column it was added to.
    statuses: Vec<String>,
    /// Values for its other fields (by id), written as their kinds say.
    extra: Vec<(String, FieldValue)>,
}

impl Creation {
    /// Creates the issue and its subtasks, moves the issue to its column if its
    /// workflow allows, and reads them all back (the issue first). Blocking.
    fn run(self, api: &dyn JiraApi, account: &Account) -> ServiceResult<Vec<SyncedIssue>> {
        let fields = IssueFields::of(&self.jira);
        let new = |kind, parent_key, summary, notes, labels| NewIssue {
            project_key: self.jira.project_key.clone(),
            kind,
            parent_key,
            summary,
            notes,
            labels,
            fields: Vec::new(),
        };
        let mut issue = new(
            self.kind,
            None,
            self.summary.clone(),
            self.notes.clone(),
            self.labels.clone(),
        );
        if !self.extra.is_empty() {
            let meta = api.create_fields(account, &self.jira.project_key, self.kind)?;
            issue.fields = super::jira_edit::written(&meta, &self.extra)?;
        }
        let key = api.create(account, &issue)?;
        let mut keys = vec![key.clone()];
        for summary in &self.subtasks {
            let sub = new(
                TaskKind::Subtask,
                Some(key.clone()),
                summary.clone(),
                String::new(),
                Vec::new(),
            );
            keys.push(api.create(account, &sub)?);
        }
        let created = api.issue(account, &key, &fields)?;
        if !self.statuses.is_empty() && !self.statuses.contains(&created.issue.status_id) {
            // Best effort: a new issue starts where its workflow says.
            let to = api
                .transitions(account, &key)?
                .into_iter()
                .find(|t| self.statuses.contains(&t.to_status));
            if let Some(to) = to {
                api.transition(account, &key, &to.id)?;
            }
        }
        keys.iter()
            .map(|k| Ok(api.issue(account, k, &fields)?))
            .collect()
    }
}

impl Service {
    /// Merges issues read back after a write into their board.
    pub(super) fn merge_written(
        &self,
        board: BoardId,
        issues: Vec<SyncedIssue>,
    ) -> ServiceResult<()> {
        self.mutate_if_changed(|state| {
            let b = state.board_mut(board)?;
            Ok(
                b.merge_issues(issues, false, || TaskId::from_uuid(new_uuid()))
                    .changed,
            )
        })
        .map(drop)
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
        .await?;
        self.merge_written(t.board, vec![read])
    }

    /// Creates an issue (and its subtasks) for a new card in `column`, moved there.
    pub(super) async fn jira_create_task(
        &self,
        board: BoardId,
        column: ColumnId,
        new: &NewTask<'_>,
    ) -> ServiceResult<Task> {
        let kind = new.kind.unwrap_or(TaskKind::Task);
        if matches!(kind, TaskKind::Epic | TaskKind::Subtask) {
            return Err(ServiceError::Invalid(format!(
                "a {} isn't a card on a Jira board",
                kind.name()
            )));
        }
        let summary = task_title(new.title)?;
        let notes = task_notes(new.notes)?;
        let tags = labels(new.tags)?;
        let subtasks = new
            .subtasks
            .iter()
            .map(|s| task_title(s))
            .collect::<Result<Vec<_>, _>>()?;
        let (jira, statuses) = self.read(|state| {
            let jira = state.jira_board(board)?;
            let statuses = state
                .boards
                .iter()
                .find(|b| b.id == board)
                .and_then(|b| b.column(column))
                .map(|c| c.statuses.clone())
                .ok_or_else(|| ServiceError::NotFound(format!("column {column}")))?;
            Ok((jira, statuses))
        })?;
        if jira.project_key.is_empty() {
            return Err(ServiceError::Precondition(
                "this Jira board has no project to create issues in".into(),
            ));
        }
        let account = self.jira_account()?;
        let guard = self.jira.lock(board);
        let _one_at_a_time = guard.lock().await;
        let api = self.jira.api.clone();
        let creation = Creation {
            jira,
            kind,
            summary,
            notes,
            labels: tags,
            subtasks,
            statuses,
            extra: new.jira_fields.to_vec(),
        };
        let read = blocking(move || creation.run(api.as_ref(), &account)).await?;
        let issue_id = read.first().map(|i| i.issue.id.clone()).unwrap_or_default();
        let key = read
            .first()
            .map(|i| i.issue.key.clone())
            .unwrap_or_default();
        self.merge_written(board, read)?;
        let id = self.read(|state| {
            state
                .boards
                .iter()
                .find(|b| b.id == board)
                .and_then(|b| b.task_for_issue(&issue_id))
                .map(|t| t.id)
                .ok_or_else(|| {
                    ServiceError::Precondition(format!(
                        "Jira made {key}, but its status isn't on this board"
                    ))
                })
        })?;
        if new.color.is_some() {
            let local = TaskPatch {
                color: Some(new.color),
                ..TaskPatch::default()
            };
            self.mutate(|state| Ok(state.board_mut(board)?.update_task(id, &local)?))?;
        }
        self.read(|state| {
            state
                .boards
                .iter()
                .find_map(|b| b.task(id).cloned())
                .ok_or_else(|| ServiceError::NotFound(format!("task {id}")))
        })
    }

    /// Creates a Jira subtask under a card.
    pub(super) async fn jira_create_subtask(
        &self,
        t: JiraTask,
        title: &str,
    ) -> ServiceResult<Task> {
        if t.nested {
            return Err(refused("a subtask can't have subtasks"));
        }
        let summary = task_title(title)?;
        let account = self.jira_account()?;
        let guard = self.jira.lock(t.board);
        let _one_at_a_time = guard.lock().await;
        let api = self.jira.api.clone();
        let (parent, jira) = (t.key.clone(), t.jira.clone());
        let read = blocking(move || {
            let key = api.create(
                &account,
                &NewIssue {
                    project_key: parent
                        .rsplit_once('-')
                        .map_or(jira.project_key.clone(), |(p, _)| p.to_owned()),
                    kind: TaskKind::Subtask,
                    parent_key: Some(parent),
                    summary,
                    notes: String::new(),
                    labels: Vec::new(),
                    fields: Vec::new(),
                },
            )?;
            Ok(api.issue(&account, &key, &IssueFields::of(&jira))?)
        })
        .await?;
        let issue_id = read.issue.id.clone();
        self.merge_written(t.board, vec![read])?;
        self.read(|state| {
            state
                .boards
                .iter()
                .find(|b| b.id == t.board)
                .and_then(|b| b.task_for_issue(&issue_id))
                .cloned()
                .ok_or_else(|| ServiceError::NotFound("the new subtask".into()))
        })
    }

    /// Issues aren't deleted from Urushi.
    pub(super) fn jira_delete_task(t: &JiraTask) -> ServiceResult<()> {
        Err(ServiceError::Precondition(format!(
            "{} is a Jira issue: delete it in Jira",
            t.key
        )))
    }
}
