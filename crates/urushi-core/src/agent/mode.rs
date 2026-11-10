//! The mode an agent starts in when a thread is started from a task, whether the task's
//! message is sent or only drafted, and the command line that starts it (ADR 0014, ADR
//! 0030).
//!
//! The prompt itself never appears on the command line: the daemon puts it in the pane's
//! environment as [`TASK_PROMPT_ENV`], and the typed line refers to that variable. That
//! keeps the line short and on one line whatever the task's notes contain. A drafted
//! message isn't on the line at all: the daemon pastes it into the agent once it is ready.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::Agent;

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

/// What happens to a task's message when its agent starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptDelivery {
    /// The agent starts with the message as its first prompt and works on it at once.
    #[default]
    Send,
    /// The message is typed into the agent's input, unsent, for the user to edit and send
    /// (ADR 0030).
    Draft,
}

impl PromptDelivery {
    /// `Draft` when `draft`, else `Send`: how the wire's `task_draft` flag reads.
    pub const fn drafting(draft: bool) -> Self {
        if draft { Self::Draft } else { Self::Send }
    }

    /// Whether the message is only drafted.
    pub const fn is_draft(self) -> bool {
        matches!(self, Self::Draft)
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
    /// with the skip-permissions choice, and, when the message is sent, with the prompt
    /// read from [`TASK_PROMPT_ENV`] in the shell that runs it. A drafted message is left
    /// off: the agent starts with an empty input.
    ///
    /// Claude in plan mode with skip-permissions gets
    /// `--allow-dangerously-skip-permissions`, so it starts by planning and the user can
    /// switch to bypassing prompts once the plan is approved.
    pub fn task_command_line(
        self,
        program: &str,
        skip_permissions: bool,
        mode: AgentMode,
        delivery: PromptDelivery,
    ) -> String {
        let mut words = self.program_words(program);
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
        if delivery == PromptDelivery::Send {
            words.push(format!("\"${TASK_PROMPT_ENV}\""));
        }
        words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::hooks;
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
        let line = |a: Agent, skip, mode| {
            a.task_command_line(a.program(), skip, mode, PromptDelivery::Send)
        };
        let p = "\"$URUSHI_TASK_PROMPT\"";
        let claude = format!("claude {}", hooks(Agent::Claude));
        let codex = format!("codex {}", hooks(Agent::Codex));
        assert_eq!(
            line(Agent::Claude, false, AgentMode::Normal),
            format!("{claude} {p}")
        );
        assert_eq!(
            line(Agent::Claude, true, AgentMode::Normal),
            format!("{claude} --dangerously-skip-permissions {p}")
        );
        assert_eq!(
            line(Agent::Claude, false, AgentMode::Plan),
            format!("{claude} --permission-mode plan {p}")
        );
        assert_eq!(
            line(Agent::Claude, true, AgentMode::Plan),
            format!("{claude} --permission-mode plan --allow-dangerously-skip-permissions {p}")
        );
        assert_eq!(
            line(Agent::Cursor, true, AgentMode::Plan),
            format!("cursor-agent --plan --force {p}")
        );
        assert_eq!(
            line(Agent::Codex, true, AgentMode::Plan),
            format!("{codex} --dangerously-bypass-approvals-and-sandbox {p}")
        );
        assert_eq!(
            Agent::Cursor.task_command_line(
                "/a b/cursor-agent",
                false,
                AgentMode::Normal,
                PromptDelivery::Send
            ),
            format!("'/a b/cursor-agent' {p}")
        );
    }

    #[test]
    fn a_drafted_message_is_left_off_the_command_line() {
        let draft = |a: Agent, skip, mode| {
            a.task_command_line(a.program(), skip, mode, PromptDelivery::Draft)
        };
        let claude = format!("claude {}", hooks(Agent::Claude));
        assert_eq!(
            draft(Agent::Claude, true, AgentMode::Plan),
            format!("{claude} --permission-mode plan --allow-dangerously-skip-permissions")
        );
        assert_eq!(
            draft(Agent::Cursor, false, AgentMode::Plan),
            "cursor-agent --plan"
        );
        assert!(
            Agent::ALL
                .iter()
                .all(|a| !draft(*a, false, AgentMode::Normal).contains(TASK_PROMPT_ENV))
        );
    }

    #[test]
    fn deliveries_serialize_as_names_and_send_by_default() {
        assert_eq!(PromptDelivery::default(), PromptDelivery::Send);
        assert_eq!(
            serde_json::to_string(&PromptDelivery::Draft).unwrap(),
            r#""draft""#
        );
        assert_eq!(PromptDelivery::drafting(true), PromptDelivery::Draft);
        assert!(!PromptDelivery::drafting(false).is_draft());
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
