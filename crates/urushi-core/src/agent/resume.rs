//! Resuming an agent's conversation in a pane restored after a daemon restart.
//!
//! The process is gone after a restart, but each agent CLI keeps its conversations on
//! disk and can reopen one. The daemon notes which conversation a pane's agent had open
//! (an [`AgentSessionId`]) and, when it restores the pane, types the command that
//! resumes it. Without an id, the agent reopens its most recent conversation in the
//! pane's directory.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::Agent;

/// Longest session id accepted.
const MAX_LEN: usize = 128;

/// The id of one of an agent's conversations (Claude's session id, Codex's thread id,
/// Cursor's chat id). It is typed into a shell, so only letters, digits, `-` and `_` are
/// allowed: an id read from a file can't smuggle shell syntax in.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AgentSessionId(String);

/// A string that isn't a usable [`AgentSessionId`].
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("not an agent session id: {0:?}")]
pub struct InvalidSessionId(pub String);

impl AgentSessionId {
    /// Checks `id`: 1 to 128 ASCII letters, digits, `-` or `_`.
    pub fn new(id: impl Into<String>) -> Result<Self, InvalidSessionId> {
        let id = id.into();
        let ok = !id.is_empty()
            && id.len() <= MAX_LEN
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if ok {
            Ok(Self(id))
        } else {
            Err(InvalidSessionId(id))
        }
    }

    /// The id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AgentSessionId {
    type Error = InvalidSessionId;
    fn try_from(id: String) -> Result<Self, Self::Error> {
        Self::new(id)
    }
}

impl From<AgentSessionId> for String {
    fn from(id: AgentSessionId) -> Self {
        id.0
    }
}

impl fmt::Display for AgentSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Agent {
    /// The command line that reopens a conversation: `session`, or without one the most
    /// recent conversation in the current directory. `program` and `skip_permissions`
    /// are as for [`Agent::command_line`].
    pub fn resume_command_line(
        self,
        program: &str,
        skip_permissions: bool,
        session: Option<&AgentSessionId>,
    ) -> String {
        let mut words = self.program_words(program);
        // Codex resumes with a subcommand; its options come after it.
        if self == Self::Codex {
            words.push("resume".into());
        }
        if skip_permissions {
            words.push(self.skip_permissions_flag().into());
        }
        match (self, session) {
            (Self::Codex, Some(id)) => words.push(id.to_string()),
            (Self::Codex, None) => words.push("--last".into()),
            (_, Some(id)) => words.extend(["--resume".into(), id.to_string()]),
            (_, None) => words.push("--continue".into()),
        }
        words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::hooks;
    use super::*;

    #[test]
    fn ids_are_checked_before_they_reach_a_shell() {
        assert!(AgentSessionId::new("8b414ee4-fba5-400d-a854-f55a3f23d5ed").is_ok());
        assert!(AgentSessionId::new("chat_01").is_ok());
        for bad in ["", "a b", "x; rm -rf ~", "$(id)", "id\n", &"a".repeat(129)] {
            assert!(AgentSessionId::new(bad).is_err(), "{bad:?}");
        }
        let json = serde_json::to_string(&AgentSessionId::new("abc").unwrap()).unwrap();
        assert_eq!(json, "\"abc\"");
        assert!(serde_json::from_str::<AgentSessionId>("\"a;b\"").is_err());
    }

    #[test]
    fn each_agent_resumes_its_own_way() {
        let id = AgentSessionId::new("s-1").unwrap();
        let line = |a: Agent, skip, id| a.resume_command_line(a.program(), skip, id);
        let claude = format!("claude {}", hooks(Agent::Claude));
        // Codex's hook options are global, so they come before `resume`.
        let codex = format!("codex {}", hooks(Agent::Codex));
        assert_eq!(
            line(Agent::Claude, false, Some(&id)),
            format!("{claude} --resume s-1")
        );
        assert_eq!(
            line(Agent::Claude, true, None),
            format!("{claude} --dangerously-skip-permissions --continue")
        );
        assert_eq!(
            line(Agent::Codex, false, Some(&id)),
            format!("{codex} resume s-1")
        );
        assert_eq!(
            line(Agent::Codex, true, None),
            format!("{codex} resume --dangerously-bypass-approvals-and-sandbox --last")
        );
        assert_eq!(
            line(Agent::Cursor, true, Some(&id)),
            "cursor-agent --force --resume s-1"
        );
        assert_eq!(line(Agent::Cursor, false, None), "cursor-agent --continue");
        assert_eq!(
            Agent::Cursor.resume_command_line("/a b/cursor-agent", false, None),
            "'/a b/cursor-agent' --continue"
        );
    }
}
