//! Task boards: creating, renaming and deleting boards; adding, editing, moving,
//! nesting and deleting their tasks, which may be subtasks of any depth (ADR 0015). The rules live in
//! `urushi_core::board`; this module finds the board, applies the change and persists
//! it. Boards never touch git or terminals (threads link themselves to tasks when they
//! are created, in `threads.rs`).

use urushi_core::{Board, BoardError, BoardId, ColumnId, LabelColor, Task, TaskId, TaskPatch};

use super::{Service, ServiceError, ServiceResult, State, new_uuid, now};

impl From<BoardError> for ServiceError {
    fn from(e: BoardError) -> Self {
        match e {
            BoardError::NoSuchColumn(id) => Self::NotFound(format!("column {id}")),
            BoardError::NoSuchTask(id) => Self::NotFound(format!("task {id}")),
            BoardError::NoSuchAttachment(id) => Self::NotFound(format!("attachment {id}")),
            BoardError::LastColumn => Self::Precondition(e.to_string()),
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
    /// Titles of subtasks to create with it.
    pub subtasks: &'a [String],
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

    /// The first prompt for an agent started for this task (or subtask of it).
    pub(super) fn task_prompt(&self, link: (TaskId, Option<TaskId>)) -> ServiceResult<String> {
        let id = self.link_target(link)?;
        let board = self
            .boards
            .iter()
            .find(|b| b.task(id).is_some())
            .ok_or_else(|| ServiceError::NotFound(format!("task {id}")))?;
        Ok(urushi_core::task_prompt(board, id)?)
    }

    /// Checks that a thread could be linked to this task (and subtask of it), and
    /// returns the task it would be linked to.
    pub(super) fn task_link_target(&self, link: (TaskId, Option<TaskId>)) -> ServiceResult<TaskId> {
        self.link_target(link)
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

    /// Adds a task at the bottom of a column.
    pub fn create_task(
        &self,
        board: BoardId,
        column: ColumnId,
        new: &NewTask<'_>,
    ) -> ServiceResult<Task> {
        let mut task = Task::new(TaskId::from_uuid(new_uuid()), new.title, new.notes, now())?;
        task.color = new.color;
        task.tags = urushi_core::board::task_tags(new.tags)?;
        if new.subtasks.len() > urushi_core::board::MAX_SUBTASKS {
            return Err(BoardError::TooManySubtasks.into());
        }
        task.subtasks = new
            .subtasks
            .iter()
            .map(|t| Task::new(TaskId::from_uuid(new_uuid()), t, "", now()))
            .collect::<Result<_, _>>()?;
        self.mutate(|state| {
            state.board_mut(board)?.add_task(column, task.clone())?;
            Ok(task)
        })
    }

    /// Changes a task's title, notes, color and/or tags (`None` keeps them).
    pub fn update_task(&self, task: TaskId, patch: &TaskPatch) -> ServiceResult<()> {
        self.mutate(|state| Ok(state.board_of_task(task)?.update_task(task, patch)?))
    }

    /// Moves a task to `index` in a column of its board.
    pub fn move_task(&self, task: TaskId, column: ColumnId, index: usize) -> ServiceResult<()> {
        self.mutate(|state| Ok(state.board_of_task(task)?.move_task(task, column, index)?))
    }

    /// Deletes a task.
    pub fn delete_task(&self, task: TaskId) -> ServiceResult<()> {
        let removed = self.mutate(|state| Ok(state.board_of_task(task)?.remove_task(task)?))?;
        self.forget_attachments(&super::attachments::of_task(&removed));
        Ok(())
    }

    /// Moves a task (a card or a subtask) under another task of its board.
    pub fn nest_task(&self, task: TaskId, parent: TaskId, index: usize) -> ServiceResult<()> {
        self.mutate(|state| Ok(state.board_of_task(task)?.nest_task(task, parent, index)?))
    }

    /// Adds a subtask at the end of a task's (at any depth) subtasks.
    pub fn create_subtask(&self, parent: TaskId, title: &str) -> ServiceResult<Task> {
        let subtask = Task::new(TaskId::from_uuid(new_uuid()), title, "", now())?;
        self.mutate(|state| {
            state
                .board_of_task(parent)?
                .add_subtask(parent, subtask.clone())?;
            Ok(subtask)
        })
    }
}
