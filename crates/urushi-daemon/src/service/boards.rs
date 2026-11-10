//! Task boards: creating, renaming, reordering and deleting boards; adding, editing, moving,
//! nesting and deleting their tasks, which may be subtasks of any depth (ADR 0015). The rules live in
//! `urushi_core::board`; this module finds the board, applies the change and persists
//! it. Boards never touch git or terminals (threads link themselves to tasks when they
//! are created, in `threads.rs`). On a board that follows Jira, task changes are
//! written to Jira first (`jira_write.rs`).

use urushi_core::{
    Board, BoardError, BoardId, ColumnId, IssueKey, LabelColor, Task, TaskId, TaskKind, TaskPatch,
};

use super::{Service, ServiceError, ServiceResult, State, new_uuid, now};

impl From<BoardError> for ServiceError {
    fn from(e: BoardError) -> Self {
        match e {
            BoardError::NoSuchColumn(id) => Self::NotFound(format!("column {id}")),
            BoardError::NoSuchTask(id) => Self::NotFound(format!("task {id}")),
            BoardError::NoSuchAttachment(id) => Self::NotFound(format!("attachment {id}")),
            BoardError::LastColumn | BoardError::FollowsJira(_) => {
                Self::Precondition(e.to_string())
            }
            _ => Self::Invalid(e.to_string()),
        }
    }
}

/// What a new task starts with.
#[derive(Clone, Copy, Debug, Default)]
pub struct NewTask<'a> {
    /// Title.
    pub title: &'a str,
    /// Notes; may be empty.
    pub notes: &'a str,
    /// Color, if any.
    pub color: Option<LabelColor>,
    /// Tags.
    pub tags: &'a [String],
    /// Subtasks to create with it, in order.
    pub subtasks: &'a [NewSubtask],
    /// Its kind, if said (on a Jira board, the type of issue to create).
    pub kind: Option<TaskKind>,
    /// On a Jira board, values for the new issue's other fields (by field id).
    pub jira_fields: &'a [(String, crate::jira::details::FieldValue)],
}

/// What a new subtask starts with: what its editor sets, as for a new task.
#[derive(Clone, Debug, Default)]
pub struct NewSubtask {
    /// Title.
    pub title: String,
    /// Notes; may be empty.
    pub notes: String,
    /// Color, if any.
    pub color: Option<LabelColor>,
    /// Tags.
    pub tags: Vec<String>,
    /// Its kind, if said (ignored on a Jira board, where it is a Sub-task issue).
    pub kind: Option<TaskKind>,
    /// On a Jira board, values for the new sub-task's other fields (by field id).
    pub jira_fields: Vec<(String, crate::jira::details::FieldValue)>,
}

impl NewSubtask {
    /// A subtask with only a title.
    pub fn titled(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// The task it starts as on a board Urushi keeps.
    fn task(&self) -> ServiceResult<Task> {
        let mut task = Task::new(
            TaskId::from_uuid(new_uuid()),
            &self.title,
            &self.notes,
            now(),
        )?;
        task.color = self.color;
        task.tags = urushi_core::board::task_tags(&self.tags)?;
        task.kind = self.kind;
        Ok(task)
    }
}

impl State {
    pub(super) fn board_mut(&mut self, id: BoardId) -> ServiceResult<&mut Board> {
        self.boards
            .iter_mut()
            .find(|b| b.id == id)
            .ok_or_else(|| ServiceError::NotFound(format!("board {id}")))
    }

    /// The board holding `task`.
    pub(super) fn board_of_task(&mut self, task: TaskId) -> ServiceResult<&mut Board> {
        self.boards
            .iter_mut()
            .find(|b| b.task(task).is_some())
            .ok_or_else(|| ServiceError::NotFound(format!("task {task}")))
    }

    /// The task a thread is linked to: `subtask` if given (it must be under `task`, not
    /// `task` itself), otherwise `task`.
    fn link_target(&self, (task, subtask): (TaskId, Option<TaskId>)) -> ServiceResult<TaskId> {
        let parent = self
            .boards
            .iter()
            .find_map(|b| b.task(task))
            .ok_or_else(|| ServiceError::NotFound(format!("task {task}")))?;
        match subtask {
            Some(id) if id == task || parent.find(id).is_none() => Err(ServiceError::NotFound(
                format!("subtask {id} of task {task}"),
            )),
            Some(id) => Ok(id),
            None => Ok(task),
        }
    }

    /// The first prompt for an agent started for this task (or subtask of it): the
    /// user's own message filled in for it (`custom`), else a reference to it when the
    /// agent can run the CLI at path `cli`, else a copy.
    pub(super) fn task_prompt(
        &self,
        link: (TaskId, Option<TaskId>),
        cli: Option<&str>,
        custom: Option<&str>,
    ) -> ServiceResult<String> {
        let id = self.link_target(link)?;
        let board = self
            .boards
            .iter()
            .find(|b| b.task(id).is_some())
            .ok_or_else(|| ServiceError::NotFound(format!("task {id}")))?;
        Ok(if let Some(template) = custom {
            urushi_core::custom_task_prompt(board, id, template, cli.unwrap_or("urushi"))?
        } else if let Some(cli) = cli {
            urushi_core::task_reference_prompt(board, id, cli)?
        } else {
            urushi_core::task_prompt(board, id)?
        })
    }

    /// Checks that a thread could be linked to this task (and subtask of it), and
    /// returns the task it would be linked to.
    pub(super) fn task_link_target(&self, link: (TaskId, Option<TaskId>)) -> ServiceResult<TaskId> {
        self.link_target(link)
    }

