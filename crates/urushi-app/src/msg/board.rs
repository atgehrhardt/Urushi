//! Messages for acting on a board's cards and columns from outside the task editor
//! (their context menus, opened with a secondary click, i.e. a right or two-finger
//! click), the tag filter, the editor's colors and tags, and attachments.

/// Something done to a card on the board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardMsg {
    /// Open a task or subtask in the editor (from another task's editor, keeping that
    /// editor, unsaved edits too, for Back).
    OpenTask(String),
    /// In the task editor: back to the task it was opened from.
    EditorBack,
    /// In the task editor, asked whether to discard unsaved edits: no, keep editing.
    KeepEditing,
    /// In the task editor, asked whether to discard unsaved edits: yes, close.
    DiscardEdits,
    /// Take a subtask out of its parent, onto the board in its card's column.
    MakeStandalone(String),
    /// Ask which task to put a task under ("Make subtask of…").
    OpenNest(String),
    /// Put a task under another, as its last subtask.
    Nest {
        /// The task to move.
        task: String,
        /// Its new parent.
        parent: String,
    },
    /// Delete a task right away (from its menu, without opening it).
    DeleteTask(String),
    /// Move a task to the bottom of another column.
    MoveTask {
        /// Task id.
        task: String,
        /// Column id.
        column: String,
    },
    /// Color a task (a wire `LabelColor` value; 0 clears it).
    ColorTask {
        /// Task id.
        task: String,
        /// The color.
        color: i32,
    },
    /// Color a column (0 clears it).
    ColorColumn {
        /// Column id.
        column: String,
        /// The color.
        color: i32,
    },
    /// Move a column to `index` among the board's columns.
    MoveColumn {
        /// Column id.
        column: String,
        /// New position.
        index: u32,
    },
    /// Ask before removing a column.
    OpenDeleteColumn(String),
    /// Confirmed: remove the column (its tasks move next door).
    ConfirmDeleteColumn,
    /// Show only tasks with this tag, or all with `None`.
    FilterTag(Option<String>),
    /// In the task editor: choose the color (0: none).
    PickColor(i32),
    /// In the task editor: add a tag (a suggestion).
    AddTag(String),
    /// In the task editor: remove a tag.
    RemoveTag(String),
    /// Choose files to attach to a task (the system file picker).
    AddFiles(String),
    /// A file read from disk, to attach to a task.
    Attach {
        /// Task id.
        task: String,
        /// The file's name.
        name: String,
        /// Its contents.
        bytes: Vec<u8>,
    },
    /// A file couldn't be attached (too big, unreadable); the reason is shown.
    AttachFailed(String),
    /// Open an attachment with the system's app for it.
    OpenAttachment(String),
    /// Delete an attachment.
    DeleteAttachment(String),
    /// In the task editor: choose the kind (a wire `TaskKind` value; 0: none).
    PickKind(i32),
    /// Show only a Jira board's tasks in this epic (by key), or all with `None`.
    FilterEpic(Option<String>),
}
