//! Task boards: kanban boards of tasks, kept in columns.
//!
//! A board is a list of columns ("To do", "In progress", "Done"), and each column holds
//! its tasks top to bottom. A task is in exactly one column of one board; moving it is
//! the only way it changes column, and no operation can lose or duplicate a task
//! (`tests/board_props.rs`). A task can have a checklist of subtasks (`subtasks.rs`),
//! a color and tags (`labels.rs`), and a task or subtask can name the thread that was
//! started for it. Columns can be added, recolored, reordered and removed
//! (`columns.rs`); new boards start with [`DEFAULT_COLUMNS`]. Boards are
//! organization only: they never touch git or terminals. Ids and timestamps are passed
//! in by the daemon.

use serde::{Deserialize, Serialize};

use crate::{BoardId, ColumnId, SubtaskId, TaskId, ThreadId, Timestamp};

mod columns;
mod labels;
mod prompt;
mod subtasks;

pub use columns::MAX_COLUMNS;
pub use labels::{LabelColor, MAX_TAG, MAX_TAGS, tag_color, task_tags};
pub use prompt::{MAX_PROMPT_BYTES, task_prompt};
pub use subtasks::{MAX_SUBTASKS, Subtask};

/// Longest board or column name, in characters.
pub const MAX_NAME: usize = 80;
/// Longest task title, in characters.
pub const MAX_TITLE: usize = 300;
/// Longest task notes, in characters.
pub const MAX_NOTES: usize = 10_000;

/// Names of the columns a new board starts with.
pub const DEFAULT_COLUMNS: [&str; 3] = ["To do", "In progress", "Done"];

/// Why a board operation was refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BoardError {
    /// A board name was blank.
    #[error("a board name must not be empty")]
    EmptyName,
    /// A name or text was too long.
    #[error("{what} must be at most {max} characters")]
    TooLong {
        /// What was too long.
        what: &'static str,
        /// The limit.
        max: usize,
    },
    /// A task title was blank.
    #[error("a task needs a title")]
    EmptyTitle,
    /// The column isn't on this board.
    #[error("no column {0} on this board")]
    NoSuchColumn(ColumnId),
    /// The task isn't on this board.
    #[error("no task {0} on this board")]
    NoSuchTask(TaskId),
    /// The subtask isn't on this board.
    #[error("no subtask {0} on this board")]
    NoSuchSubtask(SubtaskId),
    /// The task already has [`MAX_SUBTASKS`] subtasks.
    #[error("a task can have at most {MAX_SUBTASKS} subtasks")]
    TooManySubtasks,
    /// A column name was blank.
    #[error("a column name must not be empty")]
    EmptyColumnName,
    /// The board already has [`MAX_COLUMNS`] columns.
    #[error("a board can have at most {MAX_COLUMNS} columns")]
    TooManyColumns,
    /// The board's only column can't be removed.
    #[error("a board needs at least one column")]
    LastColumn,
    /// More than [`MAX_TAGS`] tags.
    #[error("a task can have at most {MAX_TAGS} tags")]
    TooManyTags,
}

/// A kanban board.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    /// Identity.
    pub id: BoardId,
    /// Display name (shown on its tab).
    pub name: String,
    /// When it was created.
    pub created_at: Timestamp,
    /// Columns, left to right.
    pub columns: Vec<Column>,
}

/// One column of a board.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    /// Identity.
    pub id: ColumnId,
    /// Display name.
    pub name: String,
    /// Its color, if one was chosen.
    #[serde(default)]
    pub color: Option<LabelColor>,
    /// Tasks, top to bottom.
    #[serde(default)]
    pub tasks: Vec<Task>,
}

/// A task: a card on a board.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Identity.
    pub id: TaskId,
    /// One-line summary.
    pub title: String,
    /// Free-form details; may be empty.
    #[serde(default)]
    pub notes: String,
    /// When it was created.
    pub created_at: Timestamp,
    /// Its color, if one was chosen.
    #[serde(default)]
    pub color: Option<LabelColor>,
    /// Free-text tags, cleaned up by [`task_tags`].
    #[serde(default)]
    pub tags: Vec<String>,
    /// A checklist of smaller steps, in order.
    #[serde(default)]
    pub subtasks: Vec<Subtask>,
    /// The thread started for this task, if any. It may have been deleted since.
    #[serde(default)]
    pub thread: Option<ThreadId>,
}

impl Task {
    /// A task with a validated title and notes.
    pub fn new(
        id: TaskId,
        title: &str,
        notes: &str,
        created_at: Timestamp,
    ) -> Result<Self, BoardError> {
        Ok(Self {
            id,
            title: task_title(title)?,
            notes: task_notes(notes)?,
            created_at,
            color: None,
            tags: Vec::new(),
            subtasks: Vec::new(),
            thread: None,
        })
    }
}