    /// The Jira issue key of the task a thread is started for (the subtask, when one
    /// is named), if it is a Jira issue: its branch is named after it (ADR 0026).
    pub(super) fn task_issue_key(
        &self,
        link: (TaskId, Option<TaskId>),
    ) -> ServiceResult<Option<IssueKey>> {
        let target = self.link_target(link)?;
        Ok(self
            .boards
            .iter()
            .find_map(|b| b.task(target))
            .and_then(|t| t.issue.as_ref())
            .and_then(|i| IssueKey::new(&i.key)))
    }
}

impl Service {
    /// Creates a board with the default columns, after the existing ones.
    pub fn create_board(&self, name: &str) -> ServiceResult<Board> {
        let board = Board::new(
            BoardId::from_uuid(new_uuid()),
            name,
            now(),
            std::array::from_fn(|_| ColumnId::from_uuid(new_uuid())),
        )?;
        self.mutate(|state| {
            state.boards.push(board.clone());
            Ok(board)
        })
    }

    /// Renames a board.
    pub fn rename_board(&self, id: BoardId, name: &str) -> ServiceResult<()> {
        self.mutate(|state| Ok(state.board_mut(id)?.rename(name)?))
    }

    /// Moves a board to `index` among the others (counted without it; clamped to the
    /// end). Boards are listed, and their tabs shown, in this order. Jira boards move
    /// like any other; nothing is sent to Jira.
    pub fn move_board(&self, id: BoardId, index: usize) -> ServiceResult<()> {
        self.mutate(|state| {
            state.board_mut(id)?;
            urushi_core::board::move_board(&mut state.boards, id, index);
            Ok(())
        })
    }

    /// Deletes a board and all its tasks.
    pub fn delete_board(&self, id: BoardId) -> ServiceResult<()> {
        let gone: Vec<_> = self.mutate(|state| {
            let board = state.board_mut(id)?;
            let gone = board.all_attachments().cloned().collect();
            state.boards.retain(|b| b.id != id);
            Ok(gone)
        })?;
        self.forget_attachments(&gone);
        Ok(())
    }

    /// Adds a task at the bottom of a column (on a Jira board, creates an issue).
    pub async fn create_task(
        &self,
        board: BoardId,
        column: ColumnId,
        new: &NewTask<'_>,
    ) -> ServiceResult<Task> {
        let follows_jira = self.read(|state| {
            Ok(state
                .boards
                .iter()
                .find(|b| b.id == board)
                .is_some_and(|b| b.jira().is_some()))
        })?;
        if follows_jira {
            return self.jira_create_task(board, column, new).await;
        }
        let mut task = Task::new(TaskId::from_uuid(new_uuid()), new.title, new.notes, now())?;
        task.color = new.color;
        task.tags = urushi_core::board::task_tags(new.tags)?;
        task.kind = new.kind;
        if new.subtasks.len() > urushi_core::board::MAX_SUBTASKS {
            return Err(BoardError::TooManySubtasks.into());
        }
        task.subtasks = new
            .subtasks
            .iter()
            .map(NewSubtask::task)
            .collect::<ServiceResult<_>>()?;
        self.mutate(|state| {
            state.board_mut(board)?.add_task(column, task.clone())?;
            Ok(task)
        })
    }

    /// Changes a task's title, notes, color, tags, kind and/or check mark (`None`
    /// keeps them).
    pub async fn update_task(&self, task: TaskId, patch: &TaskPatch) -> ServiceResult<()> {
        if let Some(t) = self.read(|state| state.jira_task(task))? {
            return self.jira_update_task(t, patch).await;
        }
        self.mutate(|state| Ok(state.board_of_task(task)?.update_task(task, patch)?))
    }

    /// Moves a task to `index` in a column of its board.
    pub async fn move_task(
        &self,
        task: TaskId,
        column: ColumnId,
        index: usize,
    ) -> ServiceResult<()> {
        if let Some(t) = self.read(|state| state.jira_task(task))? {
            return self.jira_move_task(t, column, index).await;
        }
        self.mutate(|state| Ok(state.board_of_task(task)?.move_task(task, column, index)?))
    }

    /// Deletes a task (never a Jira issue).
    pub fn delete_task(&self, task: TaskId) -> ServiceResult<()> {
        if let Some(t) = self.read(|state| state.jira_task(task))? {
            return Self::jira_delete_task(&t);
        }
        let removed = self.mutate(|state| Ok(state.board_of_task(task)?.remove_task(task)?))?;
        self.forget_attachments(&super::attachments::of_task(&removed));
        Ok(())
    }

    /// Moves a task (a card or a subtask) under another task of its board.
    pub fn nest_task(&self, task: TaskId, parent: TaskId, index: usize) -> ServiceResult<()> {
        self.mutate(|state| Ok(state.board_of_task(task)?.nest_task(task, parent, index)?))
    }

    /// Adds a subtask at the end of a task's (at any depth) subtasks.
    pub async fn create_subtask(&self, parent: TaskId, new: &NewSubtask) -> ServiceResult<Task> {
        if let Some(t) = self.read(|state| state.jira_task(parent))? {
            return self.jira_create_subtask(t, new).await;
        }
        let subtask = new.task()?;
        self.mutate(|state| {
            state
                .board_of_task(parent)?
                .add_subtask(parent, subtask.clone())?;
            Ok(subtask)
        })
    }
}
