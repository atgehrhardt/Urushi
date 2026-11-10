# 0039. Learn that an agent is running from its prompt and tool hooks, and end it on Stop, prompts, session end or an interrupt key

- **Status:** Accepted
- **Date:** 2026-11-10
- **Builds on:** [0017](0017-agent-attention-through-the-pane-terminal.md) (the signal path)

## Context
UR-47 asks for an indicator on thread cards and task cards while an agent is **working**:
it is doing something, such as answering, running tools or running subagents. An agent
that is merely open and waiting for a prompt doesn't count.

Attention (ADR 0017) already says when an agent finished or waits. Its signal goes from
the agent's hooks, through `$URUSHI_TTY`, to the daemon's emulator. "Running" is the
stretch between a prompt and the end of that turn.

Claude Code has hooks for that stretch:
- `UserPromptSubmit`
- `PreToolUse` and `PostToolUse`, which fire for subagents' tools too
- `SubagentStart`
- `Stop`
- `SessionEnd`

Claude runs **no** hook when the user interrupts it (Esc or Ctrl+C). A background
subagent can keep working after the main turn's `Stop`.

Codex reports only the end of a turn (`notify`). It runs other hooks only after the user
trusts them. Cursor has no command-line hooks.

## Decision
- **Two more signal words** on the same OSC 777 sequence: `running` and `idle`
  (`urushi_core::attention`). Neither is attention.
- **Claude's hook settings** (`agent::hooks::claude_settings`):
  - `UserPromptSubmit`, `PreToolUse`, `PostToolUse` and `SubagentStart` write `running`.
  - `SessionEnd` writes `idle`.
  - The tool hooks act as a heartbeat: anything working in the pane, including a
    subagent after the main `Stop` or a tool after a permission prompt, keeps saying so.
- **The mux** counts the signals and keeps the latest (`SessionStatus.{activity, running}`).
- **The daemon** keeps running panes in memory with their start time
  (`service/running.rs`). A pane stops running when:
  - any attention is raised (done or waiting), or
  - `idle` arrives, or
  - the user sends **Esc or Ctrl+C** to the pane (`service::is_interrupt`), or
  - the pane is gone.

  Ordinary typing doesn't stop it, because messages typed while Claude works are queued.
- **The wire** carries running panes as `Tab.running` and `QuickTerminal.running`
  (`PaneRunning{pane_id, since_ms}`, protocol 1.25). Like attention, they aren't stored and
  don't bump the revision.
- **The app** shows one indicator per thread, tab, card or subtask row, by priority:
  needs input > running > finished. The running dot is blue and pulses. The runner draws
  about 12 frames a second for it, only while something runs and the window isn't
  occluded. Running doesn't color a card's border and is never announced.
- **Claude only.** Codex and Cursor panes never show running.

## Alternatives considered
- **Scrape the agent's spinner from the screen:** it differs per agent, version and
  language, and breaks silently.
- **Treat output activity as running:** spinners and timers redraw while an agent waits
  for permission. A long, quiet tool call looks idle.
- **Claude's OSC 9;4 progress sequences:** Claude only emits them for terminals it
  recognises from the environment. That would mean impersonating another terminal.
- **Count `SubagentStart` against `SubagentStop`:** the count drifts when a subagent is
  interrupted, which leaves a pane running forever. The heartbeat corrects itself
  instead.
- **Time running out after a quiet spell:** a ten-minute build is one tool call with no
  hooks in between, so it would be shown as stopped while it still runs.
- **Codex from Enter until `notify`:** Enter also answers Codex's menus and dialogs. A
  "running" started that way stays on until a turn ends, which may never happen. The
  user chose Claude only.

## Consequences
- Each Claude tool call runs two more tiny hooks (`printf` to the terminal device).
- An agent that crashes without running `SessionEnd` stays "running" until the pane's
  next signal, an interrupt key, or the pane closing.
- Esc pressed for something other than an interrupt, such as closing a Claude menu
  mid-turn, briefly clears running. The next tool call sets it again.
- While anything runs, the app redraws a few times a second instead of only on change.
- Agents typed by hand get the indicator only with
  `--settings "$URUSHI_CLAUDE_SETTINGS"`, as for attention.
