# Feature: Agent attention and activity (finished / running / needs input)

- **Status:** Shipped
- **Owners (crates):** `urushi-core` (`attention`, `agent::hooks`), `urushi-mux` (`emulator/notices.rs`, `SessionStatus.{alerts, activity, running}`, `SpawnSpec.tty_env`), `urushi-daemon` (`service/attention.rs`, `service/running.rs`), `urushi-app` (`attention.rs`, `running.rs`, `pulse.rs`, `update/attention.rs`, `view/attention.rs`, `platform::notify`), `urushi-cli`
- **Related ADRs:** [0017](../adr/0017-agent-attention-through-the-pane-terminal.md), [0033](../adr/0033-running-agents-from-their-hooks.md), [0009](../adr/0009-agents-typed-into-a-shell.md), [0014](../adr/0014-task-prompt-via-pane-environment.md)

## What it does
When an agent in a pane **finishes its response**, its thread gets a green dot and
"finished" in the sidebar. When it **waits for you** (a permission prompt, a question, or
any program ringing the bell), it gets an amber dot and "needs input". The same dot shows
on the tab, and task cards linked to the thread (directly or through a subtask at any
depth) get a **Finished** or **Needs input** badge and a border in that color. While Urushi is in the
background, each new one also posts a system notification, e.g. "Invoice API — Claude
finished". The dot goes away once you look at the pane (it's on screen and the window
has focus) or type into it.

While a Claude agent is **working**, its thread gets a pulsing blue dot and "running".
Working means it took a prompt, is using tools, or a subagent is. An agent that is open
but idle doesn't count. Tabs, subtask rows, card thread chips and task cards (a
**Running** badge) show the same. Each shows one state, by priority: needs input >
running > finished. Running isn't attention: it doesn't color a card's border, isn't
announced, and isn't cleared by looking. It ends when the agent finishes or asks for
input, when its session ends, or when you press Esc or Ctrl+C in the pane.

## How it works
1. **Hooks.** Every pane's environment has `URUSHI_TTY` (its terminal device, set by the
   mux) and the hook configuration (`agent::hooks::hook_env`). Agent command lines start
   with `Agent::hook_args`:
   - Claude: `--settings "$URUSHI_CLAUDE_SETTINGS"`, with `Stop` → done,
     `Notification` (permission prompt, question) → waiting, and `SessionStart`
     (`startup`) → `ready` (`ready_command`). `ready` is **not attention**: it says
     Claude reads input, so a drafted task message can be pasted in
     ([task agent](task-agent.md), ADR 0030). `UserPromptSubmit`, `PreToolUse`,
     `PostToolUse` and `SubagentStart` → `running` (`running_command`), and `SessionEnd`
     → `idle` (`idle_command`), neither of them attention (ADR 0033).
   - Codex: `notify` → done, and approval requests as OSC 9 → waiting.

   A hook runs `signal_command`, which writes `ESC ] 777 ; urushi ; done BEL` to
   `$URUSHI_TTY`.
2. **Detection.** The mux's `Notices` parser watches each pane's output for that signal,
   for `OSC 9`/`OSC 777;notify` (waiting) and, through alacritty, the bell (waiting).
   `SessionStatus.{alerts, alert}` reports the latest. `ready` signals only count up
   `SessionStatus.ready` (`EmulatorEvent::Ready`). `running` and `idle` count up
   `SessionStatus.activity` (`EmulatorEvent::Running`). `SessionStatus.running` follows
   the order of the signals: `running` sets it, and `idle` or any alert clears it. So a
   turn's last tool and its `Stop`, read together, leave the pane stopped
   (`running_follows_the_order_of_signals_that_arrive_together`).
