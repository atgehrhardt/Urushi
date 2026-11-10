//! Hooks that make an agent tell Urushi when it finished or waits for the user (ADR
//! 0017).
//!
//! Agents run their hooks detached from the terminal (no `/dev/tty`), so a hook writes
//! Urushi's signal ([`AttentionKind::signal_sequence`]) straight to the pane's terminal
//! device, whose path the daemon puts in [`TTY_ENV`]. The daemon's emulator sees the
//! signal in the pane's output. The hook configuration is long, so like a task's prompt
//! (ADR 0014) it travels in the pane's environment and the typed command line only
//! refers to it:
//!
//! - Claude: `--settings "$URUSHI_CLAUDE_SETTINGS"`, adding a `Stop` hook (done,
//!   unless background agents still run: ADR 0035), a
//!   `Notification` hook for permission prompts and questions (waiting), and a
//!   `SessionStart` hook on startup (ready: it runs once Claude's own startup dialogs,
//!   such as folder trust, are answered, so a drafted task message can be typed in; ADR
//!   0030).
//! - Codex: its `notify` program on turn completion (done), and its own terminal
//!   notifications (OSC 9) for approval requests (waiting).
//! - Cursor has no command-line hooks; bells and notifications still count.

use std::fmt::Write as _;

use super::Agent;
use crate::attention::{AttentionKind, READY_SIGNAL, SIGNAL_NAMESPACE, SIGNAL_OSC};

/// Environment variable holding the path of the pane's terminal device.
pub const TTY_ENV: &str = "URUSHI_TTY";
/// Environment variable holding Claude's extra settings (JSON) with Urushi's hooks.
pub const CLAUDE_SETTINGS_ENV: &str = "URUSHI_CLAUDE_SETTINGS";
/// Environment variable holding Codex's `notify` override (`notify=[…]`, TOML).
pub const CODEX_NOTIFY_ENV: &str = "URUSHI_CODEX_NOTIFY";

/// A shell command that raises `kind` on the pane's terminal. It does nothing outside
/// an Urushi pane, and always succeeds, so a hook never shows up as an error.
pub fn signal_command(kind: AttentionKind) -> String {
    word_command(kind.signal())
}

/// The kinds of Claude background task (the `type` of a `background_tasks` entry in
/// its `Stop` hook input) that are agents still working on the user's request. Claude
/// ends its turn while they run and again after each reports back, so a `Stop` then
/// isn't the end of the response (ADR 0035). Shells, monitors and MCP tasks aren't
/// here: a server left running must not keep the agent from ever being done.
pub const CLAUDE_AGENT_TASK_TYPES: &[&str] = &["subagent", "workflow", "teammate", "cloud session"];

/// The command for Claude's `Stop` hook: raises `done` unless the hook input on stdin
/// lists a running background agent ([`CLAUDE_AGENT_TASK_TYPES`]). Claude lists only
/// running and pending tasks there; versions that don't list them get `done` as before.
/// Like [`signal_command`], harmless outside an Urushi pane and always succeeds.
pub fn claude_done_command() -> String {
    // Hook input is JSON, so a quote inside a description is escaped (`\"type\"`) and
    // can't pass for a task's own `"type"`.
    let pattern = format!(
        r#""type"[[:space:]]*:[[:space:]]*"({})""#,
        CLAUDE_AGENT_TASK_TYPES.join("|")
    );
    format!(
        "grep -Eq '{pattern}' || {{ {}; }}; true",
        signal_command(AttentionKind::Done)
    )
}

/// A shell command that tells the pane's terminal the agent reads input now
/// ([`READY_SIGNAL`]). Like [`signal_command`], harmless outside an Urushi pane.
pub fn ready_command() -> String {
    word_command(READY_SIGNAL)
}

/// A shell command writing Urushi's signal with `word` to the pane's terminal.
fn word_command(word: &str) -> String {
    format!(
        "[ -w \"${TTY_ENV}\" ] && printf '\\033]{SIGNAL_OSC};{SIGNAL_NAMESPACE};{word}\\007' > \"${TTY_ENV}\"; true"
    )
}

/// The variables every pane gets so that agents started in it can run Urushi's hooks.
pub fn hook_env() -> Vec<(String, String)> {
    vec![
        (CLAUDE_SETTINGS_ENV.into(), claude_settings()),
        (CODEX_NOTIFY_ENV.into(), codex_notify()),
    ]
}

impl Agent {
    /// Whether Urushi's hooks make this agent say when it reads input (the `ready`
    /// signal, from Claude's `SessionStart` hook). Codex only runs hooks the user has
    /// trusted, and Cursor has none on its command line.
    pub const fn says_when_ready(self) -> bool {
        matches!(self, Self::Claude)
    }

