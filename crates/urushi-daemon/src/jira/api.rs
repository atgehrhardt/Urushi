//! What the daemon asks of Jira, as a trait: [`super::CloudJira`] asks Jira Cloud over
//! HTTPS, and tests answer from memory. Every call is blocking (run it off the async
//! runtime) and takes the account to sign in with.

use std::time::Duration;

use urushi_core::TaskKind;
use urushi_core::board::{JiraColumn, StatusCategory, SyncedIssue};

use super::Account;

/// Why a Jira call failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum JiraError {
    /// Jira didn't accept the email and API token.
    #[error("Jira didn't accept the email and API token")]
    Unauthorized,
    /// The account may not do this.
    #[error("Jira refused: {0}")]
    Forbidden(String),
    /// The board or issue doesn't exist (or the account can't see it).
    #[error("Jira can't find {0}")]
    NotFound(String),
    /// Jira refused the change as invalid (e.g. a label with a space).
    #[error("Jira refused the change: {0}")]
    Rejected(String),
    /// Jira asks to wait before asking again.
    #[error("Jira is busy; trying again later")]
    RateLimited {
        /// How long it asked to wait, if it said.
        retry_after: Option<Duration>,
    },
    /// Jira couldn't be reached, or failed.
    #[error("Jira couldn't be reached: {0}")]
    Unavailable(String),
}

/// Result alias for Jira calls.
pub type JiraResult<T> = Result<T, JiraError>;

/// A board on the site, as listed for importing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardInfo {
    /// Its id.
    pub id: u64,
    /// Its name.
    pub name: String,
    /// Scrum (sprints) rather than kanban.
    pub scrum: bool,
    /// Its location's project key; empty for a board not in a project.
    pub project_key: String,
    /// Its location's name.
    pub project_name: String,
}

/// A board's configuration: what to show and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardConfig {
    /// The board itself.
    pub info: BoardInfo,
    /// Its columns, left to right.
    pub columns: Vec<JiraColumn>,
    /// The field holding its ranking (e.g. `customfield_10019`).
    pub rank_field: Option<String>,
    /// The saved filter choosing its issues.
    pub filter_id: u64,
    /// A kanban board's extra query narrowing the filter (e.g. unreleased versions).
    pub sub_query: Option<String>,
}

/// Which of a board's issues to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueQuery {
    /// The board's filter.
    pub filter_id: u64,
    /// The board's extra query, if any.
    pub sub_query: Option<String>,
    /// Only issues in open sprints (a scrum board).
    pub scrum: bool,
    /// The board's rank field, to order and place cards.
    pub rank_field: Option<String>,
    /// Only issues updated in the last this many minutes; everything when `None`.
    pub updated_within_minutes: Option<u64>,
    /// Read at most this many.
    pub limit: usize,
}

/// Issues read for a board.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fetched {
    /// The issues, in rank order.
    pub issues: Vec<SyncedIssue>,
    /// Whether there were more than the query's limit.
    pub more: bool,
}

/// A transition an issue can take now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    /// Its id.
    pub id: String,
    /// Its name ("Start progress").
    pub name: String,
    /// The status it leads to.
    pub to_status: String,
    /// That status's category.
    pub to_category: StatusCategory,
}

/// Changes to an issue's fields. Empty parts are left alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueEdit {
    /// A new summary.
    pub summary: Option<String>,
    /// A new description, as plain text.
    pub notes: Option<String>,
    /// Labels to add.
    pub labels_add: Vec<String>,
    /// Labels to remove.
    pub labels_remove: Vec<String>,
}

impl IssueEdit {
    /// Whether it changes nothing.
    pub fn is_empty(&self) -> bool {
        self.summary.is_none()
            && self.notes.is_none()
            && self.labels_add.is_empty()
            && self.labels_remove.is_empty()
    }
}

/// An issue to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewIssue {
    /// The project's key.
    pub project_key: String,
    /// Its kind: picks the project's issue type.
    pub kind: TaskKind,
    /// The parent's key, for a subtask.
    pub parent_key: Option<String>,
    /// Its summary.
    pub summary: String,
    /// Its description, as plain text.
    pub notes: String,
    /// Its labels.
    pub labels: Vec<String>,
}

/// Where to rank an issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rank {
    /// Just above this issue.
    Before(String),
    /// Just below this issue.
    After(String),
}

/// Jira, as the daemon uses it.
pub trait JiraApi: Send + Sync {
    /// The account's display name; checks the sign-in.
    fn myself(&self, account: &Account) -> JiraResult<String>;
    /// Boards whose name contains `query`, at most 50.
    fn boards(&self, account: &Account, query: &str) -> JiraResult<Vec<BoardInfo>>;
    /// A board's configuration.
    fn board(&self, account: &Account, id: u64) -> JiraResult<BoardConfig>;
    /// A board's issues, in rank order.
    fn issues(&self, account: &Account, query: &IssueQuery) -> JiraResult<Fetched>;
    /// One issue, by key.
    fn issue(
        &self,
        account: &Account,
        key: &str,
        rank_field: Option<&str>,
    ) -> JiraResult<SyncedIssue>;
    /// The transitions an issue can take now.
    fn transitions(&self, account: &Account, key: &str) -> JiraResult<Vec<Transition>>;
    /// Takes a transition.
    fn transition(&self, account: &Account, key: &str, transition: &str) -> JiraResult<()>;
    /// Changes an issue's fields.
    fn edit(&self, account: &Account, key: &str, edit: &IssueEdit) -> JiraResult<()>;
    /// Creates an issue; returns its key.
    fn create(&self, account: &Account, issue: &NewIssue) -> JiraResult<String>;
    /// Moves an issue in the board's ranking.
    fn rank(&self, account: &Account, key: &str, rank: &Rank) -> JiraResult<()>;
}
