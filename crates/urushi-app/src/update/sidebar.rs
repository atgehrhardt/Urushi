//! The desktop sidebar: hiding and showing it, and dragging its edge
//! (`crate::sidebar` holds the rules).
//!
//! Dragging the edge reuses the runner's node drags: the edge's `on_drag` starts the
//! resize, then `DragMove` follows the pointer and `DragCancel` (a release over no drop
//! target) ends it.

use crate::msg::{Msg, SidebarMsg};
use crate::state::AppState;

impl AppState {
    /// Handles a sidebar message.
    pub(super) fn sidebar_msg(&mut self, msg: SidebarMsg) {
        match msg {
            // Phones show the list or the terminals instead (`sidebar_open`).
            SidebarMsg::Toggle if !self.compact => {
                self.sidebar.end_resize();
                self.sidebar.toggle();
            }
            SidebarMsg::StartResize if !self.compact && self.dialog.is_none() => {
                self.menu = None;
                // Only movement matters: the grab offset is measured from here, so the
                // sidebar's own position (panel margin, safe area) cancels out.
                let x = self.last_press.map_or(0.0, |p| p.x);
                self.sidebar.start_resize(x);
            }
            SidebarMsg::Reveal(reveal) => self.sidebar.set_reveal(reveal),
            SidebarMsg::WindowWidth(width) => self.sidebar.set_window(width),
            SidebarMsg::Toggle | SidebarMsg::StartResize | SidebarMsg::EdgePressed => {}
        }
    }

    /// `DragMove` and `DragCancel` while the sidebar's edge is being dragged.
    pub(super) fn sidebar_drag(&mut self, msg: &Msg) {
        match *msg {
            Msg::DragMove(p) => self.sidebar.drag_to(p.x),
            Msg::DragCancel => self.sidebar.end_resize(),
            _ => {}
        }
    }
}
