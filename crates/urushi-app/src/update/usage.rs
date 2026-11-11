//! The status bar's agent usage: on every tick, ask the daemon which agent runs in the
//! focused pane and how much of its plan is used (`GetAgentUsage`, protocol 1.5). The
//! daemon caches, so ticking every few seconds is cheap, and an agent started in a pane
//! shows up within one tick.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::board::BOARDS_SINCE_MINOR;
use crate::msg::{Effect, Followup};
use crate::state::{AppState, Connection};

impl AppState {
    /// A tick: remember the time and refresh the usage of the focused pane's agent.
    pub(super) fn tick(&mut self, now_ms: i64, fx: &mut Vec<Effect>) {
        self.now_ms = now_ms;
        self.poll_usage(fx);
        self.poll_lan_pairing(fx);
    }

    /// Asks for the focused pane's agent usage, unless a request is in flight or the
    /// daemon is too old.
    pub(super) fn poll_usage(&mut self, fx: &mut Vec<Effect>) {
        if self.usage_pending
            || self.daemon_minor < BOARDS_SINCE_MINOR
            || !matches!(self.connection, Connection::Connected { .. })
        {
            return;
        }
        let Some(pane) = self.focused_pane() else {
            return;
        };
        self.usage_pending = true;
        let body = Body::GetAgentUsage(v1::GetAgentUsageRequest {
            pane_id: pane.clone(),
        });
        fx.push(request(body, Followup::Usage(pane)));
    }

    /// The answer (or its failure, which isn't worth a banner: the bar just empties).
    pub(super) fn usage_answered(&mut self, pane: String, usage: Option<v1::AgentUsage>) {
        self.usage_pending = false;
        self.usage = usage.map(|u| (pane, u));
    }

    /// The usage to show: only if it's about the focused pane and an agent runs there.
    pub fn shown_usage(&self) -> Option<&v1::AgentUsage> {
        let (pane, usage) = self.usage.as_ref()?;
        (Some(pane) == self.focused_pane().as_ref() && usage.agent() != v1::Agent::Unspecified)
            .then_some(usage)
    }
}
