//! Splitting and closing panes, from the keyboard (the focused pane) or from a pane's
//! context menu (the pane it was opened on). Each is a request to the daemon; the
//! layout changes when the workspace comes back.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{Effect, Followup, PaneMsg};
use crate::organize::MenuTarget;
use crate::state::AppState;

impl AppState {
    /// Splits `pane` of the current tab; the new pane goes on `side` and gets focus.
    pub(super) fn split(&self, pane: String, side: v1::Direction, fx: &mut Vec<Effect>) {
        if let Some(tab) = self.tab_of(&pane) {
            let body = Body::SplitPane(v1::SplitPaneRequest {
                tab_id: tab,
                pane_id: pane,
                side: side.into(),
            });
            fx.push(request(body, Followup::PaneCreated));
        }
    }

    /// Closes `pane` of the current tab (and the tab, if it was the last pane).
    pub(super) fn close_pane(&self, pane: &str, fx: &mut Vec<Effect>) {
        if let Some(tab) = self.tab_of(pane) {
            let body = Body::ClosePane(v1::ClosePaneRequest {
                tab_id: tab,
                pane_id: pane.to_owned(),
            });
            fx.push(request(body, Followup::None));
        }
    }

    /// Opens `pane`'s context menu where the pointer is, focusing the pane first so
    /// typing and pasting go where the menu is.
    pub(super) fn open_pane_menu(&mut self, pane: String, fx: &mut Vec<Effect>) {
        let Some(tab) = self.tab_of(&pane) else {
            return;
        };
        self.focus_pane(&tab, &pane, fx);
        self.open_menu(MenuTarget::Pane(pane));
    }

    /// Handles a [`PaneMsg`]. Every action closes the menu it came from.
    pub(super) fn pane_action(&mut self, msg: PaneMsg, fx: &mut Vec<Effect>) {
        self.menu = None;
        match msg {
            PaneMsg::Split { pane, side } => self.split(pane, side, fx),
            PaneMsg::Close(pane) => self.close_pane(&pane, fx),
            PaneMsg::Copy => self.copy(fx),
            PaneMsg::Paste => fx.push(Effect::RequestPaste),
        }
    }

    /// The current tab's id, if `pane` is one of its panes. A pane that has closed
    /// meanwhile has no tab, so nothing is asked of it.
    fn tab_of(&self, pane: &str) -> Option<String> {
        let tab = self.current_tab()?.id.clone();
        self.tab_has_pane(&tab, pane).then_some(tab)
    }
}
