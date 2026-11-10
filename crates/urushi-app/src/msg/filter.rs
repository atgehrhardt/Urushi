//! Messages for filtering a board (`filter.rs`): the search, people, any field's
//! values, "Recently updated", and a Jira board's quick filters.

use crate::filter::Dim;

/// A change to the shown board's filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterMsg {
    /// Choose a value in a dimension, or unchoose it.
    Toggle(Dim, String),
    /// "Only my issues" on or off.
    Mine,
    /// "Recently updated" on or off.
    Recent,
    /// A quick filter (by id) on or off.
    Quick(u64),
    /// Clear every filter, the search too.
    Clear,
}
