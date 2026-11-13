//! The board on screen changing, however it happens: picked, created, imported,
//! deleted, reordered, or another host's (UR-71). What belonged to the old board (its
//! filter and Jira's answers about it) would hide cards on the new one, so it goes.

use super::{AppState, Field};
use crate::organize::MenuTarget;

impl AppState {
    /// Called after every message with the board shown before it: if another board is
    /// shown now, starts it with no filter. (The daemon is told which board shows right
    /// after, by `watch_shown_board`.)
    pub(super) fn follow_shown_board(&mut self, before: Option<&str>) {
        let now = self.board.shown_id(&self.workspace);
        if now.as_deref() == before {
            return;
        }
        self.board.reset_for_board();
        if self.focus == Some(Field::BoardSearch) {
            self.focus = None;
        }
        // The Filters menu, or a quick filter's, was the old board's.
        if self.menu.as_ref().is_some_and(|m| {
            matches!(
                m.target,
                MenuTarget::Filter(_) | MenuTarget::QuickFilter(..)
            )
        }) {
            self.menu = None;
        }
    }
}
