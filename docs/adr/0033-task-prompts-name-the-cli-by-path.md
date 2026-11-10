# 0033. Name the CLI by its absolute path in task prompts

- **Status:** Accepted
- **Date:** 2026-11-10
- **Amends:** [0024](0024-task-prompts-by-reference.md) (how the prompt names the CLI;
  the pane still gets the CLI's directory on `PATH`)

## Context
ADR 0024 tells a task's agent to run `urushi board show <KEY>`, and relies on the
daemon putting the CLI's directory first on the pane's `PATH`. In practice the pane's
shell rebuilds `PATH`: on macOS a login shell runs `path_helper`, and users' rc files
prepend their own directories, so the CLI's directory ends up near the end. Any other
`urushi` before it wins: an older install, a `cargo install`, a release CLI in a dev
daemon's pane, or, on a case-insensitive disk, the app's own `Urushi` GUI binary when
`Urushi.app/Contents/MacOS` is on `PATH`. UR-32 hit the last one. The agent got the
GUI's usage error and couldn't read its task.

## Decision
- **The prompt names the CLI by its absolute path**, quoted for a POSIX shell when it
  has to be (`urushi_core::shell::quote`): e.g. `run
  /Applications/Urushi.app/Contents/Resources/bin/urushi board show UR-32`. The
  user's own message fills `{show}` the same way. `task_reference_prompt` and
  `custom_task_prompt` take the CLI as a value, so `urushi-core` does no IO. Without a
  CLI beside the daemon, `{show}` falls back to `urushi`.
- **Panes get `URUSHI_CLI`**, the CLI's absolute path, for scripts and agents that need
  this daemon's CLI later. `URUSHI_HOME`, and the CLI's directory first on `PATH`, stay
  as before. They are a convenience for people typing `urushi`, not a guarantee.
- `cli::locate` names the bundle's CLI without `..`
  (`Contents/Resources/bin/urushi`, not `Contents/MacOS/../Resources/bin/urushi`), as it
  shows up in prompts.

## Alternatives considered
- **Keep `urushi board show` and add a fallback hint** ("if `urushi` isn't Urushi's CLI,
  run `"$URUSHI_CLI"`"): the agent may first run the wrong program, and possibly a
  different version against this daemon, before it notices.
- **Only `URUSHI_CLI` and cleaner `PATH`s**, prompt unchanged: the daemon can't
  control what the user's shell does to `PATH` after it starts, so a bare `urushi`
  stays unreliable.
- **Prefix the typed agent command with `PATH=…`**: shell-specific (fish, PowerShell),
  shows up in the terminal, and agents' own shells (e.g. Claude Code's shell snapshot)
  may rebuild `PATH` again.

## Consequences
- The task's command always runs this daemon's CLI, whatever the shell does to `PATH`.
- Prompts are longer, and a dev daemon's prompt shows a path under `target/debug`.
- Agent permission allowlists written for `urushi board show` don't match the
  absolute-path command, so the user may get one more permission prompt.
- `tests/task_cli.rs` runs the task's command in a pane whose shell puts a decoy
  `urushi` first on `PATH`.
