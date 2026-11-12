//! Boards that follow a Jira board (ADR 0020).
//!
//! A Jira board in Urushi is a mirror: Jira is the source of truth, the daemon polls it
//! and merges what it reads ([`super::sync`]), and edits are written to Jira before
//! they are recorded here. The board remembers where it comes from
//! ([`BoardSource::Jira`]), each column the Jira statuses it shows
//! ([`super::Column::statuses`]), and each task the issue it shows ([`Issue`]). Epics
//! aren't cards: the board keeps their names and colors ([`Epic`]), and their issues
//! name them.
//!
//! A task's thread link and color are Urushi's own and survive every sync. Changes
//! Jira owns (columns, the board's name, nesting, deleting issues, attachments) are
//! refused on these boards with [`super::BoardError::FollowsJira`].

use serde::{Deserialize, Serialize};

use super::{Board, BoardError, LabelColor, PullRequest, TaskKind};
use crate::Timestamp;

/// Where a board's content comes from, if not from Urushi alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BoardSource {
    /// A Jira Cloud board.
    Jira(JiraBoard),
}

/// The Jira board a board follows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JiraBoard {
    /// The site's address, e.g. `https://acme.atlassian.net` (no trailing slash).
    pub site: String,
    /// The key of the Jira sign-in that reads and writes it (`acme.atlassian.net/ada@acme.com`,
    /// ADR 0049). Empty on a board imported before several sign-ins were kept: such a
    /// board uses the sign-in for its site.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account: String,
    /// The board's id in Jira.
    pub board_id: u64,
    /// The board's name in Jira.
    pub name: String,
    /// The project new issues are created in (the board's location).
    #[serde(default)]
    pub project_key: String,
    /// A scrum board shows its open sprints; a kanban board everything on it.
    #[serde(default)]
    pub scrum: bool,
    /// The field holding the board's ranking (e.g. `customfield_10019`), if known.
    #[serde(default)]
    pub rank_field: Option<String>,
    /// The field holding estimates (story points) on this board, if it estimates.
    #[serde(default)]
    pub points_field: Option<String>,
    /// The site's sprint field, if it has sprints.
    #[serde(default)]
    pub sprint_field: Option<String>,
    /// The board's quick filters, as Jira shows them above the board.
    #[serde(default)]
    pub quick_filters: Vec<JiraQuickFilter>,
    /// How the last sync went.
    #[serde(default)]
    pub sync: SyncState,
}

impl JiraBoard {
    /// The web page of an issue.
    pub fn issue_url(&self, key: &str) -> String {
        format!("{}/browse/{key}", self.site)
    }

    /// The board's web page. This old address redirects to the board whether its
    /// project is company- or team-managed.
    pub fn board_url(&self) -> String {
        format!(
            "{}/secure/RapidBoard.jspa?rapidView={}",
            self.site, self.board_id
        )
    }
}

/// A board's quick filter: a named JQL clause that narrows the board (Jira evaluates
/// it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JiraQuickFilter {
    /// Its id in Jira.
    pub id: u64,
    /// Its name ("Only my issues", "Recently updated").
    pub name: String,
    /// Its JQL.
    #[serde(default)]
    pub jql: String,
}

/// Something Jira names and identifies: a sprint, a component, a version.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    /// Its id in Jira.
    pub id: String,
    /// Its name.
    pub name: String,
}

/// A Jira user.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    /// Their Atlassian account id: what Jira changes are made with.
    pub account_id: String,
    /// Their display name.
    pub name: String,
}

/// The state of a board's sync with Jira.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    /// When the board last matched Jira; `None` before the first sync.
    #[serde(default)]
    pub synced_at: Option<Timestamp>,
    /// Why the last sync failed, if it did.
    #[serde(default)]
    pub error: Option<String>,
    /// Issues left out because the board was too big ([`super::MAX_SYNCED_ISSUES`],
    /// [`super::MAX_SUBTASKS`]).
    #[serde(default)]
    pub dropped: u32,
}

