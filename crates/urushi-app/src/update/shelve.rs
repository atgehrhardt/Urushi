//! Shelving and unshelving a thread (UR-75, ADR 0060). The daemon stops or starts its
//! terminals; this sends the request, asks first while an agent works in the thread,
//! and shows "shelving…" or "unshelving…" on its card until the workspace says it is
//! done (ADR 0056).

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{Effect, Followup};
use crate::state::{AppState, Dialog};

impl AppState {
    /// Shelve `thread`, asking first (unless `confirmed`) while an agent works in it.
    pub(super) fn shelve(&mut self, thread: String, confirmed: bool, fx: &mut Vec<Effect>) {
        let Some(t) = self.workspace.thread(&thread) else {
            return;
        };
        if !self.daemon_supports_shelving() || t.shelved || self.shelving.contains_key(&thread) {
            return;
        }
        if !confirmed && self.agent_works_in(&thread) {
            self.dialog = Some(Dialog::ShelveThread { thread });
            return;
        }
        if matches!(&self.dialog, Some(Dialog::ShelveThread { .. })) {
            self.dialog = None;
        }
        self.send_shelved(thread, true, fx);
    }

    /// Unshelve `thread`.
    pub(super) fn unshelve(&mut self, thread: String, fx: &mut Vec<Effect>) {
        let shelved = self.workspace.thread(&thread).is_some_and(|t| t.shelved);
        if self.daemon_supports_shelving() && shelved && !self.shelving.contains_key(&thread) {
            self.send_shelved(thread, false, fx);
        }
    }

    /// Forgets the shelves and unshelves the workspace now shows done (or whose thread
    /// is gone).
    pub(super) fn shelving_shown(&mut self) {
        let workspace = &self.workspace;
        self.shelving
            .retain(|id, wanted| workspace.thread(id).is_some_and(|t| t.shelved != *wanted));
    }

    fn send_shelved(&mut self, thread: String, shelved: bool, fx: &mut Vec<Effect>) {
        self.shelving.insert(thread.clone(), shelved);
        fx.push(request(
            Body::SetThreadShelved(v1::SetThreadShelvedRequest {
                thread_id: thread.clone(),
                shelved,
            }),
            Followup::Shelving(thread),
        ));
    }

    /// Whether an agent is working in one of `thread`'s panes (ADR 0039).
    fn agent_works_in(&self, thread: &str) -> bool {
        self.workspace
            .tabs_of(thread)
            .iter()
            .any(|tab| !tab.running.is_empty())
    }
}
