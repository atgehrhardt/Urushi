//! Messages for Jira (protocol 1.12): signing the daemon in, importing a board to
//! follow, syncing it now, and opening Jira's pages.

/// Something done about Jira.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JiraMsg {
    /// Open the dialog that signs the daemon in to Jira.
    OpenConnect,
    /// Forget the daemon's Jira sign-in.
    Disconnect,
    /// Open the dialog that imports a Jira board (signing in first if needed).
    OpenImport,
    /// Import this Jira board (its id in Jira).
    Import(u64),
    /// Sync a Jira board now (by board id).
    Sync(String),
    /// Open a page of Jira (an issue, a board) in the browser.
    Open(String),
}
