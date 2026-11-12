//! Creating issues on Jira boards (ADR 0020): a new card's issue (epics too, ADR 0027)
//! with its sub-tasks, and a sub-task under an existing card, each with the details its editor set
//! (description, labels, the site's own fields). As with every Jira write, the issues
//! are made in Jira first and read back; only a task's color is Urushi's own, and is
//! set here afterwards.

use urushi_core::board::{JiraBoard, SyncedIssue, task_notes, task_title};
use urushi_core::{BoardId, ColumnId, LabelColor, Task, TaskId, TaskKind, TaskPatch};

use super::boards::{NewSubtask, NewTask};
use super::jira_write::{JiraTask, labels, refused};
use super::{Service, ServiceError, ServiceResult, blocking};
use crate::jira::details::FieldValue;
use crate::jira::{Account, IssueFields, JiraApi, NewIssue};

/// Why an epic gets no sub-tasks: in Jira, an epic's children are standard issues
/// (stories, tasks, bugs) naming it as their parent, and sub-tasks sit under those.
const EPIC_SUBTASKS: &str = "a Jira epic holds stories, not sub-tasks";

/// An issue's own text, checked as Jira will want it.
struct IssueText {
    summary: String,
    notes: String,
    labels: Vec<String>,
    /// Values for its other fields (by id), written as their kinds say.
    extra: Vec<(String, FieldValue)>,
}

impl IssueText {
    fn of(
        title: &str,
        notes: &str,
        tags: &[String],
        extra: &[(String, FieldValue)],
    ) -> ServiceResult<Self> {
        Ok(Self {
            summary: task_title(title)?,
            notes: task_notes(notes)?,
            labels: labels(tags)?,
            extra: extra.to_vec(),
        })
    }

    fn sub(new: &NewSubtask) -> ServiceResult<Self> {
        Self::of(&new.title, &new.notes, &new.tags, &new.jira_fields)
    }

    /// The issue to create, its other fields written as `meta` (read once per kind,
    /// and only when there are any) says.
    fn issue(
        self,
        api: &dyn JiraApi,
        account: &Account,
        (project, kind, parent): (&str, TaskKind, Option<String>),
    ) -> ServiceResult<NewIssue> {
        let fields = if self.extra.is_empty() {
            Vec::new()
        } else {
            let meta = api.create_fields(account, project, kind)?;
            super::jira_edit::written(&meta, &self.extra)?
        };
        Ok(NewIssue {
            project_key: project.to_owned(),
            kind,
            parent_key: parent,
            summary: self.summary,
            notes: self.notes,
            labels: self.labels,
            fields,
        })
    }
}

/// A new card's issue and sub-tasks, to create in Jira.
struct Creation {
    jira: JiraBoard,
    kind: TaskKind,
    issue: IssueText,
    subtasks: Vec<IssueText>,
    /// The statuses of the column it was added to.
    statuses: Vec<String>,
}

