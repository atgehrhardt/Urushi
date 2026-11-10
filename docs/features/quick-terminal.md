# Feature: Quick terminal

- **Status:** Shipped (daemon, client, CLI and GUI)
- **Owners (crates):** `urushi-core` (`quick_terminal`, `pane_group`), `urushi-proto`, `urushi-daemon`, `urushi-client`, `urushi-cli`, `urushi-ui` (`slide_y`), `urushi-app`
- **Related ADRs:** [0031](../adr/0031-thread-less-quick-terminal.md), [0001](../adr/0001-in-house-multiplexer.md), [0013](../adr/0013-restore-points-for-panes.md)

## What it does
A terminal that belongs to no thread, for quick work anywhere. **Cmd+`** (Ctrl+Shift+`
elsewhere), the **>_** button beside **Tasks** in the sidebar, or the one at the right of
the tab bar drops it down over the top 60% of the main area, in frosted glass; the same
again, **Hide**, or a click below it puts it away. It starts a shell in the home
directory the first time, and can be split into panes like a tab (the pane menu, Cmd+D,
Cmd+W, Alt+arrows; → ↓ × in its header on touch screens). Putting it away leaves
everything in it running; it comes back after the daemon restarts, and on every client of
that daemon. It ends when its last pane closes or exits.

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

- **GUI** (`urushi-app`):
  - `quick_terminal.rs`: `QuickPanel{open, reveal, area, starting}`. `update/quick.rs`
    handles `QuickMsg::{Toggle, Hide, Reveal, Area}`: dropping it down sends
    `OpenQuickTerminal` once if the workspace has none (only to 1.20 daemons; older ones
    show no button and ignore the shortcut). If the quick terminal disappears while the
    panel is down, the panel goes away; if opening fails, the error shows.
  - `surface.rs`: the main area and the panel are two `Surface`s. While the panel is down
    it takes typing (also Escape, which never puts it away, and over the board), the pane
    shortcuts and its own mouse input; a dialog still comes first. `pane_rects()` is the
    panes of both, so `sync_panes` attaches the quick terminal's while the panel is out
    and detaches them once it has slid away (the processes keep running).
  - The runner animates `reveal` like the board's, and reports the panel's terminal
    area (`view::quick_area_key`) once fully down; the area is kept while it's away, so
    reopening attaches at once.
  - View: `view/quick_terminal.rs` (panel, header, toggle button with an attention dot),
    `view/terminal_area.rs` (the pane painter shared with the main area). Its attention
    clears while the panel is down and is announced as "Quick terminal" otherwise.

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
- `crates/urushi-ui/tests/slide.rs`: the panel's vertical slide.
- `crates/urushi-app/src/update/tests/quick.rs`: the shortcut and the request (once),
  older daemons, failure, attach and detach, typing and Escape over the board, dialogs
  first, split/focus/close by its id, the mouse in the panel only, the panel following
  the quick terminal away, attention. `snapshot/tests.rs` and the `quick-terminal`
  scenario: the toggles and the panel at desktop and phone sizes.

## Limitations and follow-ups
- One per daemon, unnamed. More would need a list on the wire (ADR 0031).
- The shortcut works only while Urushi's window has focus; there is no system-wide
  hotkey to summon the app.
- The panel's height is fixed (60%); it can't be dragged.
