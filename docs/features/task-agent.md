# Feature: Task agent and first-run setup

- **Status:** Shipped
- **Owners (crates):** `urushi-core` (`agent/mode.rs`, `board/prompt.rs`, `AgentSettings`), `urushi-daemon` (`cli.rs`, `service/task_agent.rs`, `service/threads.rs`, `service/settings.rs`), `urushi-proto` (`agents.proto`, `convert::agent_mode_*`), `urushi-app` (`task_agent.rs`, `update/setup.rs`, `view/setup.rs`, `view/new_thread.rs`, `update/task_threads.rs`, `view/task_in_thread.rs`), `urushi-cli` (`urushi board show`)
- **Related ADRs:** [0026](../adr/0026-issue-keys-in-branch-names.md), [0018](../adr/0018-start-tasks-in-existing-threads.md), [0014](../adr/0014-task-prompt-via-pane-environment.md), [0024](../adr/0024-task-prompts-by-reference.md), [0009](../adr/0009-agents-typed-into-a-shell.md), [0012](../adr/0012-subtasks-as-checklists-and-thread-links.md), [0015](../adr/0015-subtasks-are-nested-tasks.md)

## What it does
**Start thread** on a task or subtask opens a thread that runs the user's **task agent**
(Claude, Codex or Cursor) in their **task mode** (Normal or Plan). The agent's first
prompt refers to the task, a Jira issue by its key and address, and tells it to review
the task in full first with `urushi board show <KEY or id>` (which prints its notes,
subtasks and attachments, and for a Jira issue its fields, comments, links and files),
and for Jira to ask before changing it there and to put the issue's key in commit
messages and pull request titles (ADR 0026). Panes can run that CLI against their
daemon. If the daemon can't find its CLI, the task's title, notes and checklist are
pasted in instead. The new-thread dialog still opens
first, prefilled, and says what will start, e.g. "Starts Claude in plan mode with this
task as its prompt."

