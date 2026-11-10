//! Messages for the task editor's Jira side (`issue.rs`, protocol 1.13).

use urushi_client::v1;

use crate::issue::IssueTab;

/// Something done in the editor of a Jira issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueMsg {
    /// Open a field's picker (or [`crate::issue::STATUS`], [`crate::issue::LINK`]), or
    /// close it with `None`.
    Open(Option<String>),
    /// Choose a field's one value (`None` clears it). A draft, sent with Save.
    Pick {
        /// The field.
        field: String,
        /// The value.
        option: Option<v1::JiraOption>,
    },
    /// Add a value to a field of several, or take it out. A draft.
    Toggle {
        /// The field.
        field: String,
        /// The value.
        option: v1::JiraOption,
    },
    /// Assign the issue to the signed-in account (a draft).
    AssignToMe,
    /// Look up what the open picker's search says (people, issues).
    Search,
    /// Take a transition: a status change, made now.
    Transition(String),
    /// Show a part of the issue under the notes.
    Tab(IssueTab),
    /// Post the new comment.
    Comment,
    /// Change a comment of one's own (by id): open it to edit.
    EditComment(String),
    /// Save the comment being changed.
    SaveComment,
    /// Stop changing a comment.
    CancelComment,
    /// Delete a comment of one's own.
    DeleteComment(String),
    /// Start (`true`) or stop watching.
    Watch(bool),
    /// Open an attachment (its id and file name) with the system's app.
    OpenAttachment(String, String),
    /// Delete an attachment.
    DeleteAttachment(String),
    /// The kind of link to add (index into the link types), and the side this issue
    /// is on (`true`: outward, "this blocks").
    LinkKind(usize, bool),
    /// Link to this issue (by key).
    Link(String),
    /// Remove a link (by id).
    Unlink(String),
    /// Log the time typed.
    LogWork,
}
