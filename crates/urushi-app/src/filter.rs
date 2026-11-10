//! Filtering a board the way Jira filters one: a text search, people (with "Only my
//! issues"), and any of the issue's fields (type, priority, status, labels, epic,
//! sprint, components, fix versions), "Recently updated", and quick filters: a Jira
//! board's own, which only Jira can evaluate (`QueryJiraFilter`, protocol 1.13), and
//! those the board keeps (protocol 1.23, `quick.rs`), which are JQL on a Jira board and
//! a search, kinds and tags elsewhere.
//!
//! Values chosen in one dimension widen it (any of them); dimensions narrow each other
//! (all of them), as in Jira. A card shows when it or any of its subtasks passes, so a
//! subtask assigned to you keeps its card on the board. Everything but JQL is decided
//! here, from the fields every task carries; which filters are on is the window's own
//! and isn't kept by the daemon. The selection can be saved as a quick filter: as JQL
//! (`jql.rs`) on a Jira board, as a search, kinds and tags elsewhere.

use std::collections::{BTreeSet, HashSet};

use urushi_client::{TaskRef, v1};
use urushi_ui::TextField;

pub mod editor;
pub mod jql;
pub mod quick;

/// Updated within this long counts as "Recently updated" (as Jira's quick filter).
pub const RECENT_MS: i64 = 24 * 60 * 60 * 1000;

/// Something a board can be filtered by, with values to choose from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Dim {
    /// Who it is assigned to (an account id, or [`UNASSIGNED`]).
    Assignee,
    /// Who reported it (an account id).
    Reporter,
    /// Its type's name ("Story"); on a board of Urushi's own, its kind.
    Type,
    /// Its priority's name.
    Priority,
    /// Its status's name.
    Status,
    /// Its labels (tags), ignoring case.
    Label,
    /// Its epic's key.
    Epic,
    /// Its sprint's id.
    Sprint,
    /// Its components' ids.
    Component,
    /// Its fix versions' ids.
    Version,
}

impl Dim {
    /// Every dimension, in menu order.
    pub const ALL: [Self; 10] = [
        Self::Assignee,
        Self::Type,
        Self::Priority,
        Self::Status,
        Self::Label,
        Self::Epic,
        Self::Sprint,
        Self::Component,
        Self::Version,
        Self::Reporter,
    ];

    /// Its name in the menu.
    pub fn name(self) -> &'static str {
        match self {
            Self::Assignee => "Assignee",
            Self::Reporter => "Reporter",
            Self::Type => "Type",
            Self::Priority => "Priority",
            Self::Status => "Status",
            Self::Label => "Label",
            Self::Epic => "Epic",
            Self::Sprint => "Sprint",
            Self::Component => "Component",
            Self::Version => "Fix version",
        }
    }

    /// Whether it applies to a board of Urushi's own (no Jira fields).
    pub fn local(self) -> bool {
        matches!(self, Self::Type | Self::Label)
    }
}

/// The value [`Dim::Assignee`] uses for unassigned issues.
pub const UNASSIGNED: &str = "";

/// A board's filter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardFilter {
    /// Text the title, key or notes must contain (ignoring case).
    pub search: TextField,
    /// The values chosen in each dimension (empty: no filter on it).
    pub chosen: std::collections::BTreeMap<Dim, BTreeSet<String>>,
    /// Only issues assigned to the signed-in account.
    pub mine: bool,
    /// Only issues updated in the last [`RECENT_MS`].
    pub recent: bool,
    /// The Jira board's own quick filters on (by id), all of which must let an issue
    /// through.
    pub quick: BTreeSet<u64>,
    /// The quick filters the board keeps that are on (by id), all of which must let a
    /// task through.
    pub saved: BTreeSet<String>,
    /// The issues the quick filters in JQL let through, as Jira last said (by issue id);
    /// `None` until it answers, which shows the board unfiltered by them.
    pub quick_matches: Option<HashSet<String>>,
}

/// What a filter needs from outside the board: who "me" is, and the time.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context<'a> {
    /// The signed-in account's id, else its display name.
    pub me: Option<&'a str>,
    /// Now, in Unix milliseconds.
    pub now_ms: i64,
    /// The quick filters the board keeps.
    pub kept: &'a [v1::QuickFilter],
}

