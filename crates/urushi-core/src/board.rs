//! Task boards: kanban boards of tasks, kept in columns.
//!
//! A board is a list of columns ("To do", "In progress", "Done"), and each column holds
//! its tasks top to bottom. A task is in exactly one place on one board, a column or
//! a parent task; moving or nesting it is the only way it changes place, and no
//! operation can lose or duplicate a task
//! (`tests/board_props.rs`). A task's subtasks are tasks too, nested inside it
//! (`tree.rs`, ADR 0015); a task has a color and tags (`labels.rs`) and attached files
//! (`attachments.rs`, ADR 0016), and can name the thread that was started for it. Columns can be added, recolored, reordered and removed
//! (`columns.rs`); new boards start with [`DEFAULT_COLUMNS`]. Boards are
//! organization only: they never touch git, terminals or files (the daemon stores
//! attachments' bytes). Ids and timestamps are passed
//! in by the daemon.
//!
//! A board can also follow a Jira board (`jira.rs`, `sync.rs`, ADR 0020): its content
//! then comes from Jira, and changes Jira owns are refused here with
//! [`BoardError::FollowsJira`]. Any task can have a [`TaskKind`] (`kind.rs`).

use serde::{Deserialize, Serialize};

use crate::{AttachmentId, BoardId, ColumnId, TaskId, ThreadId, Timestamp};

mod attachments;
mod columns;
mod jira;
mod kind;
mod labels;
mod prompt;
mod sync;
mod tree;

pub use attachments::{
    Attachment, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS, MAX_FILE_NAME, file_name, is_image,
    media_type,
};
pub use columns::MAX_COLUMNS;
pub use jira::{
    BoardSource, Epic, Issue, JiraBoard, JiraColumn, StatusCategory, SyncState, SyncedIssue,
};
pub use kind::TaskKind;
pub use labels::{LabelColor, MAX_TAG, MAX_TAGS, tag_color, task_tags};
pub use prompt::{MAX_PROMPT_BYTES, task_prompt};
pub use sync::{MAX_SYNCED_ISSUES, MergeReport};
pub use tree::{MAX_DEPTH, MAX_SUBTASKS};

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
    /// A task can't go under itself or one of its own subtasks.
    #[error("a task can't be a subtask of itself or of its own subtasks")]
    Cycle,
    /// Subtasks would be nested more than [`MAX_DEPTH`] levels deep.
    #[error("subtasks can be nested at most {MAX_DEPTH} levels deep")]
    TooDeep,
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
    /// The attachment isn't on this board.
    #[error("no attachment {0} on this board")]
    NoSuchAttachment(AttachmentId),
    /// The task already has [`MAX_ATTACHMENTS`] attachments.
    #[error("a task can have at most {MAX_ATTACHMENTS} attachments")]
    TooManyAttachments,
    /// A file name was blank.
    #[error("an attachment needs a file name")]
    EmptyFileName,
    /// More than [`MAX_TAGS`] tags.
    #[error("a task can have at most {MAX_TAGS} tags")]
    TooManyTags,
    /// The board follows Jira, which owns this change.
    #[error("{0} isn't possible on a Jira board: change it in Jira")]
    FollowsJira(&'static str),
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
    /// Where its content comes from, when it follows Jira.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<BoardSource>,
    /// The epics its Jira issues belong to, by key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub epics: Vec<Epic>,
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
    /// On a Jira board, the ids of the Jira statuses it shows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statuses: Vec<String>,
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
    /// When it was created. Subtasks from before ADR 0015 have none (0).
    #[serde(default = "Timestamp::unknown")]
    pub created_at: Timestamp,
    /// Checked off. Only meaningful for a subtask; a card's progress is its column.
    #[serde(default)]
    pub done: bool,
    /// Its color, if one was chosen.
    #[serde(default)]
    pub color: Option<LabelColor>,
    /// Free-text tags, cleaned up by [`task_tags`].
    #[serde(default)]
    pub tags: Vec<String>,
    /// Files attached to it, in the order they were added.
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// Smaller steps, in order: tasks nested inside this one.
    #[serde(default)]
    pub subtasks: Vec<Task>,
    /// The thread started for this task, if any. It may have been deleted since.
    #[serde(default)]
    pub thread: Option<ThreadId>,
    /// What sort of work it is, if said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<TaskKind>,
    /// The Jira issue it shows, on a Jira board.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<Issue>,
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
            done: false,
            color: None,
            tags: Vec::new(),
            attachments: Vec::new(),
            subtasks: Vec::new(),
            thread: None,
            kind: None,
            issue: None,
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
    /// Check or uncheck it (a subtask).
    pub done: Option<bool>,
    /// New kind; `Some(None)` clears it.
    pub kind: Option<Option<TaskKind>>,
}

impl TaskPatch {
    /// Whether it changes only what Urushi keeps for itself on a Jira board (the
    /// color), so it needs no write to Jira.
    pub fn is_local(&self) -> bool {
        self.title.is_none()
            && self.notes.is_none()
            && self.tags.is_none()
            && self.done.is_none()
            && self.kind.is_none()
    }
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
                    statuses: Vec::new(),
                })
                .collect(),
            source: None,
            epics: Vec::new(),
        })
    }

    /// Renames the board.
    pub fn rename(&mut self, name: &str) -> Result<(), BoardError> {
        self.local_only("renaming the board")?;
        self.name = board_name(name)?;
        Ok(())
    }

    /// How many cards the board holds (not counting subtasks).
    pub fn task_count(&self) -> usize {
        self.columns.iter().map(|c| c.tasks.len()).sum()
    }

    /// Adds `task` at the bottom of `column`.
    pub fn add_task(&mut self, column: ColumnId, task: Task) -> Result<(), BoardError> {
        self.local_only("adding a task this way")?;
        self.column_mut(column)?.tasks.push(task);
        Ok(())
    }

    /// Changes a task's (at any depth) title, notes, color, tags, kind and/or check
    /// mark. Nothing changes unless every given value is valid. On a Jira board only
    /// the color can change here; the rest is written to Jira and merged back.
    pub fn update_task(&mut self, id: TaskId, patch: &TaskPatch) -> Result<(), BoardError> {
        if !patch.is_local() {
            self.local_only("editing a task this way")?;
        }
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
        if let Some(done) = patch.done {
            task.done = done;
        }
        if let Some(kind) = patch.kind {
            task.kind = kind;
        }
        Ok(())
    }

    /// Moves a task to position `index` of `column` (counted without the task itself;
    /// clamped to the bottom). A subtask comes out of its parent onto the board, and
    /// loses its check mark.
    pub fn move_task(
        &mut self,
        id: TaskId,
        column: ColumnId,
        index: usize,
    ) -> Result<(), BoardError> {
        self.local_only("moving a task this way")?;
        // Check the destination first, so a failed move leaves the task where it was.
        self.column_mut(column)?;
        let mut task = self.detach(id)?;
        task.done = false;
        let tasks = &mut self.column_mut(column)?.tasks;
        tasks.insert(index.min(tasks.len()), task);
        Ok(())
    }

    pub(crate) fn column_mut(&mut self, id: ColumnId) -> Result<&mut Column, BoardError> {
        self.columns
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(BoardError::NoSuchColumn(id))
    }
}

#[cfg(test)]
mod tests;
