//! Jira Cloud: the daemon's sign-in, and the calls that read and change Jira boards
//! (ADR 0020).
//!
//! - [`credentials`]: the sign-ins (site, email, API token), kept in a private file in
//!   the daemon's home and never sent to clients (ADR 0021).
//! - [`JiraApi`]: what the daemon asks of Jira, as a trait so tests answer from memory;
//!   [`CloudJira`] asks Jira Cloud over HTTPS through `curl` ([`crate::http`]).
//! - [`adf`]: Jira's rich text ⇄ plain notes; [`parse`]: Jira's JSON → board types.
//!
//! What to do with the answers (syncing boards, writing edits back) is the service's
//! job: `service/jira.rs` and `service/jira_write.rs`.

pub mod adf;
mod api;
mod cloud;
mod credentials;
pub mod details;
mod parse;

pub use api::{
    BoardConfig, BoardInfo, Fetched, IssueEdit, IssueFields, IssueQuery, JiraApi, JiraError,
    JiraResult, MAX_RECONCILE, Me, NewIssue, QuickFilterDraft, Rank, Transition,
};
pub use cloud::{CloudJira, DONE_DAYS, jql};
pub use credentials::{Account, CredentialStore, key_of, site_address};