impl BoardFilter {
    /// Whether anything filters the board.
    pub fn active(&self) -> bool {
        !self.search.text().trim().is_empty()
            || self.chosen.values().any(|v| !v.is_empty())
            || self.mine
            || self.recent
            || !self.quick.is_empty()
            || !self.saved.is_empty()
    }

    /// Whether anything but quick filters is chosen: what saving the selection as a
    /// quick filter keeps.
    pub fn selection_active(&self) -> bool {
        !self.search.text().trim().is_empty()
            || self.chosen.values().any(|v| !v.is_empty())
            || self.mine
            || self.recent
    }

    /// Whether some filter on is JQL, which Jira evaluates.
    pub fn needs_jira(&self, kept: &[v1::QuickFilter]) -> bool {
        !self.quick.is_empty() || !self.kept_jql(kept).is_empty()
    }

    /// The kept quick filters on that are JQL, by id.
    pub fn kept_jql(&self, kept: &[v1::QuickFilter]) -> Vec<String> {
        kept.iter()
            .filter(|q| self.saved.contains(&q.id))
            .filter(|q| q.rule.as_ref().is_some_and(|r| !r.jql.is_empty()))
            .map(|q| q.id.clone())
            .collect()
    }

    /// How many filters are on (shown on the Filters button), the search aside.
    pub fn count(&self) -> usize {
        self.chosen.values().map(BTreeSet::len).sum::<usize>()
            + usize::from(self.mine)
            + usize::from(self.recent)
            + self.quick.len()
            + self.saved.len()
    }

    /// Whether `value` is chosen in `dim`.
    pub fn has(&self, dim: Dim, value: &str) -> bool {
        self.chosen.get(&dim).is_some_and(|v| v.contains(value))
    }

    /// Chooses `value` in `dim`, or unchooses it.
    pub fn toggle(&mut self, dim: Dim, value: &str) {
        let values = self.chosen.entry(dim).or_default();
        if !values.remove(value) {
            values.insert(value.to_owned());
        }
    }

    /// Clears everything, the search too.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Whether the card shows: it or one of its subtasks passes.
    pub fn shows(&self, card: &v1::Task, cx: Context<'_>) -> bool {
        fn any(f: &BoardFilter, t: TaskRef<'_>, cx: Context<'_>) -> bool {
            f.passes(t, cx) || t.subtasks().iter().any(|s| any(f, TaskRef::Sub(s), cx))
        }
        !self.active() || any(self, TaskRef::Card(card), cx)
    }

    /// Whether the task itself passes every filter.
    pub fn passes(&self, task: TaskRef<'_>, cx: Context<'_>) -> bool {
        let issue = task.issue();
        let search = self.search.text().trim().to_lowercase();
        if !search.is_empty() {
            let found = task.title().to_lowercase().contains(&search)
                || task.notes().to_lowercase().contains(&search)
                || issue.is_some_and(|i| i.key.to_lowercase().contains(&search));
            if !found {
                return false;
            }
        }
        if self.mine && !(cx.me.is_some() && assignee(issue).as_deref() == cx.me) {
            return false;
        }
        if self.recent && issue.is_none_or(|i| cx.now_ms - i.updated_ms > RECENT_MS) {
            return false;
        }
        if self.needs_jira(cx.kept)
            && let Some(matches) = &self.quick_matches
            && !issue.is_some_and(|i| matches.contains(&i.id))
        {
            return false;
        }
        let kept_rules = cx
            .kept
            .iter()
            .filter(|q| self.saved.contains(&q.id))
            .filter_map(|q| q.rule.as_ref());
        if !kept_rules.into_iter().all(|r| quick::passes(r, task)) {
            return false;
        }
        self.chosen
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .all(|(dim, chosen)| {
                values(*dim, task)
                    .iter()
                    .any(|(value, _)| chosen.contains(value))
            })
    }
}

/// Who an issue is assigned to, as the filter names people: the account id, else the
/// display name (from a daemon before 1.13), else [`UNASSIGNED`].
fn assignee(issue: Option<&v1::JiraIssue>) -> Option<String> {
    let i = issue?;
    Some(if i.assignee_id.is_empty() {
        i.assignee.clone()
    } else {
        i.assignee_id.clone()
    })
}

