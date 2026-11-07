# Feature: The Urushi workspace GUI

- **Status:** Shipped. Verified on macOS (rendering, input, accessibility, dialogs), in the iOS simulator and on the Android emulator (remote over Tailscale). See [platforms](../platforms.md).
- **Owners (crates):** `urushi-app`, `urushi-ui`, `urushi-render`, `apps/desktop`
- **Related ADRs:** [0006](../adr/0006-all-rust-gpu-gui.md), [0002](../adr/0002-daemon-source-of-truth.md)

## What it does
A graphical workspace:
- **Sidebar:** Projects → Threads, showing each thread's branch and state.
- **Tab bar:** the tabs of the selected thread.
- **Terminal area:** split, recoverable terminal panes.
- **Dialogs:** add project (with folder picker and drag-and-drop), new thread, the thread-deletion warning, settings (remote branch deletion, tailnet serving, branch prefix, shell), and connect to a remote host.

## How it works
- The app connects to the local daemon (starting it if needed) or a remote one. The connection runs on a background tokio runtime (`backend.rs`), which reconnects every 2 s if the connection drops.
- **Workspace mirror:** the workspace is mirrored from `WorkspaceChanged` events.
- **Attach on view:** panes of the visible tab are attached, sized from their rectangles, and kept up to date with acknowledged `ScreenUpdate`s. Hidden tabs are detached.
- **Keyboard:**
  - Typing goes to the focused pane.
  - Shortcuts use Cmd on macOS, Ctrl+Shift elsewhere:
    - T: new tab
    - W: close pane
    - D: split right
    - Shift+D (macOS) / E: split down
    - C / V: copy and paste
    - `,`: settings
    - N: new thread
    - `[` / `]`: previous and next tab
    - Alt+arrows: move focus
- **Mouse:** click to focus a pane, drag to select text, drag dividers to resize, and use the wheel for scrollback.
- **Deleting a thread:** the app first requests a `DeletionPlan`. The warning lists everything that will be destroyed. Only the red Delete button sends `DeleteThread{token}`; Enter never does.

## Invariants
- The app holds no authoritative state. Every change goes through a daemon request.
- There is no path from the UI to `DeleteThread` without a plan shown in the dialog (`update/tests.rs::deleting_a_thread_always_goes_through_the_warning`).

## Tests
- `crates/urushi-app/src/update/tests.rs`: connect, attach, resize, screen updates, typing vs shortcuts, the deletion flow, dialog editing, connect-required mode, selection and copy.
- `crates/urushi-app/src/terminal.rs`: pane geometry, dividers, selection text, painting.
- `crates/urushi-ui/tests/layout.rs` and `crates/urushi-render/tests/pixels.rs`: toolkit and GPU.

## Limitations and follow-ups
- Mouse reporting to terminal apps (e.g. clicking in `vim`) isn't forwarded yet.
- No tab renaming in the UI (the protocol and CLI support it).
- The mobile soft keyboard relies on winit's IME support. A touch key bar provides Esc, Tab, Ctrl chords and arrows.
