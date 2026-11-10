//! Quick filters the board keeps (protocol 1.21, ADR 0033): which one a chip or menu
//! means, deciding whether a task passes one of a search, kinds and tags (a board of
//! Urushi's own; JQL is Jira's to evaluate), building that rule from what is chosen on
//! the board, and describing it in a few words.

use urushi_client::{TaskRef, jira_filter, kept_filter, v1};

use super::{BoardFilter, Dim, board_values, kind_name};

/// One quick filter of a board: a Jira board's own, or one the board keeps.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum QuickRef {
    /// A Jira board's own, by Jira's id.
    Jira(u64),
    /// One the board keeps, by id.
    Kept(String),
}

impl QuickRef {
    /// On the wire.
    pub fn to_proto(&self) -> v1::QuickFilterRef {
        match self {
            Self::Jira(id) => jira_filter(*id),
            Self::Kept(id) => kept_filter(id),
        }
    }

    /// Its name on `board`, if it is there.
    pub fn name<'a>(&self, board: &'a v1::Board) -> Option<&'a str> {
        match self {
            Self::Jira(id) => board
                .jira
                .as_deref()?
                .quick_filters
                .iter()
                .find(|q| q.id == *id)
                .map(|q| q.name.as_str()),
            Self::Kept(id) => board
                .quick_filters
                .iter()
                .find(|q| q.id == *id)
                .map(|q| q.name.as_str()),
        }
    }

    /// Whether it is on in `filter`.
    pub fn is_on(&self, filter: &BoardFilter) -> bool {
        match self {
            Self::Jira(id) => filter.quick.contains(id),
            Self::Kept(id) => filter.saved.contains(id),
        }
    }
}

/// Whether `task` itself passes a rule of a search, kinds and tags (every one given).
/// JQL rules pass here: Jira decides them.
pub fn passes(rule: &v1::QuickFilterRule, task: TaskRef<'_>) -> bool {
    if !rule.jql.is_empty() {
        return true;
    }
    let search = rule.search.trim().to_lowercase();
    if !search.is_empty()
        && !task.title().to_lowercase().contains(&search)
        && !task.notes().to_lowercase().contains(&search)
    {
        return false;
    }
    if !rule.kinds.is_empty() && !rule.kinds.contains(&task.kind()) {
        return false;
    }
    rule.tags.is_empty()
        || task
            .tags()
            .iter()
            .any(|t| rule.tags.iter().any(|r| r.eq_ignore_ascii_case(t)))
}

/// What is chosen on a board of Urushi's own, as a rule: the search, the types as
/// kinds, and the labels as tags (spelled as the tasks spell them). `None` when nothing
/// is chosen.
pub fn rule_from_selection(f: &BoardFilter, board: &v1::Board) -> Option<v1::QuickFilterRule> {
    let chosen = |dim: Dim| f.chosen.get(&dim).into_iter().flatten();
    let kinds = chosen(Dim::Type)
        .filter_map(|name| {
            KINDS
                .iter()
                .find(|k| kind_name(i32::from(**k)) == Some(name.as_str()))
                .map(|k| i32::from(*k))
        })
        .collect::<Vec<_>>();
    let labels = board_values(board, Dim::Label);
    let tags = chosen(Dim::Label)
        .map(|v| {
            labels
                .iter()
                .find(|(value, _)| value == v)
                .map_or_else(|| v.clone(), |(_, label)| label.clone())
        })
        .collect::<Vec<_>>();
    let rule = v1::QuickFilterRule {
        search: f.search.text().trim().to_owned(),
        kinds,
        tags,
        jql: String::new(),
    };
    (!rule.search.is_empty() || !rule.kinds.is_empty() || !rule.tags.is_empty()).then_some(rule)
}

/// A rule in a few words: `Search "login" · Type: Bug · Tags: ui`, or its JQL.
pub fn describe(rule: &v1::QuickFilterRule) -> String {
    if !rule.jql.is_empty() {
        return rule.jql.clone();
    }
    let mut parts = Vec::new();
    if !rule.search.is_empty() {
        parts.push(format!("Search \"{}\"", rule.search));
    }
    let kinds: Vec<&str> = rule.kinds.iter().filter_map(|k| kind_name(*k)).collect();
    if !kinds.is_empty() {
        parts.push(format!("Type: {}", kinds.join(", ")));
    }
    if !rule.tags.is_empty() {
        parts.push(format!("Tags: {}", rule.tags.join(", ")));
    }
    parts.join(" · ")
}

/// Kinds a board's tasks can have, for matching type names to them.
const KINDS: [v1::TaskKind; 5] = [
    v1::TaskKind::Task,
    v1::TaskKind::Story,
    v1::TaskKind::Bug,
    v1::TaskKind::Epic,
    v1::TaskKind::Subtask,
];

#[cfg(test)]
mod tests;
