# 0014. Start a task's agent with the task as its prompt, passed through the pane's environment

- **Status:** Accepted; what the prompt says is amended by [0024](0024-task-prompts-by-reference.md)
- **Date:** 2026-11-09

## Context
**Start thread** on a task used to open a thread on the agent launcher. The user then
picked an agent by hand and retyped what the task said: its notes and subtasks never
reached the agent. Users want a thread started from a task to run their usual agent
right away, in their usual mode (e.g. Claude in plan mode), already knowing the task.

Agents are started by typing their command line into the pane's shell (ADR 0009). The
task can be long: up to 10 000 characters of notes on several lines, with quotes and
`$`, plus up to 100 subtasks. All three agents take the first prompt as a positional
argument.

## Decision
- The **choice lives in the daemon's settings**: `AgentSettings.task_agent:
  Option<Agent>` and `task_mode: AgentMode {Normal, Plan}`. The app asks for it once,
  with a first-run setup wizard, and Settings can change it later. The mode applies only
  to task starts. The + launcher still starts agents normally.
- `CreateThread{task_id, subtask_id, start_task_agent: true}` makes the **daemon**
  (`service/task_agent.rs`) build the prompt from the board (`urushi_core::task_prompt`)
  and resolve the agent **before** the worktree exists. A missing choice, a missing
  agent or a missing task leaves nothing behind.
- The first pane's shell is spawned with **`URUSHI_TASK_PROMPT=<prompt>`** in its
  environment. The daemon then types `Agent::task_command_line`, e.g.
  `claude --permission-mode plan "$URUSHI_TASK_PROMPT"`.
- Mode flags:
  - Claude: `--permission-mode plan`. With skip-permissions on, it gets
    `--allow-dangerously-skip-permissions` instead of `--dangerously-skip-permissions`,
    so it starts by planning and bypass stays one Shift+Tab away.
  - Cursor: `--plan`.
  - Codex has no plan mode, so it starts normally.

## Alternatives considered
- **Inline the quoted prompt in the typed line:** a multi-line single-quoted string relies
  on the shell's continuation prompt while it's being typed, puts the whole task into
  shell history, and depends on POSIX quoting.
- **Write the prompt to a file in the worktree:** it would show up in `git status` and
  could be committed. In plan mode, reading it costs the agent a tool call, and maybe a
  permission prompt.
- **Paste the prompt into the agent's TUI after it starts:** this needs to know when the
  agent is ready to read input, which differs per agent and version.
- **Let the client build the prompt and send it:** this duplicates board logic in every
  client, and the CLI and the GUI could drift (ADR 0002).

## Consequences
- The typed line is short, on one line, and the same for any task. It works in sh,
  bash, zsh and fish, and history shows `"$URUSHI_TASK_PROMPT"`, not the task.
- Prompts are capped at `MAX_PROMPT_BYTES` (64 KiB), well under the per-string limits on
  environment variables and arguments. Longer tasks are cut with a notice.
- The variable stays set in that pane's shell for its life. That's harmless, but
  programs started there can see it.
- PowerShell would need `$env:URUSHI_TASK_PROMPT`. Agent command lines already assume a
  POSIX-style shell (`shell::quote`).
- After a daemon restart, a restored agent pane resumes its conversation (ADR 0013), but
  plan mode isn't applied again. The agent continues in whatever mode it resumes in.