/// Which of Jira's three status groups a status is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusCategory {
    /// Not started (Jira's `new`).
    #[default]
    ToDo,
    /// Started (Jira's `indeterminate`).
    InProgress,
    /// Finished (Jira's `done`).
    Done,
}

impl StatusCategory {
    /// The category with Jira's key for it (`new`, `indeterminate`, `done`).
    pub fn from_jira(key: &str) -> Self {
        match key {
            "done" => Self::Done,
            "indeterminate" => Self::InProgress,
            _ => Self::ToDo,
        }
    }
}

/// The Jira issue a task shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// The issue's id in Jira (stable across moves between projects).
    pub id: String,
    /// Its key, e.g. `PROJ-123`.
    pub key: String,
    /// Its type's name as the site calls it ("Story", "Bug", "Spike"…).
    #[serde(default)]
    pub type_name: String,
    /// Its type's id.
    #[serde(default)]
    pub type_id: String,
    /// Its status's id: what places it in a column.
    #[serde(default)]
    pub status_id: String,
    /// Its status's name.
    #[serde(default)]
    pub status: String,
    /// Its status's category.
    #[serde(default)]
    pub category: StatusCategory,
    /// Who it is assigned to, by display name.
    #[serde(default)]
    pub assignee: Option<String>,
    /// The assignee's account id.
    #[serde(default)]
    pub assignee_id: Option<String>,
    /// Who reported it.
    #[serde(default)]
    pub reporter: Option<Person>,
    /// Its priority's name.
    #[serde(default)]
    pub priority: Option<String>,
    /// Its priority's id.
    #[serde(default)]
    pub priority_id: Option<String>,
    /// When it is due (`YYYY-MM-DD`).
    #[serde(default)]
    pub due: Option<String>,
    /// Its estimate (story points) as Jira shows it ("3", "0.5").
    #[serde(default)]
    pub points: Option<String>,
    /// The sprint it is in (the open one, if several).
    #[serde(default)]
    pub sprint: Option<Named>,
    /// Its components.
    #[serde(default)]
    pub components: Vec<Named>,
    /// The versions it is to be fixed in.
    #[serde(default)]
    pub fix_versions: Vec<Named>,
    /// How many issues it is linked to.
    #[serde(default)]
    pub links: u32,
    /// The key of the epic it belongs to.
    #[serde(default)]
    pub epic: Option<String>,
    /// The Jira id of its parent issue, for a subtask.
    #[serde(default)]
    pub parent: Option<String>,
    /// Its place in the board's ranking (Jira's `LexoRank`: compares as text).
    #[serde(default)]
    pub rank: String,
    /// When it last changed in Jira.
    #[serde(default = "Timestamp::unknown")]
    pub updated: Timestamp,
    /// False when the description was too long to show whole: editing the notes
    /// would cut it short in Jira, so they are read-only.
    #[serde(default = "yes")]
    pub notes_editable: bool,
    /// Its pull requests, from Jira's development info (ADR 0036): read only while a
    /// thread is linked to it, at most [`super::MAX_PULL_REQUESTS`]. A board sync
    /// keeps them as they are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pull_requests: Vec<PullRequest>,
}

fn yes() -> bool {
    true
}

/// An epic that issues on the board belong to. Not a card: issues name it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Epic {
    /// Its key, e.g. `PROJ-7`.
    pub key: String,
    /// Its summary.
    pub summary: String,
    /// The color its chips are drawn in, picked from its key.
    pub color: LabelColor,
}

/// One column of a Jira board's configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JiraColumn {
    /// Its name.
    pub name: String,
    /// The ids of the statuses it shows.
    pub statuses: Vec<String>,
}

