//! The mode an agent starts in when a thread is started from a task, and the command line
//! that starts it with the task as its first prompt (ADR 0014).
//!
//! The prompt itself never appears on the command line: the daemon puts it in the pane's
//! environment as [`TASK_PROMPT_ENV`], and the typed line refers to that variable. That
//! keeps the line short and on one line whatever the task's notes contain.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::Agent;
use crate::shell::quote as shell_quote;

/// Environment variable that holds a task's prompt in the pane its agent starts in.
pub const TASK_PROMPT_ENV: &str = "URUSHI_TASK_PROMPT";

/// How an agent starts: asking as usual, or planning first without changing anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    /// The agent's normal interactive mode.
    #[default]
    Normal,
    /// Read-only planning: the agent proposes a plan before it edits anything.
    Plan,
}

impl AgentMode {
    /// Every mode, in the order pickers show them.
    pub const ALL: [Self; 2] = [Self::Normal, Self::Plan];

    /// Name shown to the user.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Plan => "Plan",
        }
    }
}

impl fmt::Display for AgentMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

impl Agent {
    /// Whether this agent can start in `mode`. Codex has no planning mode to start in.
    pub const fn supports(self, mode: AgentMode) -> bool {
        match mode {
            AgentMode::Normal => true,
            AgentMode::Plan => matches!(self, Self::Claude | Self::Cursor),
        }
    }

    /// `mode` if this agent supports it, else [`AgentMode::Normal`].
    pub const fn effective_mode(self, mode: AgentMode) -> AgentMode {
        if self.supports(mode) {
            mode
        } else {
            AgentMode::Normal
        }
    }

    /// The command line that starts this agent for a task: in `mode` (when supported),
    /// with the skip-permissions choice, and with the prompt read from
    /// [`TASK_PROMPT_ENV`] in the shell that runs it.
    ///
    /// Claude in plan mode with skip-permissions gets
    /// `--allow-dangerously-skip-permissions`, so it starts by planning and the user can
    /// switch to bypassing prompts once the plan is approved.
    pub fn task_command_line(
        self,
        program: &str,
        skip_permissions: bool,
        mode: AgentMode,
    ) -> String {
        let mut words = vec![shell_quote(program)];
        match (self, self.effective_mode(mode)) {
            (Self::Claude, AgentMode::Plan) => {
                words.extend(["--permission-mode".into(), "plan".into()]);
                if skip_permissions {
                    words.push("--allow-dangerously-skip-permissions".into());
                }
            }
            (Self::Cursor, AgentMode::Plan) => {
                words.push("--plan".into());
                if skip_permissions {
                    words.push(self.skip_permissions_flag().into());
                }
            }
            _ => {
                if skip_permissions {
                    words.push(self.skip_permissions_flag().into());
                }
            }
        }
        words.push(format!("\"${TASK_PROMPT_ENV}\""));
        words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_mode_is_offered_where_the_agent_has_one() {
        assert!(Agent::Claude.supports(AgentMode::Plan));
        assert!(Agent::Cursor.supports(AgentMode::Plan));
        assert!(!Agent::Codex.supports(AgentMode::Plan));
        assert!(Agent::ALL.iter().all(|a| a.supports(AgentMode::Normal)));
        assert_eq!(
            Agent::Codex.effective_mode(AgentMode::Plan),
            AgentMode::Normal
        );
    }

    #[test]
    fn task_command_lines_per_agent_mode_and_skip() {
        let line = |a: Agent, skip, mode| a.task_command_line(a.program(), skip, mode);
        let p = "\"$URUSHI_TASK_PROMPT\"";
        assert_eq!(
            line(Agent::Claude, false, AgentMode::Normal),
            format!("claude {p}")
        );
        assert_eq!(
            line(Agent::Claude, true, AgentMode::Normal),
            format!("claude --dangerously-skip-permissions {p}")
        );
        assert_eq!(
            line(Agent::Claude, false, AgentMode::Plan),
            format!("claude --permission-mode plan {p}")
        );
        assert_eq!(
            line(Agent::Claude, true, AgentMode::Plan),
            format!("claude --permission-mode plan --allow-dangerously-skip-permissions {p}")
        );
        assert_eq!(
            line(Agent::Cursor, true, AgentMode::Plan),
            format!("cursor-agent --plan --force {p}")
        );
        assert_eq!(
            line(Agent::Codex, true, AgentMode::Plan),
            format!("codex --dangerously-bypass-approvals-and-sandbox {p}")
        );
        assert_eq!(
            Agent::Claude.task_command_line("/a b/claude", false, AgentMode::Normal),
            format!("'/a b/claude' {p}")
        );
    }

    #[test]
    fn modes_serialize_as_names() {
        assert_eq!(
            serde_json::to_string(&AgentMode::Plan).unwrap(),
            r#""plan""#
        );
        assert_eq!(AgentMode::default(), AgentMode::Normal);
    }
}
