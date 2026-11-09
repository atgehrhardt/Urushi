# Feature: Agent launcher

- **Status:** Shipped
- **Owners (crates):** `urushi-core` (`agent`), `urushi-daemon` (`agents`, `service/agents.rs`, `service/tabs.rs`), `urushi-app` (`update/launcher.rs`, `view/launcher.rs`), `urushi-cli`
- **Related ADRs:** [0009](../adr/0009-agents-typed-into-a-shell.md), [0002](../adr/0002-daemon-source-of-truth.md)

## What it does
A new tab is chosen, not assumed: the **+** button and the new-tab shortcut open a picker
with **Terminal** plus every agent CLI installed on the daemon's machine (**Claude**,
**Codex**, **Cursor**). A new thread, and a thread whose last tab was closed, shows the
same choices inline. Agents start in normal (asking) mode; Settings has a per-agent switch
for the skip-permissions flag.

| Agent | Program | Skip-permissions flag |
|---|---|---|
| Claude | `claude` | `--dangerously-skip-permissions` |
| Codex | `codex` | `--dangerously-bypass-approvals-and-sandbox` |
| Cursor | `cursor-agent` | `--force` |

## How it works
- `ListAgents` → `Agents{InstalledAgent{agent, path}}`. The daemon's `AgentLocator`
  searches the login shell's `PATH`, its own, then common install dirs (ADR 0009).
  The app asks on connect, when opening the picker and after creating a thread, only if
  the daemon's protocol minor is ≥ 1 (`update/launcher.rs::AGENTS_SINCE_MINOR`).
- `CreateTab{agent}`: the daemon resolves the agent first (missing → `FAILED_PRECONDITION`,
  nothing opened), spawns the shell, types `Agent::command_line(program, skip)` + Enter,
  and titles the tab after the agent.
- The GUI creates threads with `CreateThread{skip_initial_tab: true}`, so they open on the
  launcher. The CLI keeps the old default (`--no-tab` to skip).
- Keys: in the picker, arrows move, Enter or a digit picks, Escape cancels. In a thread
  with no tabs, a digit picks and Enter opens a terminal.
- Settings: `Settings.agents` (`AgentSettings{skip_permissions}` in core;
  `*_skip_permissions` booleans on the wire and in `SettingsPatch`).

## Invariants
- Only installed agents are offered (`AppState::launch_choices`); Terminal is always offered.
- Skip-permissions is off unless the user turned it on for that agent
  (`AgentSettings::default`, `core::agent` tests; daemon `settings.rs` tests).
- An agent that isn't installed never leaves a tab or process behind
  (`tests/threads_and_agents.rs`).

## Tests
- `crates/urushi-core/src/agent.rs`: command lines, quoting, per-agent settings.
- `crates/urushi-daemon/src/agents.rs`: search order, executable detection, full path vs name.
- `crates/urushi-daemon/tests/threads_and_agents.rs`: a fake `claude` started with and
  without the flag, the shell surviving it, and a missing agent refused.
- `crates/urushi-app/src/update/tests/flows.rs` and `view/mod.rs`: picker, inline
  launcher, old daemons, settings toggles.
- Manually verified with real zsh and Claude Code against a throwaway daemon home.

## Limitations and follow-ups
- A profile that consumes typeahead at startup can swallow the typed command (ADR 0009).
- After a daemon restart an agent pane resumes its conversation (see [recoverable sessions](recoverable-sessions.md), ADR 0013); Codex and Cursor resume the latest one in the pane's directory.
- No per-repo agent settings, and no extra arguments per agent yet.
- Agents picked here always start in normal mode. Threads started from a task run the
  task agent in its own mode instead ([task agent](task-agent.md), ADR 0014).
