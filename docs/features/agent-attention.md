# Feature: Agent attention (finished / needs input)

- **Status:** Shipped
- **Owners (crates):** `urushi-core` (`attention`, `agent::hooks`), `urushi-mux` (`emulator/notices.rs`, `SessionStatus.alerts`, `SpawnSpec.tty_env`), `urushi-daemon` (`service/attention.rs`), `urushi-app` (`attention.rs`, `update/attention.rs`, `view/attention.rs`, `platform::notify`), `urushi-cli`
- **Related ADRs:** [0017](../adr/0017-agent-attention-through-the-pane-terminal.md), [0033](../adr/0033-claude-is-done-when-no-background-agent-runs.md), [0009](../adr/0009-agents-typed-into-a-shell.md), [0014](../adr/0014-task-prompt-via-pane-environment.md)

## What it does
When an agent in a pane **finishes its response**, its thread gets a green dot and
"finished" in the sidebar. A Claude agent that is still waiting for its own background
agents (subagents, workflows) is not finished yet. When it **waits for you** (a permission prompt, a question, or
any program ringing the bell), it gets an amber dot and "needs input". The same dot shows
on the tab, and task cards linked to the thread (directly or through a subtask at any
depth) get a **Finished** or **Needs input** badge and a border in that color. While Urushi is in the
background, each new one also posts a system notification, e.g. "Invoice API — Claude
finished". The dot goes away once you look at the pane (it's on screen and the window
has focus) or type into it.

## How it works
1. **Hooks.** Every pane's environment has `URUSHI_TTY` (its terminal device, set by the
   mux) and the hook configuration (`agent::hooks::hook_env`). Agent command lines start
   with `Agent::hook_args`:
   - Claude: `--settings "$URUSHI_CLAUDE_SETTINGS"`, with `Stop` → done
     (`claude_done_command`). It stays quiet while the hook input's `background_tasks`
     lists a running agent (`CLAUDE_AGENT_TASK_TYPES`: subagent, workflow, teammate,
     cloud session), because Claude also ends a turn while those run (ADR 0033).
     `Notification` (permission prompt, question) → waiting, and `SessionStart`
     (`startup`) → `ready` (`ready_command`). `ready` is **not attention**: it says
     Claude reads input, so a drafted task message can be pasted in
     ([task agent](task-agent.md), ADR 0030).
   - Codex: `notify` → done, and approval requests as OSC 9 → waiting.

   A hook runs `signal_command`, which writes `ESC ] 777 ; urushi ; done BEL` to
   `$URUSHI_TTY`.
2. **Detection.** The mux's `Notices` parser watches each pane's output for that signal,
   for `OSC 9`/`OSC 777;notify` (waiting) and, through alacritty, the bell (waiting).
   `SessionStatus.{alerts, alert}` reports the latest. `ready` signals only count up
   `SessionStatus.ready` (`EmulatorEvent::Ready`).
3. **Daemon.** `watch_session` calls `raise_attention(pane, kind)`. Attention is kept in
   memory per pane with its time. A repeat of the same kind is ignored. Each change wakes
   `WorkspaceChanged` subscribers without a new revision, and `workspace()` lists
   `Tab.attention[PaneAttention{pane_id, kind, since_ms}]` (and `QuickTerminal.attention`
   for the quick terminal's panes). Typing or clicking in a pane
   (`Input` key/text/paste/press, `Service::user_acted`, which also counts toward a task
   message waiting to be drafted there) and `ClearAttention{pane_ids}` clear it. Closed panes
   are forgotten.
4. **App.**
   - `attend` runs after every message.
   - Panes on screen while the window has focus (`visible_panes`) are cleared with
     `ClearAttention`, once per raise.
   - Raises elsewhere are announced once, as `Effect::SystemNotification`, but only
     while the window is in the background (`WindowFocused`).
   - Raises that already existed when the app connected aren't announced.
   - Threads and tabs show their most urgent pane (`thread_attention`, `tab_attention`;
     waiting outranks done).
   - Task cards show the most urgent of the threads linked to the task and its nested
     subtasks (`task_attention`, `subtask_attention`; `view/board/card.rs`).
   - Older daemons (< 1.9) are never asked.
5. **CLI.** `urushi ls` marks panes `[finished]` or `[needs input]`.

## Invariants
- Attention is never stored and never changes the workspace revision (`tests/attention.rs`).
- Each raise is announced at most once and cleared at most once per client
  (`AttentionTracker`, `update/tests/attention.rs`).
- Only a client that knows the user can see a pane clears it. Hover, motion and the wheel
  don't clear (`server/connection.rs::is_user_action`).
- Hooks never fail an agent: `signal_command` does nothing outside an Urushi pane and
  always exits 0.

## Tests
- `urushi-core`:
  - `attention.rs`: signal words, urgency order.
  - `agent/hooks.rs`: the exact shell commands, Claude's settings JSON, Codex's TOML, and
    the options on each command line.
- `urushi-mux`:
  - `emulator/notices.rs`: OSC 9 and 777, Urushi's signal, ConEmu's `9;4` progress
    ignored, sequences split across reads.
  - `tests/pty_session.rs::hooks_signal_through_the_terminal_device`: a real PTY.
- `urushi-daemon/tests/attention.rs`, end to end:
  - The real hook command and a bell raise attention.
  - Typing and `ClearAttention` clear it, without changing the revision.
  - Every pane gets `URUSHI_TTY` and the settings.
  - `claude_is_not_done_while_its_background_agents_run`: Claude's `Stop` command, fed
    Claude 2.1's hook input, stays quiet for a running subagent or workflow, and signals
    for shells, monitors, no tasks, and input without `background_tasks`.
- `urushi-app`:
  - `update/tests/attention.rs`: clearing what's visible, announcing in the background
    only and once, not announcing old raises, older daemons, and cards counting nested
    subtasks' threads.
  - `snapshot/tests.rs`: the sidebar's "finished" and "needs input", and the board's
    card badges.
- Manually: real Claude Code 2.1 started through `urushi tab new --agent claude` in a
  throwaway home. Its `Stop` hook marked the pane `[finished]`, and typing cleared it.
  With two background subagents (Claude 2.1.322), the pane stayed unmarked through the
  main agent's two interim turns, and became `[finished]` after its final answer.
  notify-rust's call returned `Ok` from an unbundled process on macOS 26 (not seen on
  screen).

## Limitations and follow-ups
- Agents typed by hand get no hooks. Run
  `claude --settings "$URUSHI_CLAUDE_SETTINGS"` (or alias it) to get them. Cursor has no
  command-line hooks.
- Clicking a notification brings Urushi to the front (installed app) but doesn't open the
  thread yet.
- Mobile shows the dots but posts no notifications (that needs push setup).
- Windows: ConPTY has no device path, so hooks can't signal. Bells and OSC notifications
  work.
- There is no setting to turn notifications off yet.
- A Claude subagent that moves its own command into a background shell and waits on it
  drops out of `background_tasks`; only its shell is listed, and shells aren't
  attributed to an agent there. "finished" then shows while that subagent waits
  (measured with Claude Code 2.1.322). Telling such a shell from a server the main agent
  left running would need the transcripts (`transcript_path`).
- Claude versions whose `Stop` input has no `background_tasks` still show "finished"
  while background subagents run. Codex isn't filtered either: its `notify` runs after
  every turn.
- Panes started before the daemon picked up the new settings keep the old `Stop` hook.
