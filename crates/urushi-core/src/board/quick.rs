//! Quick filters a board keeps: named filters shown as chips above the board, each
//! turned on and off with a click, as Jira's quick filters are (ADR 0037).
//!
//! A board of Urushi's own filters by fields every task has ([`QuickRule::Fields`]: a
//! search, kinds and tags), which clients evaluate. A Jira board's quick filters live in
//! Jira ([`super::JiraBoard::quick_filters`]); only one Jira wouldn't take is kept
//! here, as JQL ([`QuickRule::Jql`]) that Jira still evaluates. Which rule suits which
//! board is checked here, so a board never holds a filter it can't evaluate.

use serde::{Deserialize, Serialize};

use super::{Board, BoardError, MAX_NAME, TaskKind, task_tags, within};
use crate::QuickFilterId;

/// Most quick filters a board keeps.
pub const MAX_QUICK_FILTERS: usize = 20;
/// Longest JQL a quick filter holds, in characters.
pub const MAX_JQL: usize = 2000;
/// Longest search text a quick filter holds, in characters.
pub const MAX_SEARCH: usize = 200;

/// A named filter a board keeps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickFilter {
    /// Identity.
    pub id: QuickFilterId,
    /// Its name, shown on its chip.
    pub name: String,
    /// What it lets through.
    pub rule: QuickRule,
}

/// One quick filter of a board: one of a Jira board's own, by Jira's id, or one this
/// board keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QuickFilterRef {
    /// A Jira board's own quick filter ([`super::JiraQuickFilter::id`]).
    Jira(u64),
    /// One the board keeps ([`QuickFilter::id`]).
    Kept(QuickFilterId),
}

/// What a quick filter lets through.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum QuickRule {
    /// On a board of Urushi's own: tasks that pass every criterion given (the search
    /// in the title or notes, ignoring case; one of the kinds; one of the tags).
    Fields {
        /// Text the title or notes must contain; empty for any.
        #[serde(default)]
        search: String,
        /// Kinds, any of which lets a task through; empty for any.
        #[serde(default)]
        kinds: Vec<TaskKind>,
        /// Tags, any of which lets a task through (ignoring case); empty for any.
        #[serde(default)]
        tags: Vec<String>,
    },
    /// On a Jira board: a JQL clause, which Jira evaluates.
    Jql {
        /// The clause.
        jql: String,
    },
}

impl QuickRule {
    /// The rule cleaned up (text trimmed, repeats dropped, tags as [`task_tags`] keeps
    /// them), or why it can't be kept: it filters by nothing, or holds too much.
    pub fn checked(self) -> Result<Self, BoardError> {
        match self {
            Self::Fields {
                search,
                mut kinds,
                tags,
            } => {
                let search = within(
                    search.trim().to_owned(),
                    "a quick filter's search",
                    MAX_SEARCH,
                )?;
                let mut seen = Vec::new();
                kinds.retain(|k| {
                    let new = !seen.contains(k);
                    seen.push(*k);
                    new
                });
                let tags = task_tags(&tags)?;
                if search.is_empty() && kinds.is_empty() && tags.is_empty() {
                    return Err(BoardError::EmptyQuickFilter);
                }
                Ok(Self::Fields {
                    search,
                    kinds,
                    tags,
                })
            }
            Self::Jql { jql } => {
                let jql = within(jql.trim().to_owned(), "a quick filter's JQL", MAX_JQL)?;
                if jql.is_empty() {
                    return Err(BoardError::EmptyQuickFilter);
                }
                Ok(Self::Jql { jql })
            }
        }
    }

    /// Its JQL, for a rule Jira evaluates.
    pub fn jql(&self) -> Option<&str> {
        match self {
            Self::Jql { jql } => Some(jql),
            Self::Fields { .. } => None,
        }
    }
}

/// A quick filter's name, with runs of whitespace collapsed.
pub fn quick_filter_name(name: &str) -> Result<String, BoardError> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(BoardError::EmptyFilterName);
    }
    within(name, "a quick filter's name", MAX_NAME)
}

impl Board {
    /// The quick filter Urushi keeps with this id.
    pub fn quick_filter(&self, id: QuickFilterId) -> Option<&QuickFilter> {
        self.quick_filters.iter().find(|q| q.id == id)
    }

    /// Adds a quick filter after the others.
    pub fn add_quick_filter(
        &mut self,
        id: QuickFilterId,
        name: &str,
        rule: QuickRule,
    ) -> Result<(), BoardError> {
        if self.quick_filters.len() >= MAX_QUICK_FILTERS {
            return Err(BoardError::TooManyQuickFilters);
        }
        let name = quick_filter_name(name)?;
        let rule = self.suited(rule)?;
        self.quick_filters.push(QuickFilter { id, name, rule });
        Ok(())
    }

    /// Renames a quick filter and/or replaces its rule; `None` keeps either. Nothing
    /// changes unless every given value is valid.
    pub fn update_quick_filter(
        &mut self,
        id: QuickFilterId,
        name: Option<&str>,
        rule: Option<QuickRule>,
    ) -> Result<(), BoardError> {
        let name = name.map(quick_filter_name).transpose()?;
        let rule = rule.map(|r| self.suited(r)).transpose()?;
        let filter = self
            .quick_filters
            .iter_mut()
            .find(|q| q.id == id)
            .ok_or(BoardError::NoSuchQuickFilter(id))?;
        if let Some(name) = name {
            filter.name = name;
        }
        if let Some(rule) = rule {
            filter.rule = rule;
        }
        Ok(())
    }

    /// Removes a quick filter, returning it.
    pub fn remove_quick_filter(&mut self, id: QuickFilterId) -> Result<QuickFilter, BoardError> {
        let at = self
            .quick_filters
            .iter()
            .position(|q| q.id == id)
            .ok_or(BoardError::NoSuchQuickFilter(id))?;
        Ok(self.quick_filters.remove(at))
    }

    /// `rule`, checked, if it suits this board: JQL on a Jira board, fields on others.
    fn suited(&self, rule: QuickRule) -> Result<QuickRule, BoardError> {
        match (&rule, self.source.is_some()) {
            (QuickRule::Fields { .. }, true) => Err(BoardError::WrongQuickRule(
                "a Jira board's quick filters are JQL",
            )),
            (QuickRule::Jql { .. }, false) => Err(BoardError::WrongQuickRule(
                "JQL needs a Jira board: filter by search, type and tag here",
            )),
            _ => rule.checked(),
        }
    }
}

#[cfg(test)]
mod tests;
