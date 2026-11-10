//! Why a board operation was refused: one error type for every rule in this module,
//! so the daemon maps each to a wire error code in one place.

use super::{MAX_ATTACHMENTS, MAX_COLUMNS, MAX_DEPTH, MAX_QUICK_FILTERS, MAX_SUBTASKS, MAX_TAGS};
use crate::{AttachmentId, ColumnId, QuickFilterId, TaskId};

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
    /// The quick filter isn't on this board.
    #[error("no quick filter {0} on this board")]
    NoSuchQuickFilter(QuickFilterId),
    /// The board already has [`MAX_QUICK_FILTERS`] quick filters of Urushi's own.
    #[error("a board can have at most {MAX_QUICK_FILTERS} quick filters")]
    TooManyQuickFilters,
    /// A quick filter's name was blank.
    #[error("a quick filter needs a name")]
    EmptyFilterName,
    /// A quick filter would let everything through.
    #[error("a quick filter needs something to filter by")]
    EmptyQuickFilter,
    /// The rule doesn't suit the board: a Jira board's quick filters are JQL, which
    /// Jira evaluates; other boards filter by search, type and tag.
    #[error("{0}")]
    WrongQuickRule(&'static str),
}
