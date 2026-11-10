# 0033. Treat Claude as finished only when no background agent of its own still runs

- **Status:** Accepted
- **Date:** 2026-11-10

## Context
ADR 0017 raises "finished" from Claude's `Stop` hook. Claude Code 2.1 runs subagents in
the background by default. The main agent ends its turn right after it starts them, so
`Stop` fires. Each subagent that reports back wakes the main agent for a short turn
("waiting for the others"), and `Stop` fires again. Users saw "finished" while the work
was still running (UR-46). The daemon ignores a repeated `done`, so when the response
really ended, nothing new was announced.

Subagents run inside the Claude process, and Urushi doesn't hook `SubagentStop`. So
every false signal comes from the main agent's own `Stop`. Claude's `Stop` hook input
(JSON on stdin) has `background_tasks`, which lists the running and pending background
tasks. Each task has a `type`: `subagent`, `workflow`, `teammate`, `cloud session`,
`shell`, `monitor` or `MCP task`.

## Decision
- Claude's `Stop` hook (`agent::hooks::claude_done_command`) raises `done` unless its
  input lists a running background **agent**. That means a task whose type is in
  `CLAUDE_AGENT_TASK_TYPES`: `subagent`, `workflow`, `teammate` or `cloud session`.
- Shells, monitors and MCP tasks don't hold it back. A dev server or a watch left running
  is not unfinished work, and counting it would mean "finished" never shows.
- The check is a `grep -E` on the hook input, inside the hook command. The input is
  JSON, so a quote in a task's description is escaped and can't match `"type":"…"`.
- If the input doesn't list background tasks (older Claude), or if `grep` is missing,
  `done` is raised as before.

## Alternatives considered
- **Count every background task:** a long-running shell or monitor would keep the agent
  from ever being shown as finished.
- **Decide in the daemon** (debounce `done`, or wait for output to go quiet): the
  daemon can't see Claude's tasks, and timing can't tell "waiting for agents" from
  "done" (see ADR 0017 on idle detection).
- **Parse the input with `jq`:** more exact, but `jq` isn't installed everywhere, and a
  missing `jq` would silently lose the signal.
- **Re-announce a repeated `done`:** this would make the real end visible, but every
  early `done` would still notify the user. The early ones are the bug.
- **Use `SubagentStop`:** it fires when each subagent ends, which is the event that
  must *not* mean finished.

## Consequences
- Claude agents started by Urushi show "finished" once, when the main agent ends its
  last turn with no background agent left.
- The list of agent types follows Claude's task names. If Claude adds a new kind of
  background agent, `CLAUDE_AGENT_TASK_TYPES` needs it. Until then, the new kind
  behaves as before this change.
- A subagent waiting on its own background shell isn't listed as running (only its
  shell is, unattributed), so "finished" still shows early then. This was measured with
  Claude 2.1.322, and the feature doc tracks it.
- Codex isn't filtered. Its `notify` runs after each turn, as before.
- Panes get the new hook settings from the daemon (`hook_env`), so the fix takes effect
  once the daemon has been restarted.
