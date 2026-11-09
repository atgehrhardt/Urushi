//! The task agent: which agent CLI, in which mode, a thread started from a task runs
//! (ADR 0014), and the first-run setup wizard that asks for it.
//!
//! The choice lives in the daemon's settings (`Settings.agents.task_agent` /
//! `task_mode`), so every client shares it. This module only reads it and holds what the
//! wizard needs between steps.

use urushi_core::{Agent, AgentMode};
use urushi_proto::convert;

use crate::state::AppState;

/// First protocol minor version with the task agent (`CreateThread.start_task_agent`,
/// `AgentSettings.task_agent` / `task_mode`).
pub const TASK_AGENT_SINCE_MINOR: u32 = 7;

/// First protocol minor version that starts a task in an existing thread
/// (`CreateTab.{task_id, subtask_id, start_task_agent}`, ADR 0018).
pub const TASK_IN_THREAD_SINCE_MINOR: u32 = 10;

/// A step of the setup wizard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupStep {
    /// Choose the agent.
    Agent,
    /// Choose the mode it starts in.
    Mode,
}

/// A choice made in the setup wizard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupMsg {
    /// Choose this agent.
    PickAgent(Agent),
    /// Choose this mode.
    PickMode(AgentMode),
    /// Go on to the next step.
    Next,
    /// Go back a step.
    Back,
    /// Save the choice.
    Finish,
    /// Not now: close it until the app next starts.
    Later,
}

/// When the wizard may open: it waits for the installed agents, and stays closed for
/// the rest of the run once dismissed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SetupGate {
    /// The daemon has said which agents are installed.
    pub agents_loaded: bool,
    /// The user chose "Later" (or closed it) in this run.
    pub dismissed: bool,
}

impl AppState {
    /// Whether the daemon can start the task agent (protocol 1.7).
    pub fn daemon_supports_task_agent(&self) -> bool {
        self.daemon_minor >= TASK_AGENT_SINCE_MINOR
    }

    /// Whether the daemon can start a task in an existing thread (protocol 1.10).
    pub fn daemon_supports_task_in_thread(&self) -> bool {
        self.daemon_minor >= TASK_IN_THREAD_SINCE_MINOR
    }

    /// The task agent and the mode it actually starts in, if the user chose one and the
    /// daemon can start it.
    pub fn task_agent(&self) -> Option<(Agent, AgentMode)> {
        if !self.daemon_supports_task_agent() {
            return None;
        }
        let a = self.settings.as_ref()?.agents?;
        let agent = convert::agent_from_proto(a.task_agent)?;
        let mode = agent.effective_mode(convert::agent_mode_from_proto(a.task_mode));
        Some((agent, mode))
    }

    /// The mode the user chose for the task agent (even if that agent can't use it).
    pub fn task_mode_setting(&self) -> AgentMode {
        self.settings
            .as_ref()
            .and_then(|s| s.agents)
            .map_or(AgentMode::Normal, |a| {
                convert::agent_mode_from_proto(a.task_mode)
            })
    }

    /// Whether the setup wizard should open now: a 1.7 daemon with no task agent chosen,
    /// at least one agent installed, nothing else open, and not dismissed in this run.
    pub fn setup_due(&self) -> bool {
        self.daemon_supports_task_agent()
            && self.dialog.is_none()
            && !self.setup.dismissed
            && self.setup.agents_loaded
            && !self.agents.is_empty()
            && self
                .settings
                .as_ref()
                .is_some_and(|s| s.agents.is_some_and(|a| a.task_agent == 0))
    }
}

/// How the task agent starts, in words: "Claude in plan mode".
pub fn describe(agent: Agent, mode: AgentMode) -> String {
    match mode {
        AgentMode::Normal => agent.display_name().to_owned(),
        AgentMode::Plan => format!("{agent} in plan mode"),
    }
}

/// What a mode does, for the wizard and Settings.
pub fn mode_detail(mode: AgentMode) -> &'static str {
    match mode {
        AgentMode::Normal => "Works right away, asking before commands and edits.",
        AgentMode::Plan => "Proposes a plan first; nothing changes until you approve it.",
    }
}
