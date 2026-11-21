# Feature: Shelved threads

- **Status:** Shipped (daemon, CLI and GUI)
- **Owners (crates):** `urushi-core` (`thread.rs`: `ThreadState::Shelved`), `urushi-proto` (`Thread.shelved`, `SetThreadShelvedRequest`, `convert.rs`), `urushi-mux` (`SessionHandle::same_session`), `urushi-daemon` (`service/shelve.rs`, `service/restore.rs`, `service/spawn.rs`), `urushi-client` (`set_thread_shelved`), `urushi-app` (`update/shelve.rs`, `view/shelved.rs`, `view/thread_card/`, `view/menu/thread.rs`), `urushi-cli` (`thread shelve|unshelve`)
- **Related ADRs:** [0060](../adr/0060-shelve-threads-by-stopping-their-panes-and-keeping-restore-points.md), [0013](../adr/0013-restore-points-for-panes.md), [0056](../adr/0056-show-the-users-own-actions-before-the-daemon-confirms.md)

## What it does
**Shelve** a thread (UR-75) to free what its terminals use without losing them:
- Every pane in every tab stops.
- The worktree, branch, tabs and layout are kept. Each pane's restore point is kept
  too: its directory, its output and the agent's conversation.
- The thread stays where it was in the sidebar, dimmed, with a "shelved" badge.
- Opening it shows what is kept and an **Unshelve** button.

**Unshelving** brings every pane back as after a daemon restart: the old output, a dim
"── unshelved ──" line, and a shell in the same directory. Then Claude resumes the exact
session it had open, Codex and Cursor resume their latest conversation, and a program
that was running runs again.

A shelved thread stays shelved across daemon restarts. It can still be renamed, pinned,
moved, given notes or a status, and deleted.

Where to find it:
- The GUI: right-click a thread for **Shelve** or **Unshelve**. Shelving a thread whose
  agent is working asks first.
- The CLI: `urushi thread shelve <thread>` and `urushi thread unshelve <thread>`.
  `urushi ls` marks shelved threads `[shelved]`.

## How it works
- **Wire (protocol 1.34):**
  - `SetThreadShelved{thread_id, shelved}` → `Empty`, once the change is saved.
  - `Thread.shelved`: a shelved thread goes out as `state = READY` plus `shelved`, so
    older clients still read the workspace (ADR 0060). `convert.rs` maps it to and from
    `ThreadState::Shelved`.
- **Core:** `Thread::shelve` (`Ready → Shelved`), `unshelve` (`Shelved → Ready`), and
  `begin_deletion` also from `Shelved`. `is_ready()` is false while shelved, so
  `create_tab` and starting a task in the thread refuse it.
- **Shelving** (`service/shelve.rs::shelve_thread`):
  1. Read the thread's panes. The thread must be ready, else `FAILED_PRECONDITION`.
  2. `save_restore_points_of(those panes)` brings their restore points up to date while
     the processes run. Claude's session id is only readable from a live process.
  3. Mark the thread shelved, which saves the workspace.
  4. `stop_sessions_keeping_panes` takes the sessions out of `Service::sessions`,
     remembers their handles in `shelved_sessions`, and kills them.
- **Exits** (`spawn.rs::on_pane_exited(pane, exited)`):
  - A session in `shelved_sessions` leaves its pane in the layout.
  - The exit only acts if `exited` is still the pane's session
    (`SessionHandle::same_session`). An old session ending after an unshelve can't
    close the pane or kill its new session.
  - A periodic restore-point save that overlaps a shelve skips panes whose session
    stopped meanwhile (`restore.rs::runs`), so it can't overwrite the saved
    conversation.
- **Unshelving** (`unshelve_thread`): mark the thread ready, then call
  `restore_panes(its panes, "── unshelved ──")`. This is the same per-pane restore
  that `restore_sessions` runs at startup.
- **Startup:** `restore_sessions` only starts ready threads. Shelved panes stay in
  `State::all_panes`, so their restore points are never pruned.
- **Refusals:**
  - `split_pane` (`tabs.rs::pane_home`) refuses a non-ready thread.
  - `Service::session` answers `FAILED_PRECONDITION` ("unshelve it first") for a pane
    of a shelved thread. This covers reading, typing and attaching.
- **App:**
  - `ThreadMsg::Shelve{thread, confirmed}` and `Unshelve` are handled in
    `update/shelve.rs`.
  - While a tab has a running agent (`Tab.running`), Shelve opens
    `Dialog::ShelveThread` first.
  - `AppState.shelving` holds the asked-for state. The card shows "shelving…" or
    "unshelving…" and the menu item is gone until the workspace shows it, or a refusal
    (`Followup::Shelving`) shows the error (ADR 0056).
  - `current_tab()` is `None` for a shelved thread. So the main area shows
    `view/shelved.rs` instead of tabs, `sync_panes` detaches its panes, and New tab
    does nothing.
  - The card's title, meta and project stripe are dimmed.
  - The menu items are gated by `AppState::daemon_supports_shelving` (minor ≥ 34).

## Invariants
- Shelving never touches the worktree, branches, tabs or layout. Only processes stop
  (`tests/shelve.rs`).
- A shelved thread runs no pane, and nothing can start one in it until it is
  unshelved: new tabs, splits, tasks, input and attaching are all refused.
- A shelved thread stays shelved across daemon restarts, and its restore points are
  kept.
- A pane's exit only closes the pane when that session was the pane's current one, and
  wasn't stopped for shelving.
- Opening a shelved thread in the GUI never unshelves it.

## Tests
- `urushi-core/src/thread.rs`: transitions, and the stored form.
- `urushi-proto`: `convert/tests.rs` (shelved as READY plus `shelved`), `tests/round_trip.rs`.
- `urushi-daemon/tests/shelve.rs`, against a real daemon:
  - Shelving keeps the tabs, layout and worktree, and ends the shell's process.
  - Tabs, splits, reading and a second shelve are refused while shelved.
  - The thread stays shelved across a restart.
  - Unshelving brings back the old output with its line, the shell in its
    subdirectory, and a (fake) Claude resumed with its session id.
  - An unshelve straight after a shelve keeps the new terminals. This test fails
    without the session-identity checks.
  - A shelved thread can be deleted.
- `urushi-app/src/update/tests/shelve.rs`:
  - The menu items, and none for older daemons.
  - The question asked while an agent works.
  - The card's "shelving…" badge, and a refusal clearing it.
  - The placeholder and its Unshelve button, on desktop and phone.
  - The pane detached and attached again.
- The snapshot scenario `shelved-thread` (`snapshot/tests.rs`).

## Limitations and follow-ups
- Unshelving has a restart's limits ([recoverable sessions](recoverable-sessions.md)):
  programs start from the beginning, and two Codex or Cursor panes in one directory
  resume the same latest conversation.
- Panes opened between the restore-point save and the shelve itself have no restore
  point, and come back as shells in the worktree.
- Only whole threads can be shelved, not single tabs.
- Older clients (before 1.34) see a shelved thread as ready, and get errors when they
  try to attach its panes.
- A daemon older than 1.34 can't load a workspace with a shelved thread. Unshelve it
  before downgrading.
- Windows has no `ps`, so agents and programs aren't restored there, only output and
  directories, as after a restart.
