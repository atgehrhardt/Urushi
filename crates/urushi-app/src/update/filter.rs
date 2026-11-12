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

    /// Who "me" is (on the board's own sign-in) and the time, for the filter.
    pub(crate) fn filter_context(&self) -> Context<'_> {
        let me = self.shown_jira_account().map(|a| {
            if a.account_id.is_empty() {
                a.display_name.as_str()
            } else {
                a.account_id.as_str()
            }
        });
        let kept = self
            .board
            .board(&self.workspace)
            .map_or(&[][..], |b| b.quick_filters.as_slice());
        Context {
            me,
            now_ms: self.now_ms,
            kept,
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
                self.ask_quick_filter_again(fx);
            }
            FilterMsg::Saved(id) => {
                if !f.saved.remove(&id) {
                    f.saved.insert(id);
                }
                self.ask_quick_filter_again(fx);
            }
            FilterMsg::Clear => {
                f.clear();
                if self.focus == Some(Field::BoardSearch) {
                    self.focus = None;
                }
            }
            other => self.quick_filter_msg(other, fx),
        }
    }

    /// The quick filters on changed: asks Jira again at once, whatever is on its way.
    /// Until it answers, the board stays filtered as Jira last said, so it changes once,
    /// when the answer comes, instead of showing everything meanwhile (UR-66).
    pub(super) fn ask_quick_filter_again(&mut self, fx: &mut Vec<Effect>) {
        let needs_jira = self
            .board
            .board(&self.workspace)
            .is_some_and(|b| self.board.filter.needs_jira(&b.quick_filters));
        if !needs_jira {
            self.board.filter.quick_matches = None;
        }
        self.board.quick_asking = false;
        self.refresh_quick_filter(fx);
    }

    /// Asks which issues the quick filters in JQL that are on let through, unless an
    /// answer is on its way (then asks again when it comes, if the board changed
    /// meanwhile).
    pub(super) fn refresh_quick_filter(&mut self, fx: &mut Vec<Effect>) {
        let Some(board) = self.board.board(&self.workspace) else {
            return;
        };
        let f = &self.board.filter;
        if !f.needs_jira(&board.quick_filters) {
            return;
        }
        let kept = f.kept_jql(&board.quick_filters);
        let board = board.id.clone();
        if self.board.quick_asking {
            self.board.quick_stale = true;
            return;
        }
        self.board.quick_asking = true;
        self.board.quick_stale = false;
        self.board.quick_query += 1;
        let body = Body::QueryJiraFilter(v1::QueryJiraFilterRequest {
            board_id: board.clone(),
            quick_filter_ids: self.board.filter.quick.iter().copied().collect(),
            kept_quick_filter_ids: kept,
        });
        fx.push(request(
            body,
            Followup::QuickFilter(board, self.board.quick_query),
        ));
    }

    /// Tells the daemon which Jira board the panel shows, when that changes, so it is
    /// read every few seconds while on screen.
    pub(super) fn watch_shown_board(&mut self, fx: &mut Vec<Effect>) {
        if !self.daemon_supports_jira_issues()
            || !matches!(self.connection, crate::state::Connection::Connected { .. })
        {
            return;
        }
        let shown = self
            .board
            .board(&self.workspace)
            .filter(|b| self.board.open && b.jira.is_some())
            .map(|b| b.id.clone());
        if shown == self.board.watching {
            return;
        }
        self.board.watching.clone_from(&shown);
        let body = Body::WatchBoard(v1::WatchBoardRequest {
            board_id: shown.unwrap_or_default(),
        });
        fx.push(request(body, Followup::None));
    }

    /// The daemon's answer to a quick-filter query. One to a query older than the last
    /// sent is dropped: the filters it answered for have changed since.
    pub(super) fn quick_filter_answered(
        &mut self,
        (board, query): (&str, u64),
        ids: Option<Vec<String>>,
        fx: &mut Vec<Effect>,
    ) {
        if query != self.board.quick_query {
            return;
        }
        self.board.quick_asking = false;
        let Some(shown) = self.board.board(&self.workspace) else {
            return;
        };
        if shown.id != board || !self.board.filter.needs_jira(&shown.quick_filters) {
            return;
        }
        self.board.filter.quick_matches = ids.map(|ids| ids.into_iter().collect());
        if self.board.quick_stale {
            self.refresh_quick_filter(fx);
        }
    }
}
