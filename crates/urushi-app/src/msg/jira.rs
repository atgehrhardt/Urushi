//! Messages for Jira (protocol 1.12): signing the daemon in (to several sites since
//! 1.29, ADR 0051), importing a board to follow, syncing it now, and opening Jira's
//! pages.

/// Something done about Jira.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JiraMsg {
    /// Open the dialog that signs the daemon in to a Jira site, empty.
    OpenConnect,
    /// Open it for the sign-in a board (by id) needs, its site (and email) filled in.
    Reconnect(String),
    /// Forget one of the daemon's Jira sign-ins (by `JiraAccount.key`).
    Disconnect(String),
    /// Open the dialog that imports a Jira board (signing in first if needed).
    OpenImport,
    /// In that dialog, list the boards of this sign-in's site (by key).
    ImportFrom(String),
    /// Import this Jira board (its id in Jira).
    Import(u64),
    /// Sync a Jira board now (by board id).
    Sync(String),
    /// Open a page of Jira (an issue, a board) in the browser.
    Open(String),
}
