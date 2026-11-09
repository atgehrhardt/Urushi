//! Coding-agent CLIs that a new tab can start: Claude Code, Codex and Cursor.
//!
//! A tab running an agent is still an ordinary terminal. The daemon starts the user's
//! shell and types the agent's command into it, so the shell is still there when the
//! agent exits. This module knows each agent's program name and its "skip permission
//! prompts" flag, how it resumes a conversation (`resume.rs`), and how it starts for a
//! task (`mode.rs`). Finding the program on disk is the daemon's job.

pub use mode::{AgentMode, TASK_PROMPT_ENV};
pub use resume::{AgentSessionId, InvalidSessionId};

mod mode;
mod resume;

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::shell::quote as shell_quote;

/// An agent CLI that Urushi knows how to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    /// Anthropic's Claude Code (`claude`).
    Claude,
    /// The Codex CLI from `OpenAI` (`codex`).
    Codex,
    /// The Cursor CLI (`cursor-agent`).
    Cursor,
}

impl Agent {
    /// Every agent, in the order pickers show them.
    pub const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::Cursor];

    /// Name shown to the user, and the title of a tab running it.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
        }
    }

    /// The executable to look for on `PATH`.
    pub const fn program(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor-agent",
        }
    }

    /// The flag that makes the agent run tools without asking first. Off by default;
    /// the user opts in per agent in settings.
    pub const fn skip_permissions_flag(self) -> &'static str {
        match self {
            Self::Claude => "--dangerously-skip-permissions",
            Self::Codex => "--dangerously-bypass-approvals-and-sandbox",
            Self::Cursor => "--force",
        }
    }

    /// The command line to type into a shell. `program` is what the shell should run
    /// (the bare name, or a full path when the name isn't on the shell's `PATH`).
    pub fn command_line(self, program: &str, skip_permissions: bool) -> String {
        let mut line = shell_quote(program);
        if skip_permissions {
            line.push(' ');
            line.push_str(self.skip_permissions_flag());
        }
        line
    }
}

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Per-agent settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    /// Agents started with their skip-permissions flag. Empty by default: every agent
    /// asks before running tools.
    pub skip_permissions: BTreeSet<Agent>,
    /// The agent a thread started from a task runs. `None` until the user picks one
    /// (the app's setup wizard asks); such threads then open on the launcher.
    pub task_agent: Option<Agent>,
    /// The mode [`Self::task_agent`] starts in. Only applies to task starts.
    pub task_mode: AgentMode,
}

impl AgentSettings {
    /// Whether `agent` is started with its skip-permissions flag.
    pub fn skips_permissions(&self, agent: Agent) -> bool {
        self.skip_permissions.contains(&agent)
    }

    /// Turns the skip-permissions flag on or off for `agent`.
    pub fn set_skip_permissions(&mut self, agent: Agent, skip: bool) {
        if skip {
            self.skip_permissions.insert(agent);
        } else {
            self.skip_permissions.remove(&agent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_command_asks_for_permission() {
        assert_eq!(Agent::Claude.command_line("claude", false), "claude");
        assert_eq!(
            Agent::Claude.command_line("claude", true),
            "claude --dangerously-skip-permissions"
        );
        assert_eq!(
            Agent::Codex.command_line("codex", true),
            "codex --dangerously-bypass-approvals-and-sandbox"
        );
        assert_eq!(
            Agent::Cursor.command_line("cursor-agent", true),
            "cursor-agent --force"
        );
    }

    #[test]
    fn full_paths_with_spaces_are_quoted() {
        assert_eq!(
            Agent::Claude.command_line("/Users/a b/bin/claude", false),
            "'/Users/a b/bin/claude'"
        );
        assert_eq!(
            Agent::Claude.command_line("/opt/it's/claude", false),
            r"'/opt/it'\''s/claude'"
        );
    }

    #[test]
    fn skip_permissions_is_per_agent_and_off_by_default() {
        let mut s = AgentSettings::default();
        assert!(Agent::ALL.iter().all(|a| !s.skips_permissions(*a)));
        s.set_skip_permissions(Agent::Codex, true);
        assert!(s.skips_permissions(Agent::Codex));
        assert!(!s.skips_permissions(Agent::Claude));
        s.set_skip_permissions(Agent::Codex, false);
        assert_eq!(s, AgentSettings::default());
    }

    #[test]
    fn older_settings_have_no_task_agent() {
        let s: AgentSettings = serde_json::from_str(r#"{"skip_permissions":["claude"]}"#).unwrap();
        assert_eq!(s.task_agent, None);
        assert_eq!(s.task_mode, AgentMode::Normal);
        let s = AgentSettings {
            task_agent: Some(Agent::Claude),
            task_mode: AgentMode::Plan,
            ..AgentSettings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<AgentSettings>(&json).unwrap(), s);
    }

    #[test]
    fn settings_serialize_as_names() {
        let mut s = AgentSettings::default();
        s.set_skip_permissions(Agent::Claude, true);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"skip_permissions":["claude"],"task_agent":null,"task_mode":"normal"}"#
        );
        assert_eq!(serde_json::from_str::<AgentSettings>(&json).unwrap(), s);
    }
}
