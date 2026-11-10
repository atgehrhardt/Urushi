//! The quick terminal's drop-down panel (ADR 0031): dropping it down opens the daemon's
//! quick terminal if there is none, and attaches its panes; putting it away detaches
//! them once it has slid up, while their programs keep running in the daemon.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{Effect, Followup, QuickMsg};
use crate::state::{AppState, Connection, Field};

impl AppState {
    /// Handles a [`QuickMsg`].
    pub(super) fn quick_msg(&mut self, msg: QuickMsg, fx: &mut Vec<Effect>) {
        match msg {
            QuickMsg::Toggle if self.quick.open => self.hide_quick(fx),
            QuickMsg::Toggle => self.show_quick(fx),
            QuickMsg::Hide => self.hide_quick(fx),
            QuickMsg::Reveal(reveal) => {
                self.quick.reveal = reveal.clamp(0.0, 1.0);
                // Fully away: let go of its panes.
                if self.quick.reveal <= 0.0 && !self.quick.open {
                    self.sync_panes(fx);
                }
            }
            QuickMsg::Area(rect) => {
                if self.quick.area != Some(rect) {
                    self.quick.area = Some(rect);
                    self.sync_panes(fx);
                }
            }
        }
    }

    fn show_quick(&mut self, fx: &mut Vec<Effect>) {
        if !self.daemon_supports_quick_terminal()
            || !matches!(self.connection, Connection::Connected { .. })
        {
            return;
        }
        self.quick.open = true;
        self.let_go_of_pointer();
        // The panel takes the keyboard from the board's search.
        if self.focus == Some(Field::BoardSearch) {
            self.focus = None;
        }
        if self.workspace.quick_terminal_raw().is_none() && !self.quick.starting {
            self.quick.starting = true;
            let body = Body::OpenQuickTerminal(v1::OpenQuickTerminalRequest {});
            fx.push(request(body, Followup::QuickTerminal));
        }
        self.sync_panes(fx);
    }

    fn hide_quick(&mut self, fx: &mut Vec<Effect>) {
        if !self.quick.open {
            return;
        }
        self.quick.open = false;
        self.let_go_of_pointer();
        // Already out of sight (no animation ran): let go of its panes now.
        if self.quick.reveal <= 0.0 {
            self.sync_panes(fx);
        }
    }

    /// The panel moves over or off the terminals: no menu, selection or drag survives.
    fn let_go_of_pointer(&mut self) {
        self.menu = None;
        self.selection = None;
        self.drag = None;
        self.hover = None;
    }

    /// The daemon answered `OpenQuickTerminal`: its panes attach (they also arrive with
    /// the workspace). If it failed, the panel goes back up and the error shows.
    pub(super) fn quick_terminal_answered(&mut self, error: Option<String>, fx: &mut Vec<Effect>) {
        self.quick.starting = false;
        if let Some(error) = error {
            self.quick.open = false;
            self.error = Some(error);
        }
        self.sync_panes(fx);
    }

    /// After a workspace change: if the quick terminal ended (its last pane closed or
    /// exited) while its panel was down, put the panel away.
    pub(super) fn follow_quick_terminal(&mut self, had_one: bool) {
        if had_one
            && self.quick.open
            && !self.quick.starting
            && self.workspace.quick_terminal_raw().is_none()
        {
            self.quick.open = false;
            self.let_go_of_pointer();
        }
    }
}
