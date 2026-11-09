# 0009. Start agent CLIs by typing their command into the pane's shell

- **Status:** Accepted
- **Date:** 2026-11-07

## Context
A new tab can start a coding-agent CLI (Claude Code, Codex, Cursor) instead of a bare
shell. Pickers must only offer agents that are installed, and the user can opt in, per
agent, to its "skip permission prompts" flag. Agents live on the daemon's machine, which
may not be the client's (mobile, remote over Tailscale). A daemon started by the GUI on
macOS inherits launchd's minimal `PATH`, while agents are usually installed in
directories the user's shell profile adds (`~/.local/bin`, Homebrew, nvm).

## Decision
- **Detection is the daemon's job** (`urushi-daemon::agents::AgentLocator`, `ListAgents`).
  It searches the `PATH` of the user's interactive login shell (probed once a minute with
  `$SHELL -l -i -c env`, 5 s timeout), then the daemon's own `PATH`, then well-known
  install directories. An agent found only in one of the extra directories is started by
  its full path.
- **Starting an agent = a normal pane whose shell gets the command typed in.** The daemon
  spawns the usual shell, then writes `<command line>\r` to the PTY
  (`Service::type_command`). The terminal buffers it until the shell reads input.
- The command line, including the skip-permissions flag, comes from
  `urushi_core::Agent::command_line` and the daemon's `Settings.agents`. Clients only name
  the agent (`CreateTabRequest.agent`).

## Alternatives considered
- **Spawn the agent as the pane's process** (`program = claude`): the pane closes as soon
  as the agent exits, it doesn't get the profile's environment (API keys, nvm, `PATH`),
  and restoring panes after a daemon restart would relaunch the agent unasked.
- **`$SHELL -l -i -c 'claude; exec $SHELL -l'`**: keeps a shell afterwards, but needs
  per-shell quoting rules (fish, nushell, PowerShell) and loses the command from history.
- **Detect agents in the client**: wrong machine for remote and mobile clients, and it
  would break the "daemon is the source of truth" rule (ADR 0002).

## Consequences
- The shell remains when the agent exits, the command is in the shell's history, and the
  agent runs with exactly the environment the user's terminals have.
- Startup output that eats typeahead (e.g. a profile prompting "update now? [Y/n]") can
  swallow the command. The user can still type it themselves.
- A daemon restart restores the pane as a plain shell, not the agent.
- The first `ListAgents` after a minute runs the user's login shell, which costs up to a
  few seconds with heavy profiles; it runs off the async runtime.
