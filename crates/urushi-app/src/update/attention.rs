//! Acting on attention (ADR 0017): panes the user is looking at are cleared on the
//! daemon, and new raises elsewhere are announced with a system notification while the
//! window is in the background. Runs after every message, like `want_thumbnails`.

use std::collections::HashSet;

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::attention::{Raise, describe};
use crate::msg::{Effect, Followup};
use crate::state::{AppState, Connection};

/// First protocol minor version with attention (`Tab.attention`, `ClearAttention`).
pub(crate) const ATTENTION_SINCE_MINOR: u32 = 9;

impl AppState {
    /// Clears what the user can see, and announces new raises they can't.
    pub(super) fn attend(&mut self, fx: &mut Vec<Effect>) {
        if self.daemon_minor < ATTENTION_SINCE_MINOR
            || !matches!(self.connection, Connection::Connected { .. })
        {
            return;
        }
        let visible = self.visible_panes();
        let mut current = HashSet::new();
        let mut clear = Vec::new();
        let mut announce = Vec::new();
        for tab in &self.workspace.raw().tabs {
            for a in &tab.attention {
                let raise: Raise = (a.pane_id.clone(), a.since_ms);
                current.insert(raise.clone());
                if visible.contains(&a.pane_id) {
                    if !self.attention.cleared.contains(&raise) {
                        clear.push(raise);
                    }
                } else if !self.attention.announced.contains(&raise) {
                    announce.push((raise, tab.thread_id.clone(), tab.title.clone(), a.kind()));
                }
            }
        }
        self.attention.retain(&current);
        if !clear.is_empty() {
            let pane_ids = clear.iter().map(|(pane, _)| pane.clone()).collect();
            self.attention.cleared.extend(clear);
            let body = Body::ClearAttention(v1::ClearAttentionRequest { pane_ids });
            fx.push(request(body, Followup::None));
        }
        for (raise, thread, tab_title, kind) in announce {
            self.attention.announced.insert(raise);
            // In the foreground, the dot in the sidebar is enough.
            if self.attention.in_background {
                let title = self
                    .workspace
                    .thread(&thread)
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                let body = describe(kind, &tab_title);
                fx.push(Effect::SystemNotification { title, body });
            }
        }
    }

    /// Marks what is already raised when the app connects as announced: it happened
    /// while the app wasn't watching, and the sidebar shows it.
    pub(super) fn seed_attention(&mut self) {
        let raised = self
            .workspace
            .raw()
            .tabs
            .iter()
            .flat_map(|t| &t.attention)
            .map(|a| (a.pane_id.clone(), a.since_ms));
        self.attention.announced = raised.collect();
        self.attention.cleared.clear();
    }
}
