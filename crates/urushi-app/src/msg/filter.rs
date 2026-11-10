//! Messages for filtering a board (`filter.rs`): the search, people, any field's
//! values, "Recently updated", quick filters (a Jira board's and the board's own), and
//! saving, changing and deleting quick filters (protocol 1.21).

use crate::filter::Dim;
use crate::filter::quick::QuickRef;

/// A change to the shown board's filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterMsg {
    /// Choose a value in a dimension, or unchoose it.
    Toggle(Dim, String),
    /// "Only my issues" on or off.
    Mine,
    /// "Recently updated" on or off.
    Recent,
    /// A Jira board's own quick filter (by id) on or off.
    Quick(u64),
    /// A quick filter the board keeps (by id) on or off.
    Saved(String),
    /// Clear every filter, the search too.
    Clear,
    /// Open the dialog that saves what is chosen as a quick filter.
    OpenSave,
    /// Open the dialog that changes a quick filter.
    OpenEdit(QuickRef),
    /// On a board of Urushi's own, while changing a filter: make it filter by what is
    /// chosen now.
    UseCurrent,
    /// Ask before deleting a quick filter.
    OpenDelete(QuickRef),
    /// Delete the quick filter the dialog asks about.
    ConfirmDelete,
}
