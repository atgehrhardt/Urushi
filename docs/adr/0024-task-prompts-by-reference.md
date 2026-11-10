# 0024. Start a task's agent with a reference to the task and the CLI to read it, not a copy

- **Status:** Accepted
- **Date:** 2026-11-09
- **Amends:** [0014](0014-task-prompt-via-pane-environment.md) (what the prompt says;
  how it travels is unchanged)

## Context
ADR 0014 starts a task's agent with the task pasted in as its first prompt: title, notes
and checklist. For a Jira issue that copy leaves out what matters most (comments, links,
the fields, files) and goes stale as the issue changes. Users asked that threads get "the
link with instructions to review the task" instead, for Jira and for Urushi's own boards
(UR-10).

Agents run in panes the daemon starts, on the same machine. The `urushi` CLI ships next
to the daemon (beside it in a build, in `Urushi.app/Contents/Resources/bin` on macOS),
but isn't necessarily on the user's `PATH`, and the dev and real daemons live in
different homes.

## Decision
- **Every pane can reach this daemon's CLI.** Its environment has `URUSHI_HOME` (the
  daemon's home) and, when the daemon finds the CLI beside it, the CLI's directory first
  on `PATH`.
- **The prompt refers to the task.** With the CLI found, `task_reference_prompt` names
  the task (a Jira issue by key, with its address), the task it is part of, and says to
  review it first with `urushi board show <KEY or id>`, and, for Jira, to ask before
  changing it in Jira. Without the CLI, the copy of ADR 0014 is still used.
- **`urushi board show`** prints a task in full: where it is, its notes, subtasks and
  attachments, and for a Jira issue its fields, comments, links and files, read from
  Jira at that moment (protocol 1.13).

## Alternatives considered
- **Paste the Jira issue in full** (fields, comments, links) into the prompt: long, stale
  as soon as someone comments, and capped by `MAX_PROMPT_BYTES`.
- **Only the issue's address.** Agents can't read Jira without a sign-in; the daemon
  has it.
- **An MCP server for boards.** Needs per-agent configuration; a CLI on `PATH` works with
  every agent that can run commands.

## Consequences
- Agents read the task as it is when they start, and can read it again (or a linked
  issue) later.
- Reviewing costs the agent a command (and, in plan mode, maybe a permission prompt).
- Every pane's `PATH` starts with the CLI's directory, and programs in it can see
  `URUSHI_HOME`.
- A daemon whose CLI isn't beside it (an unusual install) falls back to the copy.