3. **Daemon.** `watch_session` calls `raise_attention(pane, kind)`. Attention is kept in
   memory per pane with its time. A repeat of the same kind is ignored. Each change wakes
   `WorkspaceChanged` subscribers without a new revision, and `workspace()` lists
   `Tab.attention[PaneAttention{pane_id, kind, since_ms}]` (and `QuickTerminal.attention`
   for the quick terminal's panes). Typing or clicking in a pane
   (`Input` key/text/paste/press, `Service::user_acted`, which also counts toward a task
   message waiting to be drafted there) and `ClearAttention{pane_ids}` clear it. Closed panes
   are forgotten.

   **Running** (`service/running.rs`) works the same way:
   - The watcher sees `activity` go up and calls `set_running(pane, running)`.
   - Running panes are kept in memory with their start time and listed as
     `Tab.running[PaneRunning{pane_id, since_ms}]` (and `QuickTerminal.running`).
   - A pane stops running when:
     - attention is raised (`raise_attention`), or
     - `idle` arrives, or
     - the user sends Esc or Ctrl+C (`server/connection.rs` → `is_interrupt` →
       `user_interrupted`; Claude runs no hook when interrupted), or
     - the pane closes.
   - Other typing leaves it running, because Claude queues messages typed while it works.
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
   - **Indicators** (`running.rs`): `Indicator {Done, Running, Waiting}` combines both.
     `thread_indicator`, `tab_indicator`, `quick_indicator`, `task_indicator` and
     `subtask_indicator` pick the highest, and the views draw it (`view/attention.rs`).
     Card borders still follow `task_attention` only.
   - **Pulse** (`pulse.rs`): while `any_running()` is true and the window isn't occluded,
     the runner's `about_to_wait` wakes about 12 times a second. `redraw` then sends
     `Msg::Pulse(brightness)`, which sets `AppState.pulse` and nothing else.
     `view/attention.rs` fades the running dot by it. Older daemons (< 1.21) send no
     running panes.
5. **CLI.** `urushi ls` marks panes `[running]`, `[finished]` or `[needs input]`.

## Invariants
- Attention and running are never stored and never change the workspace revision
  (`tests/attention.rs`, `tests/running.rs`).
- A pane that finished or waits isn't running (`raise_attention` clears it first).
- With nothing running, the app draws no extra frames (`Runner::pulsing`).
- Each raise is announced at most once and cleared at most once per client
  (`AttentionTracker`, `update/tests/attention.rs`).
- Only a client that knows the user can see a pane clears it. Hover, motion and the wheel
  don't clear (`server/connection.rs::is_user_action`).
- Hooks never fail an agent: `signal_command` does nothing outside an Urushi pane and
  always exits 0.

## Tests
- `urushi-core`:
  - `attention.rs`: signal words, urgency order.
  - `agent/hooks.rs`: the exact shell command, Claude's settings JSON (with the running
    and idle hooks), Codex's TOML, and the options on each command line.
- `urushi-mux`:
  - `emulator/notices.rs`: OSC 9 and 777, Urushi's signal, `running`/`idle` not being
    attention, ConEmu's `9;4` progress ignored, sequences split across reads.
  - `tests/pty_session.rs::hooks_signal_through_the_terminal_device`: a real PTY,
    including `activity` and `running`.
- `urushi-daemon/tests/attention.rs`, end to end:
  - The real hook command and a bell raise attention.
  - Typing and `ClearAttention` clear it, without changing the revision.
  - Every pane gets `URUSHI_TTY` and the settings.
- `urushi-daemon/tests/running.rs`, end to end:
  - The real running hook command marks the pane, without a revision bump.
  - Typing leaves it running.
  - Done, idle, Esc and the pane closing each end it.
  - `service/running.rs` unit tests: which inputs count as an interrupt.
- `urushi-app`:
  - `update/tests/attention.rs`: clearing what's visible, announcing in the background
    only and once, not announcing old raises, older daemons, and cards counting nested
    subtasks' threads.
  - `update/tests/running.rs`: priority (waiting > running > finished), cards counting
    nested subtasks' running threads, older daemons, and `Msg::Pulse`.
  - `pulse.rs`: brightness over a period, frame spacing.
  - `snapshot/tests.rs`: the sidebar's "finished", "running" and "needs input", and the
    board's card badges.
- Manually: real Claude Code 2.1 started through `urushi tab new --agent claude` in a
  throwaway home. Its `Stop` hook marked the pane `[finished]`, and typing cleared it.
  notify-rust's call returned `Ok` from an unbundled process on macOS 26 (not seen on
  screen).

## Limitations and follow-ups
- Running is shown for Claude only. Codex reports only a turn's end, and its other hooks
  need the user's trust. Cursor has no hooks. Both show only finished or needs input.
- An agent that crashes without running `SessionEnd` stays running until the pane's
  next signal, Esc or Ctrl+C, or the pane closing.
- Esc used for something else mid-turn (closing a Claude menu) clears running until the
  next tool call.
- Agents typed by hand get no hooks. Run
  `claude --settings "$URUSHI_CLAUDE_SETTINGS"` (or alias it) to get them. Cursor has no
  command-line hooks.
- Clicking a notification brings Urushi to the front (installed app) but doesn't open the
  thread yet.
- Mobile shows the dots but posts no notifications (that needs push setup).
- Windows: ConPTY has no device path, so hooks can't signal. Bells and OSC notifications
  work.
- There is no setting to turn notifications off yet.
