# 0059. Count an agent's background shells and monitors from Claude's Stop hook, and show them beside its status

- **Status:** Accepted
- **Date:** 2026-11-20
- **Builds on:** [0039](0039-running-agents-from-their-hooks.md) (running), [0035](0035-claude-is-done-when-no-background-agent-runs.md) (background tasks)

## Context
UR-74: running (ADR 0039) only says whether an agent or subagent is working. A
shell that Claude left running in the background (a dev server, a `sleep` it waits for)
or a `Monitor` doesn't show. So once the agent finishes, nothing says that the thread
still has work alive. The user asked for thread cards in the sidebar to show those
shells and monitors next to the agent's status.

Claude Code 2.1.330 lists background tasks (`background_tasks`, each with a `type`) only
in the inputs of its `Stop` and `SubagentStop` hooks. ADR 0035 already reads that list
from `Stop`, to keep "finished" back while background agents run, and leaves shells and
monitors out. No hook runs when a background task starts or ends. What does happen is
this:
- A shell or monitor that ends wakes Claude for a turn, so its `Stop` runs again with
  the new list.
- Quitting Claude stops its tasks (it asks first) and runs `SessionEnd`.

## Decision
- **A second `Stop` hook** (`agent::hooks::claude_background_command`):
  - It counts the hook input's `"type":"shell"` and `"type":"monitor"` entries with
    `grep -Eo … | wc -l` and writes Urushi's signal with them:
    `OSC 777 ; urushi ; background ; <shells> ; <monitors>`.
  - It sits in its own matcher group, so Claude gives it the hook input too.
  - The same JSON-escaping argument as ADR 0035 applies: a description can't fake a type.
- **The mux** keeps the latest report (`SessionStatus.{background, background_reports}`).
  `idle` (`SessionEnd`) empties it. Alerts leave it alone.
- **The daemon** keeps each pane's counts in memory (`service/background.rs`).
  - It drops a pane when a report says nothing is open, when the session ends, and when
    the pane closes.
  - Esc, Ctrl+C and attention leave the counts as they are: the shells keep running.
  - It doesn't store them, and doesn't bump the revision.
- **The wire** (protocol 1.33): `Tab.background` and `QuickTerminal.background`
  (`PaneBackground{pane_id, shells, monitors}`). Only panes with something open are listed.
- **The app**:
  - A thread card's status line adds the thread's total in muted text, e.g.
    `running · 2 shells · 1 monitor`, or `1 shell` on its own when there's no agent status.
  - Open tasks don't pulse, and aren't attention.
  - `urushi ls` marks panes `[2 shells, 1 monitor]`.
- **Claude only**, like running.

## Alternatives considered
- **Count the pane's child processes:** the daemon can't tell a background shell Claude
  owns from the agent's own helpers, MCP servers or a tool call in the foreground. It
  would also mean polling the process table per pane.
- **Read `run_in_background` from `PreToolUse`/`PostToolUse` input:** this tells when a
  shell starts, but not when it ends, nor when a monitor stops. So the counts would only
  grow. `Stop` lists what is really open.
- **Count panes with a shell open:** every pane is a shell, so the count says nothing.
- **Also report from `SubagentStop`:** it does list background tasks, but every report
  overwrites the pane's counts. We haven't checked whether a subagent's list is the
  whole session's, and the main agent's `Stop` follows anyway.
- **Count shells and monitors as running (pulse):** a dev server left open isn't work in
  progress, and the window would redraw forever.

## Consequences
- A shell started mid-turn shows when that turn ends, not when it starts.
- A task stopped from Claude's task panel without waking Claude may keep showing until
  Claude's next turn or exit.
- An agent that crashes without `SessionEnd` keeps its counts until the pane's next
  report or the pane closes.
- If Claude renames its task types, the counts silently become 0. Then
  `CLAUDE_SHELL_TASK_TYPE` / `CLAUDE_MONITOR_TASK_TYPE` need the new names.
- Each `Stop` runs one more tiny hook. Panes get it from the daemon's `hook_env`, so it
  takes effect after a daemon restart, and only in panes started after that.
