//! Filtering the shown board (`filter.rs`): the search field (which takes the keyboard
//! while focused, though no dialog is open), choosing values, and asking the daemon
//! which issues a Jira board's quick filters let through, again whenever the board
//! changes while one is on.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::filter::Context;
use crate::msg::{Effect, FilterMsg, Followup};
use crate::state::{AppState, Field};

impl AppState {
    /// Whether the board's search field has the keyboard.
    pub(crate) fn searching(&self) -> bool {
        self.dialog.is_none() && self.focus == Some(Field::BoardSearch) && self.board.visible()
    }

    /// Who "me" is and the time, for the filter.
    pub(crate) fn filter_context(&self) -> Context<'_> {
        let me = self.jira.as_ref().filter(|a| a.connected).map(|a| {
            if a.account_id.is_empty() {
                a.display_name.as_str()
            } else {
                a.account_id.as_str()
            }
        });
        Context {
            me,
            now_ms: self.now_ms,
        }
    }

    /// A change to the shown board's filter.
    pub(super) fn filter_msg(&mut self, msg: FilterMsg, fx: &mut Vec<Effect>) {
        let f = &mut self.board.filter;
        match msg {
            FilterMsg::Toggle(dim, value) => f.toggle(dim, &value),
            FilterMsg::Mine => f.mine = !f.mine,
            FilterMsg::Recent => f.recent = !f.recent,
            FilterMsg::Quick(id) => {
                if !f.quick.remove(&id) {
                    f.quick.insert(id);
                }
                f.quick_matches = None;
                self.board.quick_asking = false;
                self.refresh_quick_filter(fx);
            }
            FilterMsg::Clear => {
                f.clear();
                if self.focus == Some(Field::BoardSearch) {
                    self.focus = None;
                }
            }
        }
    }

    /// Asks which issues the quick filters on let through, unless an answer is on its
    /// way (then asks again when it comes, if the board changed meanwhile).
    pub(super) fn refresh_quick_filter(&mut self, fx: &mut Vec<Effect>) {
        if self.board.filter.quick.is_empty() {
            return;
        }
        let Some(board) = self.board.board(&self.workspace).map(|b| b.id.clone()) else {
            return;
        };
        if self.board.quick_asking {
            self.board.quick_stale = true;
            return;
        }
        self.board.quick_asking = true;
        self.board.quick_stale = false;
        let body = Body::QueryJiraFilter(v1::QueryJiraFilterRequest {
            board_id: board.clone(),
            quick_filter_ids: self.board.filter.quick.iter().copied().collect(),
        });
        fx.push(request(body, Followup::QuickFilter(board)));
    }

    /// The daemon's answer to a quick-filter query.
    pub(super) fn quick_filter_answered(
        &mut self,
        board: &str,
        ids: Option<Vec<String>>,
        fx: &mut Vec<Effect>,
    ) {
        self.board.quick_asking = false;
        let shown = self.board.board(&self.workspace).map(|b| b.id.as_str());
        if shown != Some(board) || self.board.filter.quick.is_empty() {
            return;
        }
        self.board.filter.quick_matches = ids.map(|ids| ids.into_iter().collect());
        if self.board.quick_stale {
            self.refresh_quick_filter(fx);
        }
    }
}
