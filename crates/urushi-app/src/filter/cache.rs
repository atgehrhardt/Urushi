//! What the filter decided for the board on screen, kept until the board or the filter
//! changes (UR-66). Views ask on every frame which cards show and which values each
//! field has; on a board of hundreds of issues, working that out again each time made
//! scrolling, hovering and choosing filters lag.
//!
//! The memo lives in the panel's state behind a `RefCell`, so `view()` can fill it
//! while staying a function of the state: it is checked against the board and the
//! filter it was made from on every use, so it never answers for anything else. It
//! takes no part in comparing or cloning states.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

use urushi_client::v1;

use super::{BoardFilter, Context, Dim, board_values};

/// "Recently updated" is decided again each minute.
const MINUTE_MS: i64 = 60_000;

/// The memo of one board's filter results.
#[derive(Debug, Default)]
pub struct FilterCache {
    memo: RefCell<Option<Memo>>,
}

/// What was worked out for `board`.
#[derive(Debug)]
struct Memo {
    board: v1::Board,
    values: BTreeMap<Dim, Rc<[(String, String)]>>,
    shown: Option<(ShownKey, Rc<HashSet<String>>)>,
}

/// What, besides the board, decides which cards show.
#[derive(Debug, PartialEq)]
struct ShownKey {
    filter: BoardFilter,
    me: Option<String>,
    /// The minute it was made in, while "Recently updated" is on.
    minute: Option<i64>,
}

impl Clone for FilterCache {
    /// A copy starts empty: it fills again on first use.
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for FilterCache {
    /// A memo never makes two states differ.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl FilterCache {
    /// Runs `f` on the memo for `board`, starting a new one if the board changed.
    fn with<T>(&self, board: &v1::Board, f: impl FnOnce(&mut Memo) -> T) -> T {
        let mut memo = self.memo.borrow_mut();
        if memo.as_ref().is_some_and(|m| m.board != *board) {
            *memo = None;
        }
        f(memo.get_or_insert_with(|| Memo {
            board: board.clone(),
            values: BTreeMap::new(),
            shown: None,
        }))
    }

    /// The values the board's tasks have in `dim` ([`board_values`]).
    pub fn values(&self, board: &v1::Board, dim: Dim) -> Rc<[(String, String)]> {
        self.with(board, |m| {
            m.values
                .entry(dim)
                .or_insert_with(|| board_values(board, dim).into())
                .clone()
        })
    }

    /// The fields worth offering on the board ([`super::board_dims`]).
    pub fn dims(&self, board: &v1::Board) -> Vec<Dim> {
        Dim::ALL
            .into_iter()
            .filter(|d| board.jira.is_some() || d.local())
            .filter(|d| !self.values(board, *d).is_empty())
            .collect()
    }

    /// The ids of the board's cards that `filter` shows ([`BoardFilter::shows`]).
    pub fn shown(
        &self,
        board: &v1::Board,
        filter: &BoardFilter,
        cx: Context<'_>,
    ) -> Rc<HashSet<String>> {
        let key = ShownKey {
            filter: filter.clone(),
            me: cx.me.map(str::to_owned),
            minute: filter.recent.then_some(cx.now_ms / MINUTE_MS),
        };
        self.with(board, |m| {
            if let Some((k, shown)) = &m.shown
                && *k == key
            {
                return shown.clone();
            }
            let shown: Rc<HashSet<String>> = board
                .columns
                .iter()
                .flat_map(|c| &c.tasks)
                .filter(|t| filter.shows(t, cx))
                .map(|t| t.id.clone())
                .collect::<HashSet<_>>()
                .into();
            m.shown = Some((key, shown.clone()));
            shown
        })
    }

    /// How many of `column`'s cards `filter` shows.
    pub fn count(
        &self,
        board: &v1::Board,
        column: &v1::BoardColumn,
        filter: &BoardFilter,
        cx: Context<'_>,
    ) -> usize {
        if !filter.active() {
            return column.tasks.len();
        }
        let shown = self.shown(board, filter, cx);
        column
            .tasks
            .iter()
            .filter(|t| shown.contains(&t.id))
            .count()
    }
}

#[cfg(test)]
mod tests;