The dialog can also send the task to an **existing thread** ("New thread | Existing
thread", 1.10 daemons). A new tab in the picked thread runs the task agent the same way,
and the task is linked there.

When the app connects to a daemon on which no task agent is chosen yet, a **setup
wizard** asks for one:
1. Which installed agent should work on your tasks?
2. How should it start (only the modes that agent has)?

**Later** (or Escape) puts it off until the app next starts. Settings has the same
choice under **Tasks start with**.

## How it works
- **Settings:** `AgentSettings{task_agent: Option<Agent>, task_mode: AgentMode}` in the
  daemon's document. On the wire, `AgentSettings.task_agent` / `task_mode` and
  `SettingsPatch.task_agent` / `task_mode` (`AGENT_UNSPECIFIED` clears the choice). The
  CLI sets them with `urushi settings set task-agent claude` and `task-mode plan`.
- **The prompt** (ADR 0024): `urushi_core::task_reference_prompt(board, task)` when the
  daemon found its CLI (`cli::locate`: beside `urushid`, or in the bundle's
  `Resources/bin`; `DaemonOptions::cli`), else `urushi_core::task_prompt(board, task)`,
  for a task at any depth (`State::task_prompt(link, by_reference)`). Every pane gets
  `URUSHI_HOME` and the CLI's directory first on `PATH` (`cli::pane_env`).
- **The copy** (`task_prompt`):
  - A card gives `Task:`, its `Notes:` and its `Subtasks:` checklist.
  - A subtask gives `Step:`, its own notes and subtasks, the tasks it is part of
    ("Card › Subtask"), its parent's notes, and its parent's subtasks with the step
    marked.
  - Capped at `MAX_PROMPT_BYTES`.
- **Starting it:** `CreateThread{task_id, subtask_id, start_task_agent: true}`.
  1. `Service::task_agent_for` resolves the agent (`agent_task_line`) and the prompt
     (`State::task_prompt`) before the worktree is made.
  2. `finish_thread_creation` spawns the first pane with `URUSHI_TASK_PROMPT` in its
     environment (`spawn_session_with_env`) and names the tab after the agent.
  3. After the thread is recorded, `start_task_agent` types
     `Agent::task_command_line(program, skip, mode)`.
- **In an existing thread:** `CreateTab{thread_id, task_id, subtask_id,
  start_task_agent: true}` (`service/task_tabs.rs::create_task_tab`). It resolves the
  same `TaskAgent` before spawning, starts the pane with `URUSHI_TASK_PROMPT`, records the
  tab and the link in one `mutate`, and then types the command. CLI: `urushi tab new
  <thread> --task <id> --start-agent`.
- **Command lines:**

  | Agent | Normal | Plan |
  |---|---|---|
  | Claude | `claude "$URUSHI_TASK_PROMPT"` | `claude --permission-mode plan "$URUSHI_TASK_PROMPT"` |
  | Codex | `codex "$URUSHI_TASK_PROMPT"` | (no plan mode: as Normal) |
  | Cursor | `cursor-agent "$URUSHI_TASK_PROMPT"` | `cursor-agent --plan "$URUSHI_TASK_PROMPT"` |

  The skip-permissions switch adds the agent's flag. For Claude in plan mode, it adds
  `--allow-dangerously-skip-permissions` instead, so bypass is available after the plan.
- **App:**
  - `AppState::task_agent()` reads the choice; it is `None` on daemons older than 1.7.
  - `submit` sends `start_task_agent` only for a dialog opened from a task, when a task
    agent is chosen. Otherwise the thread opens on the launcher, as before.
  - `Dialog::NewThread.target` (`ThreadTarget::{New, Existing(thread)}`) is the switch.
    `daemon_supports_task_in_thread` (≥ 1.10) shows it. Existing mode sends `CreateTab`
    with `Followup::TaskTabCreated`, which closes the board and selects the new tab.
  - `AppState::setup_due` opens `Dialog::Setup` once both the settings and the installed
    agents (`SetupGate::agents_loaded`) have arrived, if nothing else is open.
  - `SetupGate::dismissed` keeps it closed for the rest of the run.

## Invariants
- A refused start leaves no thread, worktree, branch, tab or link: no task agent chosen
  or the agent not installed is `FAILED_PRECONDITION`, and `start_task_agent` without a
  task is `INVALID_ARGUMENT`
  (`tests/task_agents.rs::a_task_agent_that_cant_start_leaves_nothing_behind`). The same
  holds in an existing thread: no tab and no link
  (`a_task_that_cant_start_in_a_thread_leaves_nothing_behind`).
- The whole prompt reaches the agent as one argument
  (`tests/task_agents.rs::a_task_starts_the_task_agent_in_its_mode_with_the_task_as_prompt`).
- A mode the agent lacks is never passed to it (`Agent::effective_mode`; `agent/mode.rs`
  tests). The wizard saves such a choice as Normal.
- The wizard never opens on a daemon that can't store the choice, or when no agent is
  installed (`update/tests/setup.rs`).

## Tests
- **Core:**
  - `agent/mode.rs`: flags per agent × mode × skip.
  - `board/prompt.rs`: task, subtask, a deeper subtask, empty notes and truncation, and
    references to a task, a step and a Jira subtask.
  - `agent.rs`: older settings load.
- **Proto:** `convert/tests.rs` (modes, unknown values mean Normal) and
  `tests/round_trip.rs`.
- **Daemon:**
  - `service/settings.rs` (choose and clear).
  - `tests/task_agents.rs`, with a fake `claude` that prints each argument, for both a new
    thread and an existing one (`a_task_starts_in_an_existing_thread_in_a_new_tab`), and
    by reference with a CLI beside the daemon, which the pane finds on `PATH` with
    `URUSHI_HOME` set (`with_the_cli_beside_it_a_task_starts_by_reference`).
  - `cli.rs`: finding the CLI, the pane environment.
- **App:**
  - `update/tests/setup.rs`: the wizard, the Settings choices, and Start thread with and
    without a task agent or a 1.7 daemon.
  - `update/tests/task_in_thread.rs`: the switch, picking, the `CreateTab` request, and
    older daemons.
  - `snapshot.rs`: the `setup`, `setup-mode` and `task-in-thread` scenarios.

## Limitations and follow-ups
- The prompt is fixed text. There is no per-repo or per-board template yet.
- Reviewing the task costs the agent a command (and in plan mode maybe a permission
  prompt).
- After a daemon restart, a resumed agent isn't put back into plan mode (ADR 0013, ADR 0014).
- PowerShell can't expand `"$URUSHI_TASK_PROMPT"`. Agent launching assumes a POSIX-style
  shell.
- Choosing a task agent that isn't installed is possible from Settings (all three are
  offered). Start thread then fails with "… is not installed".
