# Feature: The Urushi workspace GUI

- **Status:** Shipped. Verified on macOS (rendering, input, accessibility, dialogs), in the iOS simulator and on the Android emulator (remote over Tailscale). See [platforms](../platforms.md).
- **Owners (crates):** `urushi-app`, `urushi-ui`, `urushi-render`, `apps/desktop`
- **Related ADRs:** [0006](../adr/0006-all-rust-gpu-gui.md), [0002](../adr/0002-daemon-source-of-truth.md), [0019](../adr/0019-in-app-frosted-glass.md)

## What it does
A graphical workspace in dark frosted glass. Behind everything, a near-black window has
a few large, soft colour glows (Urushi red top left, indigo bottom right). The sidebar
and the main area float over them as translucent, rounded glass panels. Overlays
(dialogs, menus, selectors, the task board, drag ghosts) blur whatever they cover
([ADR 0019](../adr/0019-in-app-frosted-glass.md)), and a dialog's scrim frosts the whole
window. On phones the panels fill the screen edge to edge.
- **Appearance** (Settings, 1.13 daemons): six color schemes (Urushi, Ocean, Forest,
  Dusk, Ember, Mono), each shown as a small preview of its backdrop, or **Custom** with a
  `#rrggbb` field for the background, the three glows and the accent. The **Logo** is
  drawn in its own colors, in the accent, in the text color, or hidden. Changes show at
  once; **Save** keeps them, **Close** puts the old look back. The look is a daemon
  setting, so every client of that daemon looks the same; the CLI sets it with `urushi
  settings set colors ocean|#bg,#g1,#g2,#g3,#accent` and `logo original|accent|mono|hidden`.
- **Hiding and resizing the sidebar** (desktop and tablet widths):
  - The sidebar button (a window outline) in the sidebar's header slides it away; the
    same button then brings it back from wherever it can be seen: the left of the tab
    bar, the top of the empty screen (no thread or no repo yet), or the task panel's
    header while the panel covers the tab bar. Cmd+\ (Ctrl+Shift+\ elsewhere) does both.
  - Drag the gap between the sidebar and the main panel to resize it, from 200 px up to
    560 px or half the window. Dragging it narrower than 140 px collapses it, and it
    reopens at its last width.
  - The width and whether it's collapsed are remembered across launches, with the
    selection (`ui_state.rs`). Logic: `sidebar.rs`, `update/sidebar.rs`.