    /// Options that load Urushi's hooks, right after the program name (before any
    /// subcommand). They refer to [`hook_env`]'s variables.
    pub const fn hook_args(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["--settings", "\"$URUSHI_CLAUDE_SETTINGS\""],
            Self::Codex => &[
                "-c",
                "\"$URUSHI_CODEX_NOTIFY\"",
                "-c",
                "'tui.notifications=[\"approval-requested\"]'",
                "-c",
                "tui.notification_method=osc9",
                "-c",
                "tui.notification_condition=always",
            ],
            Self::Cursor => &[],
        }
    }
}

/// Claude settings adding Urushi's hooks. Claude merges them with the user's own.
fn claude_settings() -> String {
    let hook = |command: String| {
        format!(
            r#"[{{"type":"command","command":{}}}]"#,
            json_string(&command)
        )
    };
    format!(
        r#"{{"hooks":{{"Stop":[{{"hooks":{}}}],"Notification":[{{"matcher":"permission_prompt|elicitation_dialog","hooks":{}}}],"SessionStart":[{{"matcher":"startup","hooks":{}}}]}}}}"#,
        hook(claude_done_command()),
        hook(signal_command(AttentionKind::Waiting)),
        hook(ready_command())
    )
}

/// Codex's `notify` setting. Codex runs it with a JSON argument after each turn; as
/// `sh -c`'s `$0` it is ignored.
fn codex_notify() -> String {
    format!(
        "notify=[\"sh\", \"-c\", {}]",
        toml_string(&signal_command(AttentionKind::Done))
    )
}

/// `s` as a JSON string literal.
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `s` as a TOML basic string. Same escapes as JSON for what we write.
fn toml_string(s: &str) -> String {
    json_string(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signal_command_writes_the_signal_to_the_pane_terminal() {
        assert_eq!(
            signal_command(AttentionKind::Done),
            r#"[ -w "$URUSHI_TTY" ] && printf '\033]777;urushi;done\007' > "$URUSHI_TTY"; true"#
        );
    }

    #[test]
    fn claudes_done_command_skips_the_signal_while_background_agents_run() {
        assert_eq!(
            claude_done_command(),
            r#"grep -Eq '"type"[[:space:]]*:[[:space:]]*"(subagent|workflow|teammate|cloud session)"' || { [ -w "$URUSHI_TTY" ] && printf '\033]777;urushi;done\007' > "$URUSHI_TTY"; true; }; true"#
        );
    }

    #[test]
    fn the_ready_command_writes_the_ready_signal() {
        assert_eq!(
            ready_command(),
            r#"[ -w "$URUSHI_TTY" ] && printf '\033]777;urushi;ready\007' > "$URUSHI_TTY"; true"#
        );
    }

    #[test]
    fn claude_gets_stop_and_permission_hooks_as_json() {
        let settings: serde_json::Value = serde_json::from_str(&claude_settings()).unwrap();
        let command = |event: &str| {
            settings["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(command("Stop"), claude_done_command());
        assert_eq!(
            command("Notification"),
            signal_command(AttentionKind::Waiting)
        );
        // Not the reminder Claude posts after a minute idle: that's still "done".
        assert_eq!(
            settings["hooks"]["Notification"][0]["matcher"],
            "permission_prompt|elicitation_dialog"
        );
        // Ready once it has started, not again on /clear, resume or compaction.
        assert_eq!(command("SessionStart"), ready_command());
        assert_eq!(settings["hooks"]["SessionStart"][0]["matcher"], "startup");
    }

    #[test]
    fn codex_notify_is_a_toml_array_running_the_signal() {
        let notify = codex_notify();
        assert!(notify.starts_with("notify=[\"sh\", \"-c\", \""), "{notify}");
        // The TOML string decodes (same escapes as JSON) to the signal command.
        let value = notify.trim_start_matches("notify=");
        let array: Vec<String> = serde_json::from_str(value).unwrap();
        assert_eq!(array[2], signal_command(AttentionKind::Done));
    }

    #[test]
    fn hook_options_refer_to_the_pane_environment() {
        let env = hook_env();
        assert!(env.iter().any(|(k, _)| k == CLAUDE_SETTINGS_ENV));
        assert!(env.iter().any(|(k, _)| k == CODEX_NOTIFY_ENV));
        assert_eq!(
            Agent::Claude.hook_args().join(" "),
            "--settings \"$URUSHI_CLAUDE_SETTINGS\""
        );
        assert!(
            Agent::Codex
                .hook_args()
                .contains(&"\"$URUSHI_CODEX_NOTIFY\"")
        );
        assert!(Agent::Cursor.hook_args().is_empty());
    }
}
