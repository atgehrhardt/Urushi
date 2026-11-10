# Feature: Quick terminal

- **Status:** In progress (daemon, client and CLI; the GUI panel follows)
- **Owners (crates):** `urushi-core` (`quick_terminal`, `pane_group`), `urushi-proto`, `urushi-daemon`, `urushi-client`, `urushi-cli`
- **Related ADRs:** [0031](../adr/0031-thread-less-quick-terminal.md), [0001](../adr/0001-in-house-multiplexer.md), [0013](../adr/0013-restore-points-for-panes.md)

## What it does
A terminal that belongs to no thread, for quick work anywhere. The daemon keeps one; it
can be split into panes like a tab. Hiding it leaves everything in it running, and it
comes back after the daemon restarts. It ends when its last pane closes or exits.

## How it works
- **Core:** `QuickTerminal { id: TabId, group: PaneGroup }`. `PaneGroup` is the layout and
  focus a tab also owns ([split-pane layouts](split-pane-layouts.md)).
- **Daemon:** `State.quick_terminal` (stored in `workspace.json` as `quick_terminal`).
  - `Service::open_quick_terminal` (`service/quick.rs`) returns the open one, or starts a
    shell and records a new one. If two clients race, the one recorded first wins and the
    other shell is killed.
  - Layout requests resolve their `tab_id` with `State::group`/`group_mut`, which also
    match the quick terminal, so `SplitPane`, `ClosePane`, `FocusPane`, `MoveDivider`,
    `ResizePaneEdge`, `Equalize` and `CloseTab` work on it unchanged.
  - Its panes start through `spawn_session_in` with `PaneHome::Quick` (`service/spawn.rs`):
    the PTY's default directory (the user's home), the global default shell,
    `URUSHI_PANE_ID`, `URUSHI_TTY`, the agent hooks, `URUSHI_HOME` and `PATH`, but no
    `URUSHI_THREAD_ID` or `URUSHI_WORKTREE`. Per-pane variables inherited from where the
    daemon was started are cleared first (`PANE_VARS`).
  - `State::remove_pane` drops the quick terminal with its last pane. Restore points
    (`restore_sessions`, the `all_panes` keep-set) and attention (`QuickTerminal.attention`)
    cover it like a tab.
- **Protocol 1.20:** `Workspace.quick_terminal`, `OpenQuickTerminal` → `QuickTerminal`.
- **Client:** `Client::open_quick_terminal`, `QUICK_TERMINAL_SINCE_MINOR`;
  `WorkspaceMirror::{quick_terminal, group, owner_of_pane}`.
- **CLI:** `urushi quick open` prints it and its panes, `urushi quick close` ends it,
  `urushi ls` lists it, and `urushi pane split|close` work on its panes.

## Invariants
- At most one quick terminal per daemon; it always has at least one pane, and its focused
  pane is in its layout (`PaneGroup`, checked when loaded or received).
- Thread operations (deletion, startup repair) never touch it.

## Tests
- `crates/urushi-core/src/{pane_group,quick_terminal}.rs`: layout operations, focus,
  stored form.
- `crates/urushi-proto/src/convert/quick.rs`, `tests/round_trip.rs`: the wire form.
- `crates/urushi-daemon/src/service/state.rs`: lookup by id, removal with its last pane,
  repair and storage.
- `crates/urushi-daemon/tests/quick_terminal.rs`: opens once, home directory and
  environment, splits and closes by its id, `exit` ends it, keeps running without clients,
  attention, untouched by thread deletion, restored after a restart.
- `crates/urushi-client/src/workspace.rs`: mirror lookups.

## Limitations and follow-ups
- One per daemon, unnamed. More would need a list on the wire (ADR 0031).
