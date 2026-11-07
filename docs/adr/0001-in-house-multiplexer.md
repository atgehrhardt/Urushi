# 0001. Build an in-house, headless terminal multiplexer

- **Status:** Accepted
- **Date:** 2026-11-06

## Context
Each tab is a tree of split terminal panes, and sessions must be **recoverable**: they survive window close, app crash and client disconnect. The multiplexer must be lightweight, easy for us to modify, permissively licensed, and work on macOS, Linux and Windows.

## Decision
The multiplexer is a **headless** component inside `urushid`, not a user interface:

- Each pane is a Session actor that owns a PTY (`Pty` trait, adapter: `portable-pty`) and a VT emulator (`Emulator` trait, adapter: `alacritty_terminal`).
- Layout trees live in `urushi-core`.
- Clients attach and detach freely. The GUI owns all user interaction: splitting, resizing and focus are GUI actions.

## Alternatives considered
- **tmux:** the ISC license is fine. But it doesn't run on Windows. We would have to drive it through control mode or fork ~70k lines of C to change behaviour. And our GUI already provides the UI that tmux duplicates.
- **Zellij:** MIT, but heavier, with a plugin runtime we don't need.
- **GNU screen, dtach:** GPL, so excluded.
- **abduco, shpool:** only keep sessions alive. No splits, no screen model, no Windows support.

## Consequences
- We own a few thousand lines of code: session actor, diffing, key encoding.
- Emulator and PTY are swappable behind traits. Replacing `alacritty_terminal` with our own emulator later is a local change.
- Because the screen state lives in the daemon, reattaching shows exactly what was on screen, and remote/mobile clients come almost for free.
