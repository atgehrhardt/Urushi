# Feature: Recoverable terminal sessions

- **Status:** Shipped in the daemon, client and CLI. The GUI renderer is pending.
- **Owners (crates):** `urushi-mux`, `urushi-daemon`, `urushi-client`
- **Related ADRs:** [0001](../adr/0001-in-house-multiplexer.md), [0002](../adr/0002-daemon-source-of-truth.md)

## What it does
Every pane is a real process on a PTY, owned by the daemon. Closing the window, the app crashing or the network dropping doesn't stop the process. Reattaching shows exactly what was on screen.

## How it works
- **`SessionHandle::spawn`** starts the process (`Pty` trait, `NativePty` via portable-pty) and an actor task that alone owns the `Emulator` (`AlacrittyEmulator`).
- **Blocking PTY I/O runs on dedicated threads.** The queues between those threads and the actor are bounded.
- **Line versions:** after each output chunk, the actor turns the emulator's damage into per-line versions. Watchers are notified through `watch<SessionStatus>`.
- **Clients pull changes:** a client asks for `frame_since(version)` and gets only the lines changed since then (a `full` frame after a resize or full damage). Combined with the protocol's ack rule (one un-acknowledged `ScreenUpdate` per pane), slow clients get merged updates instead of growing queues.
- **Input encoding happens in the daemon** (`keys::encode_key`, `encode_paste`), because it depends on terminal modes such as application cursor keys and bracketed paste.

## Invariants
- Only the actor touches the emulator: no locks, and no shared mutable state.
- A frame requested from before the last reset is always `full`.
- Pastes can't break out of bracketed-paste mode (embedded end markers are stripped).

## Tests
- `crates/urushi-mux/src/emulator/alacritty.rs`: spans, wide characters, damage, replies, modes, scrollback.
- `crates/urushi-mux/src/keys.rs`: the encoding table.
- `crates/urushi-mux/tests/pty_session.rs`: real processes (output, echo, exit codes, resize, incremental frames, env/cwd).
- `crates/urushi-daemon/tests/e2e.rs`:
  - screen survives a client disconnect
  - no update is sent before an ack
  - exits close panes
  - a daemon restart restores layouts and respawns shells in the worktree

## Limitations and follow-ups
- Mouse reporting to applications isn't encoded yet. The mode is reported, so clients can tell.
- The scrollback position is shared by every client attached to a pane, not tracked per client.
- Processes don't survive a daemon *restart*. Layouts are restored and shells are re-spawned.