/// A change to a task's own fields; `None` keeps the current value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskPatch {
    /// New title.
    pub title: Option<String>,
    /// New notes.
    pub notes: Option<String>,
    /// New color; `Some(None)` clears it.
    pub color: Option<Option<LabelColor>>,
    /// New tags (replacing all of them).
    pub tags: Option<Vec<String>>,
}

/// A board name, with runs of whitespace collapsed. Blank and over-long names are refused.
pub fn board_name(name: &str) -> Result<String, BoardError> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(BoardError::EmptyName);
    }
    within(name, "a board name", MAX_NAME)
}

/// A task title, trimmed and on one line.
pub fn task_title(title: &str) -> Result<String, BoardError> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return Err(BoardError::EmptyTitle);
    }
    within(title, "a task title", MAX_TITLE)
}

/// Task notes, with trailing whitespace removed.
pub fn task_notes(notes: &str) -> Result<String, BoardError> {
    within(notes.trim_end().to_owned(), "task notes", MAX_NOTES)
}

fn within(text: String, what: &'static str, max: usize) -> Result<String, BoardError> {
    if text.chars().count() > max {
        Err(BoardError::TooLong { what, max })
    } else {
        Ok(text)
    }
}

impl Board {
    /// A new, empty board with the [`DEFAULT_COLUMNS`], whose ids are `column_ids`.
    pub fn new(
        id: BoardId,
        name: &str,
        created_at: Timestamp,
        column_ids: [ColumnId; DEFAULT_COLUMNS.len()],
    ) -> Result<Self, BoardError> {
        Ok(Self {
            id,
            name: board_name(name)?,
            created_at,
            columns: DEFAULT_COLUMNS
                .iter()
                .zip(column_ids)
                .map(|(name, id)| Column {
                    id,
                    name: (*name).to_owned(),
                    color: None,
                    tasks: Vec::new(),
                })
                .collect(),
        })
    }

    /// Renames the board.
    pub fn rename(&mut self, name: &str) -> Result<(), BoardError> {
        self.name = board_name(name)?;
        Ok(())
    }

    /// The task with this id, if it's on this board.
    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.columns
            .iter()
            .flat_map(|c| &c.tasks)
            .find(|t| t.id == id)
    }

    /// How many tasks the board holds.
    pub fn task_count(&self) -> usize {
        self.columns.iter().map(|c| c.tasks.len()).sum()
    }

    /// Adds `task` at the bottom of `column`.
    pub fn add_task(&mut self, column: ColumnId, task: Task) -> Result<(), BoardError> {
        self.column_mut(column)?.tasks.push(task);
        Ok(())
    }

    /// Changes a task's title, notes, color and/or tags. Nothing changes unless every
    /// given value is valid.
    pub fn update_task(&mut self, id: TaskId, patch: &TaskPatch) -> Result<(), BoardError> {
        let title = patch.title.as_deref().map(task_title).transpose()?;
        let notes = patch.notes.as_deref().map(task_notes).transpose()?;
        let tags = patch.tags.as_deref().map(task_tags).transpose()?;
        let task = self.task_mut(id)?;
        if let Some(title) = title {
            task.title = title;
        }
        if let Some(notes) = notes {
            task.notes = notes;
        }
        if let Some(color) = patch.color {
            task.color = color;
        }
        if let Some(tags) = tags {
            task.tags = tags;
        }
        Ok(())
    }

    /// Moves a task to position `index` of `column` (counted without the task itself;
    /// clamped to the bottom).
    pub fn move_task(
        &mut self,
        id: TaskId,
        column: ColumnId,
        index: usize,
    ) -> Result<(), BoardError> {
        // Check the destination first, so a failed move leaves the task where it was.
        self.column_mut(column)?;
        let task = self.remove_task(id)?;
        let tasks = &mut self.column_mut(column)?.tasks;
        tasks.insert(index.min(tasks.len()), task);
        Ok(())
    }

    /// Removes a task and returns it.
    pub fn remove_task(&mut self, id: TaskId) -> Result<Task, BoardError> {
        for column in &mut self.columns {
            if let Some(at) = column.tasks.iter().position(|t| t.id == id) {
                return Ok(column.tasks.remove(at));
            }
        }
        Err(BoardError::NoSuchTask(id))
    }

    pub(crate) fn column_mut(&mut self, id: ColumnId) -> Result<&mut Column, BoardError> {
        self.columns
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(BoardError::NoSuchColumn(id))
    }

    pub(crate) fn task_mut(&mut self, id: TaskId) -> Result<&mut Task, BoardError> {
        self.columns
            .iter_mut()
            .flat_map(|c| &mut c.tasks)
            .find(|t| t.id == id)
            .ok_or(BoardError::NoSuchTask(id))
    }
}

#[cfg(test)]
mod tests;
