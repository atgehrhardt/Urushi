# 0044. Let the client a person last clicked, tapped or typed in decide a shared pane's size

- **Status:** Accepted
- **Date:** 2026-11-11

## Context
A pane has one terminal size, and the program in it, such as a shell or an agent's TUI, lays out for that size. The same pane is often shown on two clients at once: the desktop app and a phone. Each client sizes the pane to its own rectangle.

The daemon applied whichever `SetPaneSize` or `AttachPane` came last. A client only sent a new size when its *own* rectangle changed. So after the phone shrank a pane, the desktop drew the phone-sized grid until its user switched threads, which re-attached the pane. The same happened the other way round (UR-56).

The user wants the pane to follow the client a person is using, *the moment* they click, tap or type in it. They don't want it to follow anything an agent does.

## Decision
- The daemon keeps one size per pane, the last one asked for, as before.
- The client claims the size back on human interaction with its terminals:
  - a press or tap in the terminal area (`area_press`);
  - a right or middle click on a pane (`area_button`);
  - a key sent to a terminal (`on_key`);
  - text typed or pasted into a terminal (`insert_text`).
- Claiming (`AppState::claim_sizes`) sends `SetPaneSize` for each pane on screen whose size, as the daemon last reported it in `ScreenUpdate.cols/rows`, differs from the client's grid. When the sizes already match, nothing is sent.
- Hovering, the wheel, focus changes and incoming frames never claim.
- No protocol change.

## Alternatives considered
- **Daemon-side: resize to the sender's size on any input.** The daemon can't tell a person's keystrokes from an agent's. `urushi send`, agent launch and task-prompt pastes all arrive as `Input`. Agents would trigger resizes, which the user ruled out. It would also need each connection's size, which is a protocol change.
- **Smallest size wins (tmux's default):** every client sees the whole terminal, but the desktop would be stuck at phone width while the phone is connected.
- **Re-send whenever the daemon reports a different size:** two visible clients would resize the pane back and forth endlessly.
- **Claim on window focus:** a window can gain focus without the person meaning to use the terminal, e.g. to read the board. Click, tap and typing are explicit.

## Consequences
- The client whose terminal was used last decides the size. The other client draws the daemon's grid as it is (clipped, or with space to spare) until a person uses it.
- One click claims every pane on screen, so a split tab is whole again at once.
- Any new human input path into a terminal must call `claim_sizes`. Tests in `update/tests/sizing.rs` cover the existing paths.
