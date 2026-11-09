# Feature: Split-pane layouts

- **Status:** Shipped
- **Owners (crates):** `urushi-core` (`layout`, `tab`), `urushi-daemon`, `urushi-app`
- **Related ADRs:** [0001](../adr/0001-in-house-multiplexer.md), [0002](../adr/0002-daemon-source-of-truth.md)

## What it does
Each tab is split into any arrangement of terminal panes. Users split right/left/up/down, drag dividers, resize from the keyboard, equalize, and move focus by direction.

## How it works
- `urushi_core::Layout` is an n-ary tree. Splits have an `Axis` (`Row` or `Column`) and child ratios.
- Operations live in `layout/ops.rs`: `split`, `close`, `resize`, `move_divider`, `equalize`.
- `Layout::geometry(bounds)` returns pane rectangles plus `Divider`s. The GUI draws from these and maps a divider drag to `move_divider(anchor, axis, fraction)`.
- `focus_neighbour` picks the next pane spatially, not by tree order.
- The daemon owns the authoritative `Tab` (layout + focus) and broadcasts changes. Clients send intents (`SplitPaneRequest`, `MoveDividerRequest`, …).
- When a pane closes (closed by the user, or its process exited), `Tab::close_pane` hands focus to the pane that took its space.

## In the GUI
- **Pane menu:** a right or two-finger click on a pane focuses it and opens its context menu: Split right, Split down, Copy, Paste, Close pane. This happens even when the program in the pane takes the mouse; only the middle button still goes to the program. Split and close name the menu's pane (`PaneMsg` in `msg/pane.rs`), so they act on that pane even if focus moves while the menu is open. Logic: `update/panes.rs`, view: `view/menu/pane.rs`.
- **Keyboard:** Cmd (macOS) / Ctrl+Shift (elsewhere) + D splits right, Shift+D (macOS) / E splits down, W closes the focused pane, and Alt+arrows move focus.
- **Mobile:** touch screens have no secondary click (`platform::HAS_SECONDARY_CLICK`), so the tab bar keeps **→ ↓ ×** buttons that act on the focused pane. Desktop has no such buttons.
- **Local focus:** a click or a split focuses a pane at once, before the daemon confirms it (`AppState::focused_override`). The override is dropped as soon as the daemon reports the same focus, or when its pane is gone. `focused_pane()` also ignores an override whose pane isn't in the tab. So after a pane closes, focus, typing, splits and closes go to the pane the daemon chose, never to the closed one.

## Invariants
Documented in `layout/mod.rs`, checked by `Layout::check_invariants`:
- at least one pane, and pane ids are unique
- every split has two or more children
- ratios are positive and sum to 1
- no split directly contains a split on the same axis (it is flattened)

Deserialization validates the tree. A tab's focused pane is always in its layout.

## Tests
- Unit tests: `crates/urushi-core/src/layout/tests.rs`, plus tests in `tab.rs`.
- Property tests: `crates/urushi-core/tests/layout_props.rs`. Random sequences of operations must keep all invariants and tile the unit square.
- GUI: `crates/urushi-app/src/update/tests/panes.rs` (the pane menu, and focus after a pane closes or exits); the `pane-menu` snapshot scenario (`snapshot/tests.rs`).

## Limitations and follow-ups
- A divider is identified by an anchor pane, not by path, so it stays stable while other clients edit the layout at the same time.
- There is no "zoom pane" yet.
