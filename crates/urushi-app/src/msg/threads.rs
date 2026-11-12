//! Messages about a thread's unread mark and notes, and a project's color (UR-62,
//! ADR 0048). Each becomes a request to the daemon (`update/thread_extras.rs`).

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
    /// Open the notes editor for a thread.
    OpenNotes(String),
    /// Color a project's sidebar section with a wire `LabelColor` (0: none).
    SetProjectColor {
        /// Project id.
        project: String,
        /// Wire color.
        color: i32,
    },
}
