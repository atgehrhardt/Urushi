//! The quick-filter dialog's draft (ADR 0033): saving what is chosen on the board as a
//! quick filter, or changing one. On a Jira board it holds the JQL to send, which the
//! person can read and change; on a board of Urushi's own, the search, kinds and tags.

use urushi_client::v1;
use urushi_ui::TextField;

use super::quick::QuickRef;

/// What the quick-filter dialog is editing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuickFilterEditor {
    /// The board (by id).
    pub board: String,
    /// The filter being changed; `None` for a new one.
    pub editing: Option<QuickRef>,
    /// Its name.
    pub name: TextField,
    /// On a Jira board: its JQL (several lines).
    pub jql: Option<TextField>,
    /// On a board of Urushi's own: what it will filter by; `None` keeps what a filter
    /// being changed has.
    pub rule: Option<v1::QuickFilterRule>,
    /// On a board of Urushi's own, changing a filter: what it filters by now.
    pub current: Option<v1::QuickFilterRule>,
    /// Waiting for the daemon (and Jira).
    pub busy: bool,
    /// Why the last try failed.
    pub error: Option<String>,
}

impl QuickFilterEditor {
    /// What the filter will let through, as the dialog shows it (local boards).
    pub fn shown_rule(&self) -> Option<&v1::QuickFilterRule> {
        self.rule.as_ref().or(self.current.as_ref())
    }

    /// The rule to send: the JQL on a Jira board, otherwise the new search, kinds and
    /// tags (none when they stay as they are).
    pub fn rule_to_send(&self) -> Option<v1::QuickFilterRule> {
        match &self.jql {
            Some(jql) => Some(v1::QuickFilterRule {
                jql: jql.text().trim().to_owned(),
                ..v1::QuickFilterRule::default()
            }),
            None => self.rule.clone(),
        }
    }
}
