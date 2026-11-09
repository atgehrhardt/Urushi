//! Which conversation an agent in a pane has open, for its restore point.
//!
//! Claude Code writes `<config dir>/sessions/<pid>.json` for each running instance,
//! naming the session it has open (it changes with `/clear` and `/resume`), so a pane's
//! Claude conversation is known exactly. Its transcript, `projects/<dir>/<id>.jsonl`,
//! only exists once something was said; a session without one comes back as a fresh
//! Claude. Codex and Cursor don't say, so their panes resume the latest conversation in
//! the pane's directory.

use std::path::Path;

use serde_json::Value;
use urushi_core::AgentSessionId;

use super::Conversation;

/// The conversation a Claude Code process has open (see the module docs).
pub(crate) fn claude_conversation(pid: u32) -> Conversation {
    match crate::usage::claude_dir() {
        Ok(dir) => claude_conversation_in(&dir, pid),
        Err(_) => Conversation::Unknown,
    }
}

/// [`claude_conversation`], with Claude's config directory given.
pub(crate) fn claude_conversation_in(dir: &Path, pid: u32) -> Conversation {
    let file = dir.join("sessions").join(format!("{pid}.json"));
    let Some(id) = std::fs::read_to_string(file)
        .ok()
        .and_then(|json| parse_claude_session(&json, pid))
    else {
        return Conversation::Unknown;
    };
    let transcript = format!("{id}.jsonl");
    let saved = std::fs::read_dir(dir.join("projects"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|project| project.path().join(&transcript).is_file());
    if saved {
        Conversation::Saved(id)
    } else {
        Conversation::Empty
    }
}

/// The `sessionId` of a Claude Code session file, if it belongs to `pid`.
pub(crate) fn parse_claude_session(json: &str, pid: u32) -> Option<AgentSessionId> {
    let value: Value = serde_json::from_str(json).ok()?;
    if value.get("pid").and_then(Value::as_u64) != Some(u64::from(pid)) {
        return None;
    }
    let id = value.get("sessionId")?.as_str()?;
    AgentSessionId::new(id).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_session_files_name_the_open_session() {
        let json = r#"{"pid":98847,"sessionId":"8b414ee4-fba5-400d-a854-f55a3f23d5ed",
                       "cwd":"/x","status":"busy"}"#;
        assert_eq!(
            parse_claude_session(json, 98847).unwrap().as_str(),
            "8b414ee4-fba5-400d-a854-f55a3f23d5ed"
        );
        assert_eq!(
            parse_claude_session(json, 1),
            None,
            "another process's file"
        );
        assert_eq!(
            parse_claude_session(r#"{"pid":1,"sessionId":"a; rm -rf ~"}"#, 1),
            None,
            "an id that isn't safe to type is ignored"
        );
        assert_eq!(parse_claude_session("{", 1), None);
    }

    #[test]
    fn claude_conversations_are_saved_once_something_was_said() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path();
        assert_eq!(claude_conversation_in(claude, 7), Conversation::Unknown);
        std::fs::create_dir_all(claude.join("sessions")).unwrap();
        std::fs::write(
            claude.join("sessions/7.json"),
            r#"{"pid":7,"sessionId":"abc-1"}"#,
        )
        .unwrap();
        assert_eq!(claude_conversation_in(claude, 7), Conversation::Empty);
        let project = claude.join("projects/-Users-me-x");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("abc-1.jsonl"), "{}\n").unwrap();
        assert_eq!(
            claude_conversation_in(claude, 7),
            Conversation::Saved(AgentSessionId::new("abc-1").unwrap())
        );
    }
}