/// An issue as read from Jira, ready to merge into a board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedIssue {
    /// The issue's own fields.
    pub issue: Issue,
    /// Its summary.
    pub title: String,
    /// Its description as plain text.
    pub notes: String,
    /// Its labels.
    pub labels: Vec<String>,
    /// Its kind.
    pub kind: TaskKind,
    /// The summary of its epic ([`Issue::epic`]), when Jira sent it.
    pub epic_summary: Option<String>,
    /// When it was created.
    pub created: Timestamp,
}

impl Board {
    /// The Jira board this board follows, if it follows one.
    pub fn jira(&self) -> Option<&JiraBoard> {
        match &self.source {
            Some(BoardSource::Jira(jira)) => Some(jira),
            None => None,
        }
    }

    /// The Jira board this board follows, to change its sync state.
    pub fn jira_mut(&mut self) -> Option<&mut JiraBoard> {
        match &mut self.source {
            Some(BoardSource::Jira(jira)) => Some(jira),
            None => None,
        }
    }

    /// Refuses `what` on a board that follows Jira.
    pub(crate) fn local_only(&self, what: &'static str) -> Result<(), BoardError> {
        if self.source.is_some() {
            Err(BoardError::FollowsJira(what))
        } else {
            Ok(())
        }
    }

    /// The epic with this key.
    pub fn epic(&self, key: &str) -> Option<&Epic> {
        self.epics.iter().find(|e| e.key == key)
    }

    /// Shows a card of a Jira board in `column` at `index` before Jira has moved it
    /// (ADR 0023), so the move shows at once. Returns where it was, to put it back if
    /// Jira refuses ([`Board::undo_jira_move`]). Only cards of a board that follows
    /// Jira; the next merge places them where Jira says.
    pub fn show_jira_move(
        &mut self,
        id: super::TaskId,
        column: super::ColumnId,
        index: usize,
    ) -> Result<(super::ColumnId, usize), BoardError> {
        if self.source.is_none() {
            return Err(BoardError::FollowsJira("showing a Jira move"));
        }
        let from = self
            .columns
            .iter()
            .find_map(|c| Some((c.id, c.tasks.iter().position(|t| t.id == id)?)))
            .ok_or(BoardError::NoSuchTask(id))?;
        self.column_mut(column)?;
        let task = self.column_mut(from.0)?.tasks.remove(from.1);
        let tasks = &mut self.column_mut(column)?.tasks;
        tasks.insert(index.min(tasks.len()), task);
        Ok(from)
    }

    /// Puts a card back where [`Board::show_jira_move`] took it from.
    pub fn undo_jira_move(
        &mut self,
        id: super::TaskId,
        (column, index): (super::ColumnId, usize),
    ) -> Result<(), BoardError> {
        let now = self
            .columns
            .iter()
            .find_map(|c| Some((c.id, c.tasks.iter().position(|t| t.id == id)?)))
            .ok_or(BoardError::NoSuchTask(id))?;
        self.column_mut(column)?;
        let task = self.column_mut(now.0)?.tasks.remove(now.1);
        let tasks = &mut self.column_mut(column)?.tasks;
        tasks.insert(index.min(tasks.len()), task);
        Ok(())
    }

    /// Takes a task (a card, or a subtask at any depth) and its subtasks off a board
    /// that follows Jira, once Jira has deleted its issue. Returns it. A board of
    /// Urushi's own refuses: its tasks are removed with [`Board::remove_task`].
    pub fn forget_issue(&mut self, id: super::TaskId) -> Result<super::Task, BoardError> {
        if self.source.is_none() {
            return Err(BoardError::FollowsJira("forgetting a deleted Jira issue"));
        }
        self.detach(id)
    }

    /// The task showing the issue with this Jira id or key.
    pub fn task_for_issue(&self, id_or_key: &str) -> Option<&super::Task> {
        self.all_tasks().find(|t| {
            t.issue
                .as_ref()
                .is_some_and(|i| i.id == id_or_key || i.key == id_or_key)
        })
    }
}
