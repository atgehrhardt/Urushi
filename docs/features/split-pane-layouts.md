# Feature: Split-pane layouts

- **Status:** In progress (model done, GUI pending)
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

## Limitations and follow-ups
- A divider is identified by an anchor pane, not by path, so it stays stable while other clients edit the layout at the same time.
- There is no "zoom pane" yet.
