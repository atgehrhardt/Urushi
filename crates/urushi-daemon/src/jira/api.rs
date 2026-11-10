//! What the daemon asks of Jira, as a trait: [`super::CloudJira`] asks Jira Cloud over
//! HTTPS, and tests answer from memory. Every call is blocking (run it off the async
//! runtime) and takes the account to sign in with.

use std::time::Duration;

use urushi_core::TaskKind;
use urushi_core::board::{JiraBoard, JiraColumn, Person, QuickFilter, StatusCategory, SyncedIssue};

use super::details::{Field, IssueDetails, IssueRef};

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
    /// The field holding its estimates (story points), if it estimates.
    pub points_field: Option<String>,
    /// The site's sprint field, if it has sprints.
    pub sprint_field: Option<String>,
    /// Its quick filters.
    pub quick_filters: Vec<QuickFilter>,
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
    /// The board's own fields to read.
    pub fields: IssueFields,
    /// Only issues updated in the last this many minutes; everything when `None`.
    pub updated_within_minutes: Option<u64>,
    /// More JQL clauses the issues must match (e.g. quick filters).
    pub extra_jql: Vec<String>,
    /// Read at most this many.
    pub limit: usize,
}

/// Fields whose ids depend on the site or board, read with every issue.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueFields {
    /// The rank field, to order and place cards.
    pub rank: Option<String>,
    /// The estimate (story points) field.
    pub points: Option<String>,
    /// The sprint field.
    pub sprint: Option<String>,
}

impl IssueFields {
    /// The fields of a board that follows Jira.
    pub fn of(jira: &JiraBoard) -> Self {
        Self {
            rank: jira.rank_field.clone(),
            points: jira.points_field.clone(),
            sprint: jira.sprint_field.clone(),
        }
    }

    /// The fields of a board's configuration.
    pub fn of_config(config: &BoardConfig) -> Self {
        Self {
            rank: config.rank_field.clone(),
            points: config.points_field.clone(),
            sprint: config.sprint_field.clone(),
        }
    }

    /// Those known, by id.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        [&self.rank, &self.points, &self.sprint]
            .into_iter()
            .filter_map(|f| f.as_deref())
    }
}

/// Who the daemon is signed in as.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Me {
    /// Their display name.
    pub display_name: String,
    /// Their Atlassian account id.
    pub account_id: String,
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
    /// Other fields, as Jira's edit API takes them (`null` clears one).
    pub fields: Vec<(String, serde_json::Value)>,
}

impl IssueEdit {
    /// Whether it changes nothing.
    pub fn is_empty(&self) -> bool {
        self.summary.is_none()
            && self.notes.is_none()
            && self.labels_add.is_empty()
            && self.labels_remove.is_empty()
            && self.fields.is_empty()
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
    /// Other fields, as Jira's create API takes them.
    pub fields: Vec<(String, serde_json::Value)>,
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
    /// Who the account is; checks the sign-in.
    fn myself(&self, account: &Account) -> JiraResult<Me>;
    /// Boards whose name contains `query`, at most 50.
    fn boards(&self, account: &Account, query: &str) -> JiraResult<Vec<BoardInfo>>;
    /// A board's configuration.
    fn board(&self, account: &Account, id: u64) -> JiraResult<BoardConfig>;
    /// A board's issues, in rank order.
    fn issues(&self, account: &Account, query: &IssueQuery) -> JiraResult<Fetched>;
    /// Which of a board's issues the query matches (at most its limit), each with when
    /// it last changed: the cheap first look of a sync.
    fn changed(
        &self,
        account: &Account,
        query: &IssueQuery,
    ) -> JiraResult<Vec<(String, urushi_core::Timestamp)>>;
    /// The ids of a board's issues the query matches (at most its limit), without
    /// their fields.
    fn issue_ids(&self, account: &Account, query: &IssueQuery) -> JiraResult<Vec<String>>;
    /// One issue, by key.
    fn issue(&self, account: &Account, key: &str, fields: &IssueFields) -> JiraResult<SyncedIssue>;
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
    /// Deletes an issue for good, with its sub-tasks (Jira refuses to delete an issue
    /// that has sub-tasks otherwise).
    fn delete_issue(&self, account: &Account, key: &str) -> JiraResult<()>;

    // ---- One issue in full (the editor) -------------------------------------------
    /// Everything the editor shows of an issue: its editable fields with their values
    /// (sprints offered from `board`), transitions, comments, attachments, links,
    /// watchers and work log.
    fn details(&self, account: &Account, key: &str, board: u64) -> JiraResult<IssueDetails>;
    /// The fields a new issue of `kind` in `project` has (with what each allows).
    fn create_fields(
        &self,
        account: &Account,
        project: &str,
        kind: TaskKind,
    ) -> JiraResult<Vec<Field>>;
    /// People who may be assigned `key` (or anyone, without a key) whose name matches.
    fn users(&self, account: &Account, query: &str, key: Option<&str>) -> JiraResult<Vec<Person>>;
    /// Issues of `project` whose key or text matches `query`, at most `limit`.
    fn search(
        &self,
        account: &Account,
        project: &str,
        query: &str,
        limit: usize,
    ) -> JiraResult<Vec<IssueRef>>;
    /// Adds a comment (Markdown).
    fn add_comment(&self, account: &Account, key: &str, body: &str) -> JiraResult<()>;
    /// Changes a comment.
    fn update_comment(&self, account: &Account, key: &str, id: &str, body: &str) -> JiraResult<()>;
    /// Deletes a comment.
    fn delete_comment(&self, account: &Account, key: &str, id: &str) -> JiraResult<()>;
    /// Attaches a file.
    fn attach(&self, account: &Account, key: &str, name: &str, data: &[u8]) -> JiraResult<()>;
    /// Deletes an attachment.
    fn delete_attachment(&self, account: &Account, id: &str) -> JiraResult<()>;
    /// An attachment's contents, from its content address.
    fn download(&self, account: &Account, url: &str) -> JiraResult<Vec<u8>>;
    /// Links `outward` to `inward` with a kind of link ("`outward` blocks `inward`").
    fn link(&self, account: &Account, kind: &str, outward: &str, inward: &str) -> JiraResult<()>;
    /// Removes a link.
    fn unlink(&self, account: &Account, id: &str) -> JiraResult<()>;
    /// Starts or stops the account watching an issue.
    fn watch(&self, account: &Account, key: &str, watch: bool) -> JiraResult<()>;
    /// Logs work (`started` in Jira's `2026-11-09T10:00:00.000+0000` form).
    fn log_work(
        &self,
        account: &Account,
        key: &str,
        seconds: u64,
        started: &str,
        comment: &str,
    ) -> JiraResult<()>;
}