impl Creation {
    /// Creates the issue and its sub-tasks, moves the issue to its column if its
    /// workflow allows, and reads them all back (the issue first, then its sub-tasks in
    /// order). Blocking.
    fn run(self, api: &dyn JiraApi, account: &Account) -> ServiceResult<Vec<SyncedIssue>> {
        let fields = IssueFields::of(&self.jira);
        let project = self.jira.project_key.as_str();
        let issue = self.issue.issue(api, account, (project, self.kind, None))?;
        let key = api.create(account, &issue)?;
        let mut keys = vec![key.clone()];
        for sub in self.subtasks {
            let sub = sub.issue(
                api,
                account,
                (project, TaskKind::Subtask, Some(key.clone())),
            )?;
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
    /// Creates an issue (and its sub-tasks) for a new card in `column`, moved there.
    pub(super) async fn jira_create_task(
        &self,
        board: BoardId,
        column: ColumnId,
        new: &NewTask<'_>,
    ) -> ServiceResult<Task> {
        let kind = new.kind.unwrap_or(TaskKind::Task);
        if kind == TaskKind::Subtask {
            return Err(ServiceError::Invalid(
                "a subtask isn't a card on a Jira board".into(),
            ));
        }
        if kind == TaskKind::Epic && !new.subtasks.is_empty() {
            return Err(ServiceError::Invalid(EPIC_SUBTASKS.into()));
        }
        let issue = IssueText::of(new.title, new.notes, new.tags, new.jira_fields)?;
        let subtasks = new
            .subtasks
            .iter()
            .map(IssueText::sub)
            .collect::<ServiceResult<Vec<_>>>()?;
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
        let account = self.jira_account(board)?;
        let guard = self.jira.lock(board);
        let _one_at_a_time = guard.lock().await;
        let api = self.jira.api.clone();
        let creation = Creation {
            jira,
            kind,
            issue,
            subtasks,
            statuses,
        };
        let read = blocking(move || creation.run(api.as_ref(), &account)).await?;
        let ids: Vec<String> = read.iter().map(|i| i.issue.id.clone()).collect();
        let key = read
            .first()
            .map(|i| i.issue.key.clone())
            .unwrap_or_default();
        self.merge_written(board, read)?;
        let tasks = self.tasks_for_issues(board, &ids)?;
        let Some(Some(id)) = tasks.first().copied() else {
            return Err(ServiceError::Precondition(format!(
                "Jira made {key}, but this board doesn't show it"
            )));
        };
        // Colors are Urushi's own.
        let colors = std::iter::once(new.color).chain(new.subtasks.iter().map(|s| s.color));
        for (task, color) in tasks.iter().zip(colors) {
            if let (Some(task), Some(color)) = (task, color) {
                self.set_color(board, *task, color)?;
            }
        }
        self.task_by_id(id)
    }

    /// Creates a Jira sub-task under a card.
    pub(super) async fn jira_create_subtask(
        &self,
        t: JiraTask,
        new: &NewSubtask,
    ) -> ServiceResult<Task> {
        if t.nested {
            return Err(refused("a subtask can't have subtasks"));
        }
        if t.task.kind == Some(TaskKind::Epic) {
            return Err(ServiceError::Invalid(EPIC_SUBTASKS.into()));
        }
        let text = IssueText::sub(new)?;
        let account = self.jira_account(t.board)?;
        let guard = self.jira.lock(t.board);
        let _one_at_a_time = guard.lock().await;
        let api = self.jira.api.clone();
        let (parent, jira) = (t.key.clone(), t.jira.clone());
        let read = blocking(move || {
            let project = parent
                .rsplit_once('-')
                .map_or(jira.project_key.clone(), |(p, _)| p.to_owned());
            let issue = text.issue(
                api.as_ref(),
                &account,
                (&project, TaskKind::Subtask, Some(parent)),
            )?;
            let key = api.create(&account, &issue)?;
            Ok(api.issue(&account, &key, &IssueFields::of(&jira))?)
        })
        .await?;
        let issue_id = read.issue.id.clone();
        self.merge_written(t.board, vec![read])?;
        let id = self
            .tasks_for_issues(t.board, &[issue_id])?
            .first()
            .copied()
            .flatten()
            .ok_or_else(|| ServiceError::NotFound("the new subtask".into()))?;
        if let Some(color) = new.color {
            self.set_color(t.board, id, color)?;
        }
        self.task_by_id(id)
    }

    /// The tasks showing these issues on `board`, in order (`None` for one not shown).
    fn tasks_for_issues(
        &self,
        board: BoardId,
        ids: &[String],
    ) -> ServiceResult<Vec<Option<TaskId>>> {
        self.read(|state| {
            let b = state
                .boards
                .iter()
                .find(|b| b.id == board)
                .ok_or_else(|| ServiceError::NotFound(format!("board {board}")))?;
            Ok(ids
                .iter()
                .map(|i| b.task_for_issue(i).map(|t| t.id))
                .collect())
        })
    }

    /// Sets a task's color, which only Urushi keeps.
    fn set_color(&self, board: BoardId, task: TaskId, color: LabelColor) -> ServiceResult<()> {
        let local = TaskPatch {
            color: Some(Some(color)),
            ..TaskPatch::default()
        };
        self.mutate(|state| Ok(state.board_mut(board)?.update_task(task, &local)?))
    }

    fn task_by_id(&self, id: TaskId) -> ServiceResult<Task> {
        self.read(|state| {
            state
                .boards
                .iter()
                .find_map(|b| b.task(id).cloned())
                .ok_or_else(|| ServiceError::NotFound(format!("task {id}")))
        })
    }
}
