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
//! - Claude: `--settings "$URUSHI_CLAUDE_SETTINGS"`, adding a `Stop` hook (done) and a
//!   `Notification` hook for permission prompts and questions (waiting).
//! - Codex: its `notify` program on turn completion (done), and its own terminal
//!   notifications (OSC 9) for approval requests (waiting).
//! - Cursor has no command-line hooks; bells and notifications still count.

use std::fmt::Write as _;

use super::Agent;
use crate::attention::{AttentionKind, SIGNAL_NAMESPACE, SIGNAL_OSC};

/// Environment variable holding the path of the pane's terminal device.
pub const TTY_ENV: &str = "URUSHI_TTY";
/// Environment variable holding Claude's extra settings (JSON) with Urushi's hooks.
pub const CLAUDE_SETTINGS_ENV: &str = "URUSHI_CLAUDE_SETTINGS";
/// Environment variable holding Codex's `notify` override (`notify=[…]`, TOML).
pub const CODEX_NOTIFY_ENV: &str = "URUSHI_CODEX_NOTIFY";

/// A shell command that raises `kind` on the pane's terminal. It does nothing outside
/// an Urushi pane, and always succeeds, so a hook never shows up as an error.
pub fn signal_command(kind: AttentionKind) -> String {
    format!(
        "[ -w \"${TTY_ENV}\" ] && printf '\\033]{SIGNAL_OSC};{SIGNAL_NAMESPACE};{}\\007' > \"${TTY_ENV}\"; true",
        kind.signal()
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
    let hook = |kind| {
        format!(
            r#"[{{"type":"command","command":{}}}]"#,
            json_string(&signal_command(kind))
        )
    };
    format!(
        r#"{{"hooks":{{"Stop":[{{"hooks":{}}}],"Notification":[{{"matcher":"permission_prompt|elicitation_dialog","hooks":{}}}]}}}}"#,
        hook(AttentionKind::Done),
        hook(AttentionKind::Waiting)
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
    fn claude_gets_stop_and_permission_hooks_as_json() {
        let settings: serde_json::Value = serde_json::from_str(&claude_settings()).unwrap();
        let command = |event: &str| {
            settings["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(command("Stop"), signal_command(AttentionKind::Done));
        assert_eq!(
            command("Notification"),
            signal_command(AttentionKind::Waiting)
        );
        // Not the reminder Claude posts after a minute idle: that's still "done".
        assert_eq!(
            settings["hooks"]["Notification"][0]["matcher"],
            "permission_prompt|elicitation_dialog"
        );
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