- **Sidebar:** a Projects tab (projects → pinned threads → status sections → threads, then threads in no project) and a Repos tab. See [organizing threads](thread-organization.md) for the context menu, statuses and drag and drop. A thread whose agent finished shows a green dot and "finished"; one waiting for you shows an amber dot and "needs input", and Urushi posts a system notification while it's in the background ([agent attention](agent-attention.md)). Under the tabs, **Tasks** slides out the [task boards](task-boards.md), and **>_** drops down the [quick terminal](quick-terminal.md).
- **Tab bar:** the tabs of the selected thread. **+** opens the [agent launcher](agent-launcher.md) picker; **>_** on the right drops down the [quick terminal](quick-terminal.md).
- **Terminal area:** split, recoverable terminal panes. A right or two-finger click on a pane opens its menu: **Split right**, **Split down**, **Copy**, **Paste**, **Close pane** (see [split-pane layouts](split-pane-layouts.md)). Touch screens have no secondary click, so on mobile the tab bar keeps **→ ↓ ×** buttons for the focused pane. A thread with no tabs (new, or its last tab closed) keeps its tab bar and shows the launcher inline.
- **Header:** the logo (the app icon's mark, in the chosen style) and the name, a DEV badge in debug builds, and the connection.
- **Status bar:** under the terminals: the thread's branch, and the [plan usage](agent-usage.md) of the agent running in the focused pane.
- **Dialogs:**
  - *Add repo:* **Choose a folder…** opens the system picker and adds the repository straight away. Against a remote daemon (or on mobile) there is no picker, so a path is typed. "Remote over SSH" and "Connect from your phone" are shown as coming soon.
  - *New thread:* **repo**, **project** (or None) and **start from** (chosen from the repo's branches) are selectors; title and **branch** are typed. The branch follows the title as `<prefix><title in lower-case-with-dashes>` until edited, and clearing it makes it follow again. Started from a project (or a task near one), the project is preselected.
  - *Prompts:* rename a thread or project, new status, new project.
  - *New tab:* the launcher picker.
  - The thread-deletion warning, settings (remote branch deletion, tailnet serving, per-agent skip-permissions, branch prefix, shell), and connect to a remote host.

## How it works
- The app connects to the local daemon (starting it if needed) or a remote one. The connection runs on a background tokio runtime (`backend.rs`), which reconnects every 2 s if the connection drops.
- **Workspace mirror:** the workspace is mirrored from `WorkspaceChanged` events.
- **Look** (`appearance.rs`): `Settings.appearance` (core `Appearance{scheme, logo}`,
  presets in `Scheme::colors`) becomes the theme with `theme_for` (`Theme::dark()` with
  `with_scheme`: the fill, the glows' colors at their places and softness, the accent,
  and the glass tinted towards the fill). `AppState::appearance` is Settings' draft
  (`LookDraft`, `Msg::{LookScheme, LookLogo}`, `Field::LookColor`) while it is open, else
  the daemon's; the runner rebuilds its theme when it changes. `appearance::logo` tints
  the mark (colored parts in the tint, light strokes kept light, alpha kept) and caches
  it under an id per look so the renderer's image cache doesn't show an old one. Save
  sends `SettingsPatch.appearance` only when the look changed.
- **Reopening:** the app shows the thread and tabs it showed when it was closed, for the local daemon (`ui_state.rs`, see [recoverable sessions](recoverable-sessions.md)).
- **Attach on view:** panes of the visible tab (and of the quick terminal while its panel is out) are attached, sized from their rectangles, and kept up to date with acknowledged `ScreenUpdate`s. Hidden tabs are detached. Which surface takes typing and the mouse is decided in `surface.rs`.
- **Keyboard:**
  - Typing goes to the focused pane (of the quick terminal while its panel is down).
  - Shortcuts use Cmd on macOS, Ctrl+Shift elsewhere:
    - T: new tab (opens the launcher picker)
    - W: close pane
    - D: split right
    - Shift+D (macOS) / E: split down
    - C / V: copy and paste
    - `,`: settings
    - N: new thread
    - B: open or close the task board
    - `\`: hide or show the sidebar
    - `[` / `]`: previous and next tab
    - `` ` ``: drop the quick terminal down, or put it away
    - Alt+arrows: move focus
- **Mouse** (`update/pointer.rs`):
  - Click to focus a pane, drag dividers to resize.
  - A right or two-finger click always focuses the pane under it and opens the pane menu, even over an application that takes the mouse (`update/panes.rs`).
  - If the pane's application asked for the mouse (`TerminalModes.mouse_reporting`, e.g. Claude Code, Codex, vim), presses, drags, releases, middle clicks and hover are sent to it as `MouseInput`. Hover and drag movement are sent once per cell. **Shift** selects text instead, as in other terminals.
  - Otherwise, drag to select text.
  - The wheel is sent as `WheelInput`, and the daemon decides what it does (see [recoverable sessions](recoverable-sessions.md)). Daemons before protocol 1.3 get `ScrollPane` instead.
  - Input is queued on the UI thread in order (`Client::try_notify`), so a click's press and release, and a burst of motion, reach the application in the order they happened (`tests/mouse.rs::queued_input_arrives_in_order`).
  - While the task panel, the quick terminal or a dialog covers the terminals (`AppState::terminals_covered`), the mouse, wheel and typing don't reach them. The quick terminal's own panes take the mouse inside its panel (`AppState::surface_at`).
- **Older daemon:** the daemon keeps running across app updates. When it speaks an older protocol minor than the app, Settings shows it as older and highlights **Restart daemon…**. The confirmation says how many terminals in how many threads will close; only its red button restarts (Enter doesn't), and a restart under way can't be cancelled. The app then reconnects, which starts its own daemon (`update/daemon.rs`, `backend.rs::stop_daemon`). Until then, agent switches are inert rather than snapping back, and the launcher explains why agents are missing.
- **Drag and drop:** a dropped file (e.g. a screenshot for Claude) is pasted into the focused pane as a shell-escaped path followed by a space, as macOS Terminal and iTerm2 do. A dropped folder is added as a repo, unless it lands on a terminal. Logic: `update/intake.rs`.
- **Text fields** (`update/fields.rs`, toolkit `textfield.rs`): every dialog field edits like a native one.
  - Cmd (macOS) / Ctrl (elsewhere) + A, C, X, V, Z, Shift+Z (and Ctrl+Y): select all, copy, cut, paste, undo, redo. W closes the dialog.
  - Shift extends the selection with any move. Option (macOS) / Ctrl (elsewhere) + arrows and Backspace work by word; Cmd+arrows and Cmd+Backspace go to the line's ends on macOS, as do Home and End. The Emacs keys (Ctrl+A/E/B/F/H/D) work on macOS.
  - Mouse: click to place the caret, drag or Shift-click to select, double-click selects a word, triple-click everything. Right-click opens Cut / Copy / Paste / Select all.
  - Tab and Shift+Tab move between fields. Long text scrolls to keep the caret in view.
  - Multiline fields (task notes) wrap their text. Enter starts a new line there, Up/Down/Home/End work on lines, and Cmd+Enter (macOS) / Ctrl+Enter (elsewhere) submits the dialog, as it does from any field.
- **Deleting a thread:** the app first requests a `DeletionPlan`. The warning lists everything that will be destroyed. Only the red Delete button sends `DeleteThread{token}`; Enter never does.
  - Confirming closes the warning and hides the thread at once. The daemon deletes in the background while the user keeps working. Logic: `update/deletion.rs`.
  - The app keeps the daemon's workspace as reported and shows it without threads being deleted (`deletions.rs`). Those are threads the daemon reports as `Deleting` (from any client) and threads this app asked to delete that the daemon still lists. If the selected thread goes, the first remaining one is shown. Otherwise selection and focus stay where they are.
  - The answer to `DeleteThread` never closes a dialog or moves focus. A refusal (e.g. the thread changed since the warning) shows the thread again, and the error banner says why. A deletion that fails partway brings the thread back as "deletion failed".

## Invariants
- The app holds no authoritative state. Every change goes through a daemon request.
- There is no path from the UI to `DeleteThread` without a plan shown in the dialog (`update/tests.rs::deleting_a_thread_always_goes_through_the_warning`).
- Deleting a thread never blocks the app and never takes focus (`update/tests/deletion.rs`).
- A collapsed sidebar always has a visible, clickable button to bring it back, whatever the main area shows (`view/mod.rs::a_collapsed_sidebar_can_always_be_shown_again`).

## Tests
- `crates/urushi-app/src/update/tests.rs`: connect, attach, resize, screen updates, typing vs shortcuts, the deletion flow, dialog editing, connect-required mode, selection and copy. `update/tests/fields.rs`: field shortcuts, mouse selection and the field menu; `urushi-ui/tests/fields.rs`: pointer → caret, scrolling, selection painting.
- `crates/urushi-app/src/update/tests/daemon.rs`: the restart confirmation and reconnecting.
- `crates/urushi-app/src/sidebar.rs` and `update/tests/sidebar.rs`: hiding, the shortcut, resizing within limits, snapping shut, phones ignoring both; `view/mod.rs` tests: the edge, its cursor, and a reachable show button while hidden (with a thread, the launcher, the task panel out, and an empty workspace).
- `crates/urushi-app/src/update/tests/flows.rs`: launcher, branch name following the title, adding repos, dropped files, agent settings. `update/tests/organize.rs`: menus, prompts, drag and drop, selectors. `update/tests/panes.rs`: the pane menu and focus after a pane closes.
- `cargo run -p urushi-app --example snapshot -- out.bmp 1280 820 1 <scenario>` renders one of `snapshot::SCENARIOS` (`launcher`, `picker`, `add-repo`, `new-thread`, `new-thread-base`, `new-thread-project`, `settings`, `settings-appearance`, `settings-custom`, `logo-hidden`, `menu`, `menu-status`, `drag`, `drag-project`, `repos`, `board`, `board-drag`, `board-tab-drag`, `task`, `task-menu`, `column-menu`, `task-nested`, `nest`, `status`, `pane-menu`, `sidebar-collapsed`, `quick-terminal`, `setup`, `setup-mode`, `project-base-thread`, `project-settings`, and the Jira screens) offscreen for visual checks; `snapshot.rs` tests render each at desktop and phone sizes.
- `crates/urushi-app/src/appearance.rs`: themes from schemes, logo styles, drafts; `snapshot/tests.rs`: the look shown at once, reverted on Close, sent only when changed. Core `appearance.rs`, proto `convert/appearance.rs`, daemon `service/settings.rs`, CLI `look.rs`.
- `crates/urushi-app/src/terminal.rs`: pane geometry, dividers, selection text, painting.
- `crates/urushi-ui/tests/layout.rs` and `crates/urushi-render/tests/pixels.rs`: toolkit and GPU.

## Limitations and follow-ups
- No tab renaming in the UI (the protocol and CLI support it).
- Dropped file paths are local to the client. Against a remote daemon they're pasted as-is and won't exist there.
- winit doesn't report the pointer during a drag, so "dropped on a terminal" uses the last known pointer position.
- Adding a repo over SSH, and pairing a phone from this dialog, are placeholders.
- The mobile soft keyboard relies on winit's IME support. A touch key bar provides Esc, Tab, Ctrl chords and arrows.