/// A task's values in `dim`, each as `(value, label)`.
pub fn values(dim: Dim, task: TaskRef<'_>) -> Vec<(String, String)> {
    let issue = task.issue();
    let pair = |id: &str, name: &str| (id.to_owned(), name.to_owned());
    let named = |n: &[v1::JiraNamed]| n.iter().map(|n| pair(&n.id, &n.name)).collect();
    match dim {
        Dim::Label => task
            .tags()
            .iter()
            .map(|t| (t.to_lowercase(), t.clone()))
            .collect(),
        Dim::Type => match issue {
            Some(i) if !i.type_name.is_empty() => vec![pair(&i.type_name, &i.type_name)],
            _ => kind_name(task.kind())
                .map(|k| vec![pair(k, k)])
                .unwrap_or_default(),
        },
        Dim::Assignee => issue
            .map(|i| match assignee(Some(i)) {
                Some(id) if !id.is_empty() => vec![(id, i.assignee.clone())],
                _ => vec![pair(UNASSIGNED, "Unassigned")],
            })
            .unwrap_or_default(),
        Dim::Reporter => issue
            .and_then(|i| i.reporter.as_ref())
            .map(|r| vec![pair(&r.account_id, &r.name)])
            .unwrap_or_default(),
        Dim::Priority => issue
            .filter(|i| !i.priority.is_empty())
            .map(|i| vec![pair(&i.priority, &i.priority)])
            .unwrap_or_default(),
        Dim::Status => issue
            .filter(|i| !i.status.is_empty())
            .map(|i| vec![pair(&i.status, &i.status)])
            .unwrap_or_default(),
        // An epic card is in its own epic, so it shows with its stories.
        Dim::Epic => issue
            .map(|i| {
                if task.kind() == i32::from(v1::TaskKind::Epic) {
                    &i.key
                } else {
                    &i.epic_key
                }
            })
            .filter(|k| !k.is_empty())
            .map(|k| vec![pair(k, k)])
            .unwrap_or_default(),
        Dim::Sprint => issue
            .and_then(|i| i.sprint.as_ref())
            .map(|s| vec![pair(&s.id, &s.name)])
            .unwrap_or_default(),
        Dim::Component => issue.map(|i| named(&i.components)).unwrap_or_default(),
        Dim::Version => issue.map(|i| named(&i.fix_versions)).unwrap_or_default(),
    }
}

/// A kind's name ("Story"), or `None` without one.
pub fn kind_name(kind: i32) -> Option<&'static str> {
    Some(match v1::TaskKind::try_from(kind).ok()? {
        v1::TaskKind::Unspecified => return None,
        v1::TaskKind::Task => "Task",
        v1::TaskKind::Story => "Story",
        v1::TaskKind::Bug => "Bug",
        v1::TaskKind::Epic => "Epic",
        v1::TaskKind::Subtask => "Subtask",
    })
}

/// The values a board's tasks (at any depth) have in `dim`, each once, in first-seen
/// order, as `(value, label)`. Epics are labeled with their summaries.
pub fn board_values(board: &v1::Board, dim: Dim) -> Vec<(String, String)> {
    fn walk(t: TaskRef<'_>, dim: Dim, out: &mut Vec<(String, String)>) {
        for v in values(dim, t) {
            if !out.iter().any(|(o, _)| *o == v.0) {
                out.push(v);
            }
        }
        for s in t.subtasks() {
            walk(TaskRef::Sub(s), dim, out);
        }
    }
    let mut out = Vec::new();
    for card in board.columns.iter().flat_map(|c| &c.tasks) {
        walk(TaskRef::Card(card), dim, &mut out);
    }
    if dim == Dim::Epic {
        for (key, label) in &mut out {
            if let Some(e) = board.epics.iter().find(|e| e.key == *key) {
                label.clone_from(&e.summary);
            }
        }
    }
    out
}

/// The dimensions worth offering on `board`: those some task has a value in, and on a
/// board of Urushi's own only those that apply.
pub fn board_dims(board: &v1::Board) -> Vec<Dim> {
    Dim::ALL
        .into_iter()
        .filter(|d| board.jira.is_some() || d.local())
        .filter(|d| !board_values(board, *d).is_empty())
        .collect()
}

#[cfg(test)]
mod tests;
