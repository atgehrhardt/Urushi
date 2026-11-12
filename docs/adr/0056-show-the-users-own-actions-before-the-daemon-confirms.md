# 0056. Show the user's own actions before the daemon confirms them, without deciding anything in the GUI

- **Status:** Accepted
- **Date:** 2026-11-12

## Context
The daemon is the single source of truth (ADR 0002), so the GUI changed only when the
daemon's workspace or reply came back. Several actions felt frozen while they waited
(UR-68):
- the New Thread dialog sat open during a fetch and a checkout;
- a dragged divider moved only after a round trip and a save per pointer move;
- a split or a new tab appeared only after a shell had started;
- the delete warning opened only after several git commands;
- dialogs closing on reply could be submitted twice.

## Decision
The GUI shows what the user asked for at once, as a preview it drops when the daemon
answers. It never applies a rule of its own:

| Action | Shown at once | Until |
|---|---|---|
| New Thread | The dialog closes. A "creating" row in the sidebar and a "Creating …" screen (`creations.rs`). | The reply. The thread opens if its placeholder is still shown. A failure reopens the dialog as submitted. |
| Divider drag | The daemon's layout with the core's own `PaneGroup::move_divider` applied. One move in flight; the newest position waits (`divider.rs`). | The daemon's workspace shows the drop, or the second workspace after the last reply. |
| Split | The core's own `PaneGroup::split`, with an empty placeholder pane that is never attached or clicked (`starting.rs`). | The workspace has the new pane. |
| New tab | A "Starting …" chip in the tab bar. | The reply. |
| Delete thread | The warning, saying it is checking. Delete waits for the plan. | The plan arrives. |
| Dialogs closed on reply | "Saving…". A second submit does nothing (`update/saving.rs`). | The reply. It closes only a dialog of the kind that was saved. |

A repeated split waits for the first one to arrive.

## Alternatives considered
- **The daemon publishes a thread in `Creating` before its worktree exists:** other
  clients would see it too, but it bends invariant 8. A failure would show a thread
  appearing and vanishing on every client.
- **Optimistic changes with the GUI's own layout rules:** this duplicates daemon logic
  (ADR 0002). The core's pure operations give the same result with no second copy.
- **Spinners only:** they show that something is happening, but the divider and splits
  would still lag behind the hand.

## Consequences
- Only the client that acted sees the preview; other clients see the result.
- Each preview needs a rule for when it ends. A preview that outlived its reply would
  show a state the daemon doesn't have, so every preview also ends on a refusal, and
  the layout previews also end at the second workspace after their reply.
- Placeholders are local state in `AppState` (`creations`, `divider`, `starting`,
  `submitting`), tested through `update()`.
