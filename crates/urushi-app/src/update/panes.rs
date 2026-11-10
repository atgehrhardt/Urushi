//! Panes: splitting and closing them, from the keyboard (the focused pane) or from a
//! pane's context menu (the pane it was opened on), focusing them, and keeping the
//! panes on screen attached at the right size. Each change is a request to the daemon;
//! the layout changes when the workspace comes back.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{Effect, Followup, PaneMsg};
use crate::organize::MenuTarget;
use crate::state::{AppState, Connection};
use crate::surface::Surface;
use crate::terminal;

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

    /// The id of the quick terminal or the current tab, whichever has `pane`. A pane
    /// that has closed meanwhile is in neither, so nothing is asked of it.
    fn tab_of(&self, pane: &str) -> Option<String> {
        [Surface::Quick, Surface::Main]
            .into_iter()
            .filter_map(|s| self.surface_group(s))
            .map(|(id, _)| id)
            .find(|id| self.group_has_pane(id, pane))
    }

    /// Forgets every attached pane (the connection is new or gone).
    pub(super) fn reset_panes(&mut self) {
        self.quick.starting = false;
        self.screens.clear();
        self.attaching.clear();
        self.sizes.clear();
        self.selection = None;
        self.drag = None;
    }

    /// Attaches the panes on screen (the current tab's, and the quick terminal's while
    /// its panel is out), detaches others, and keeps terminal sizes in step with pane
    /// rectangles. Detaching never stops a pane's program: the daemon keeps it running.
    pub(super) fn sync_panes(&mut self, fx: &mut Vec<Effect>) {
        let wanted = self.pane_rects();
        if self.area.is_none() && self.quick.area.is_none()
            || !matches!(self.connection, Connection::Connected { .. })
        {
            return;
        }
        let wanted_ids: Vec<&String> = wanted.iter().map(|(id, _)| id).collect();
        let stale: Vec<String> = self
            .screens
            .keys()
            .chain(self.attaching.iter())
            .filter(|id| !wanted_ids.contains(id))
            .cloned()
            .collect();
        for pane_id in stale {
            self.screens.remove(&pane_id);
            self.attaching.remove(&pane_id);
            self.sizes.remove(&pane_id);
            fx.push(request(
                Body::DetachPane(v1::DetachPaneRequest { pane_id }),
                Followup::None,
            ));
        }
        for (pane, rect) in wanted {
            let (cols, rows) = terminal::grid_size(rect, self.cell);
            if self.screens.contains_key(&pane) {
                if self.sizes.get(&pane) != Some(&(cols, rows)) {
                    self.sizes.insert(pane.clone(), (cols, rows));
                    let body = Body::SetPaneSize(v1::SetPaneSizeRequest {
                        pane_id: pane,
                        cols: cols.into(),
                        rows: rows.into(),
                    });
                    fx.push(request(body, Followup::None));
                }
            } else if self.attaching.insert(pane.clone()) {
                self.sizes.insert(pane.clone(), (cols, rows));
                let body = Body::AttachPane(v1::AttachPaneRequest {
                    pane_id: pane.clone(),
                    cols: cols.into(),
                    rows: rows.into(),
                });
                fx.push(request(body, Followup::Attached(pane)));
            }
        }
    }

    /// Moves focus to the neighbouring pane towards `side`, in the active surface.
    pub(super) fn focus_towards(&mut self, side: urushi_core::Direction, fx: &mut Vec<Effect>) {
        let Some((id, group)) = self.surface_group(self.active_surface()) else {
            return;
        };
        let Some(current) = self.focused_in(&id).and_then(|p| p.parse().ok()) else {
            return;
        };
        if let Some(next) = group.layout().focus_neighbour(current, side) {
            self.focus_pane(&id, &next.to_string(), fx);
        }
    }

    /// Focuses `pane` of the tab (or quick terminal) `tab`: here at once, and on the
    /// daemon.
    pub(super) fn focus_pane(&mut self, tab: &str, pane: &str, fx: &mut Vec<Effect>) {
        if self.focused_in(tab).as_deref() != Some(pane) {
            self.focused_override
                .insert(tab.to_owned(), pane.to_owned());
            let body = Body::FocusPane(v1::FocusPaneRequest {
                tab_id: tab.to_owned(),
                pane_id: pane.to_owned(),
            });
            fx.push(request(body, Followup::None));
        }
    }
}
