# 0017. Learn when an agent finished or waits from what it writes to its pane's terminal

- **Status:** Accepted
- **Date:** 2026-11-09

## Context
Users run several agents at once, in different threads. They want to know, without
watching each pane, when an agent **finished its response** and when it **waits for
them** (a permission prompt or a question). They want a dot on the thread in the sidebar,
and a system notification when Urushi is in the background.

The signal has to come from the agent. Screen-scraping its TUI is fragile and differs per
agent and version. Agents do offer hooks: Claude Code has `Stop` and `Notification` hooks,
which can be loaded per run with `--settings <json>`. Codex runs a `notify` program after
each turn, and its TUI can post OSC 9 notifications for approval requests. Urushi starts
agents by typing their command line into the pane's shell (ADR 0009), so it can add
options there.

A hook then has to reach the daemon. We measured how Claude runs hooks: in a new session
with no controlling terminal, so `/dev/tty` fails ("Device not configured"). A hook can
still write to the pane's terminal device by its path (`/dev/ttys004`).

## Decision
- **Signal = an escape sequence in the pane's output.** Urushi's signal is a private
  `OSC 777 ; urushi ; done|waiting` (`urushi_core::attention`). The daemon's emulator
  watches output for it, and also treats the bell and other programs' notifications
  (`OSC 9 ; text`, `OSC 777 ; notify ; …`) as "waiting" (`urushi-mux`
  `emulator/notices.rs`). That gives one path for hooked agents and any other program.
- **Hooks write the signal to `$URUSHI_TTY`**, the path of the pane's terminal device.
  The mux puts the path in the pane's environment (`SpawnSpec.tty_env`).
- **The hook configuration travels in the pane's environment**, as the task prompt does
  (ADR 0014): `URUSHI_CLAUDE_SETTINGS` and `URUSHI_CODEX_NOTIFY`. Every agent command
  line Urushi types starts with options that refer to them (`Agent::hook_args`):
  - Claude: `--settings "$URUSHI_CLAUDE_SETTINGS"`. It adds a `Stop` hook (done), and a
    `Notification` hook for `permission_prompt|elicitation_dialog` (waiting). The
    reminder Claude posts after a minute idle is left out, because the agent is still
    just done.
  - Codex: `-c "$URUSHI_CODEX_NOTIFY"` (`notify` runs the signal: done), plus
    `tui.notifications=["approval-requested"]`, `notification_method=osc9` and
    `notification_condition=always` (approval requests become OSC 9: waiting).
  - Cursor has no command-line hooks. Only its bells count.
- **Attention lives in the daemon, in memory only** (`service/attention.rs`). It is per
  pane, with a time, and reaches every client on each tab of `WorkspaceChanged`
  (`Tab.attention`, protocol 1.9). It isn't stored and doesn't bump the revision. It is
  cleared when the user types or clicks in the pane, or when a client says the user is
  looking at it (`ClearAttention`).
- **Clients decide what the user saw.** The app clears panes that are on screen while its
  window has focus. It posts a system notification for a new raise only while the window
  is in the background, using `notify-rust`.

## Alternatives considered
- **Hooks run the `urushi` CLI, which tells the daemon over the socket:** the hook needs
  the CLI's path and the right daemon home. A pane of the real daemon would then have to
  carry `URUSHI_HOME`, and that would point debug builds run in it (`cargo test`) at the
  real workspace, breaking the dev-home safety rule. It is also a second channel next to
  the terminal.
- **Hooks write to `/dev/tty`:** fails, because hooks have no controlling terminal
  (measured with Claude Code 2.1).
- **Detect "idle" from output going quiet:** can't tell done from waiting, and misfires
  on slow tools and spinners.
- **Install hooks in `~/.claude/settings.json` and `~/.codex/config.toml`:** would cover
  agents the user types themselves, but it edits personal config. The user chose not to.
- **Persist attention in the workspace document:** it's transient. Storing it would turn
  every bell into a disk write and a revision bump (and a conflict for edits based on the
  old revision).
- **Post notifications from the daemon:** the daemon may be remote or headless, and only
  the client knows whether the user is looking.
- **Shell out for notifications (`osascript`, `notify-send`):** no dependency, but on
  macOS the notification comes from Script Editor, and clicking it opens Script Editor.

## Consequences
- Agents started by Urushi (launcher, task start, restore) report exactly. Agents the
  user types by hand only report through bells and notifications, unless the user adds
  `--settings "$URUSHI_CLAUDE_SETTINGS"` themselves. The variables are in every pane.
- Typed command lines are longer (Codex's especially) and show up in shell history.
- Urushi overrides Codex's `notify` and `tui.notifications` for the agents it starts.
- Any program can raise attention by printing the signal or ringing the bell. Printing a
  file that contains the sequence does too. That's harmless: a dot, cleared on sight.
- Windows has no device path for ConPTY, so hooks can't signal there yet. Bells and OSC
  notifications still work.
- New dependency: `notify-rust` (`mac-notification-sys` on macOS, pure-Rust `zbus` on
  Linux, WinRT toasts on Windows). On macOS, notifications from the installed app carry
  its bundle identity. The bare dev binary's notifications use the system default.
