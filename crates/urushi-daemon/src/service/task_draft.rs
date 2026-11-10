//! Drafting a task's message in its agent instead of sending it (ADR 0030).
//!
//! No agent CLI can start with text waiting in its input, so the agent starts without a
//! prompt and the daemon pastes the message (bracketed, without Enter) once the agent
//! reads input:
//! - **Claude** says so itself: Urushi's `SessionStart` hook writes the `ready` signal
//!   (`urushi_core::agent::hooks`), after Claude's own startup dialogs such as folder
//!   trust, so the draft can't land in one.
//! - **Codex and Cursor** have no hook Urushi can add. The pane is taken as ready when
//!   bracketed paste is on and the agent itself is the shell's foreground job, so the
//!   shell's own prompt (which also asks for bracketed paste) doesn't count.
//!
//! Either way the screen must then have been still for [`QUIET`]. Until the agent is
//! ready the user may type in the pane: that answers the agent's startup dialogs (or the
//! shell). Once it is ready, typing or clicking there ([`Service::user_acted`]) means
//! the user took over, and the draft is dropped, with a log line. So it is if the
//! pane's program exits or nothing is ready in time. The thread and tab stay as they
//! are, and the message is still in the pane's `URUSHI_TASK_PROMPT`.

use std::time::Duration;

use tokio::time::Instant;
use urushi_core::{Agent, PaneId};
use urushi_mux::{Input, SessionHandle, SessionStatus};

use super::{Service, lock};
use crate::usage::process::{agent_process_under, foreground_job};

/// How long Claude may take to say it is ready: long enough for the user to answer its
/// startup dialogs.
const SIGNAL_TIMEOUT: Duration = Duration::from_secs(120);
/// How long an agent without a ready signal may take to start.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the screen must stay unchanged before the draft is pasted, so it lands in
/// the agent's input once it has finished drawing.
const QUIET: Duration = Duration::from_millis(500);
/// How often the pane is looked at while nothing changes.
const POLL: Duration = Duration::from_millis(100);
/// Least time between two looks at the process table (it runs `ps`).
const PROCESS_CHECK: Duration = Duration::from_millis(500);

/// Why a draft wasn't pasted.
#[derive(Debug, PartialEq, Eq)]
enum Dropped {
    /// The user typed or clicked in the pane after the agent was ready.
    UserActed,
    /// The pane's program exited, or the pane is gone.
    Exited,
    /// The agent didn't become ready in time.
    TimedOut,
}

/// A draft waiting for its pane's agent.
pub(super) struct PendingDraft {
    pane: PaneId,
    /// Ready signals the pane had seen before the agent was started.
    ready_before: u64,
}

impl Service {
    /// Records that `pane` will get a drafted message, before the agent's command is
    /// typed, so its ready signal can't be missed. `None` if the pane is gone.
    pub(super) fn expect_draft(&self, pane: PaneId) -> Option<PendingDraft> {
        let ready_before = self.session(pane).ok()?.status().borrow().ready;
        lock(&self.drafts).insert(pane, 0);
        Some(PendingDraft { pane, ready_before })
    }

    /// Pastes `prompt` into `agent` in the pending draft's pane once it reads input, in
    /// the background.
    pub(super) fn draft_when_ready(&self, draft: PendingDraft, agent: Agent, prompt: String) {
        let Some(service) = self.me.upgrade() else {
            return;
        };
        tokio::spawn(async move {
            let pane = draft.pane;
            let outcome = service.paste_when_ready(&draft, agent, prompt).await;
            lock(&service.drafts).remove(&pane);
            if let Err(why) = outcome {
                tracing::warn!(%pane, %agent, ?why, "task message not drafted");
            }
        });
    }

    /// The user typed or clicked in `pane`: they have seen it. A draft waiting there
    /// counts it: once its agent is ready, a draft would interleave with what the user
    /// types, so it is dropped.
    pub fn user_acted(&self, pane: PaneId) {
        self.clear_attention([pane]);
        if let Some(acts) = lock(&self.drafts).get_mut(&pane) {
            *acts += 1;
        }
    }

    /// Takes `pane`'s draft to paste it, unless the user acted since `acts`: checked and
    /// taken at once, as the user may act meanwhile.
    fn take_draft(&self, pane: PaneId, acts: u64) -> bool {
        let mut drafts = lock(&self.drafts);
        if drafts.get(&pane) != Some(&acts) {
            return false;
        }
        drafts.remove(&pane);
        true
    }

    /// How often the user acted on `pane` since its draft was expected; `None` once the
    /// draft is gone.
    fn user_acts(&self, pane: PaneId) -> Option<u64> {
        lock(&self.drafts).get(&pane).copied()
    }

    async fn paste_when_ready(
        &self,
        draft: &PendingDraft,
        agent: Agent,
        prompt: String,
    ) -> Result<(), Dropped> {
        let session = self.session(draft.pane).map_err(|_| Dropped::Exited)?;
        let mut status = session.status();
        let timeout = if agent.says_when_ready() {
            SIGNAL_TIMEOUT
        } else {
            START_TIMEOUT
        };
        let deadline = Instant::now() + timeout;
        let mut version = status.borrow().version;
        let mut changed_at = Instant::now();
        let mut checked_processes: Option<Instant> = None;
        // The user's actions counted when the agent was first seen ready.
        let mut ready_at: Option<u64> = None;
        loop {
            let acts = self.user_acts(draft.pane).ok_or(Dropped::Exited)?;
            let now = *status.borrow_and_update();
            if now.exit_code.is_some() {
                return Err(Dropped::Exited);
            }
            if now.version != version {
                version = now.version;
                changed_at = Instant::now();
            }
            if ready_at.is_none() {
                let ready = if agent.says_when_ready() {
                    now.ready > draft.ready_before
                } else {
                    let due = checked_processes.is_none_or(|at| at.elapsed() >= PROCESS_CHECK);
                    due && {
                        checked_processes = Some(Instant::now());
                        self.agent_has_the_input(&session, now, agent).await
                    }
                };
                ready_at = ready.then_some(acts);
            }
            if let Some(before) = ready_at {
                if acts != before {
                    return Err(Dropped::UserActed);
                }
                if changed_at.elapsed() >= QUIET {
                    if !self.take_draft(draft.pane, before) {
                        return Err(Dropped::UserActed);
                    }
                    return session
                        .input(Input::Paste(prompt))
                        .await
                        .map_err(|_| Dropped::Exited);
                }
            }
            if Instant::now() >= deadline {
                return Err(Dropped::TimedOut);
            }
            if let Ok(Err(_)) = tokio::time::timeout(POLL, status.changed()).await {
                return Err(Dropped::Exited);
            }
        }
    }

    /// Whether `agent` reads input in `session`'s pane: bracketed paste is on, and the
    /// agent is the foreground job of the pane's shell.
    async fn agent_has_the_input(
        &self,
        session: &SessionHandle,
        status: SessionStatus,
        agent: Agent,
    ) -> bool {
        let Ok(frame) = session.frame_since(status.version).await else {
            return false;
        };
        let Some(shell) = session.pid() else {
            return false;
        };
        if !frame.modes.bracketed_paste {
            return false;
        }
        let processes = self.usage.processes_now().await;
        !foreground_job(&processes, shell).is_empty()
            && agent_process_under(&processes, shell).is_some_and(|(a, _)| a == agent)
    }
}
