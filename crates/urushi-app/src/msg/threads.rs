//! Messages about a thread's unread mark and notes, and a project's color (UR-62,
//! ADR 0048), the notes editor's formatting controls and color (UR-72, ADR 0058), and
//! shelving a thread (UR-75, ADR 0060).
//! Each becomes a request to the daemon or an edit in the open editor
//! (`update/thread_extras.rs`, `update/notes_format.rs`).

/// Something done to a thread's extras or a project's color.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadMsg {
    /// Mark a thread unread (`true`) or read.
    SetUnread {
        /// Thread id.
        thread: String,
        /// Unread or read.
        unread: bool,
    },
    /// Shelve a thread: stop its terminals, keeping its worktree and tabs (ADR 0060).
    /// While an agent works in it, asks first unless `confirmed`.
    Shelve {
        /// Thread id.
        thread: String,
        /// The user already said yes.
        confirmed: bool,
    },
    /// Unshelve a thread: start its terminals again.
    Unshelve(String),
    /// Open the notes editor for a thread.
    OpenNotes(String),
    /// A formatting control in the open notes editor.
    FormatNotes(NoteFormat),
    /// Pick the color of the notes in the open editor (a wire `LabelColor`).
    NotesColor(i32),
    /// Color a project's sidebar section with a wire `LabelColor` (0: none).
    SetProjectColor {
        /// Project id.
        project: String,
        /// Wire color.
        color: i32,
    },
}

/// What a notes formatting control does: wrap the selection in Markdown marks, or put a
/// marker on the selected lines (`urushi_core::markdown::edit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteFormat {
    /// `**bold**`.
    Bold,
    /// `_italic_`.
    Italic,
    /// `~~strikethrough~~`.
    Strike,
    /// `` `code` ``.
    Code,
    /// `## heading`.
    Heading,
    /// `- item`.
    Bullet,
    /// `1. item`.
    Numbered,
    /// `- [ ] task`.
    Task,
    /// `[text](url)`.
    Link,
}
