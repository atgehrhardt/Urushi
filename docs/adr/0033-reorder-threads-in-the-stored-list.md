# 0033. Reorder threads by moving them in the one stored list, and let a drop join the target's group

- **Status:** Accepted
- **Date:** 2026-11-10

## Context
Threads couldn't be reordered in the sidebar (UR-45). The daemon stores all threads in one
list (`State.threads`, sent as `Workspace.threads`), and the sidebar shows each project's
threads in that list's order, split into groups: the parent-branch thread, pinned
threads, one section per status, then threads without a status. Projects (`MoveFolder`)
and boards (`MoveBoard`) are already reordered by moving them in their stored lists.

Two questions: what a reorder request names, and what happens when a thread is dropped
on a thread in another group (another status, pinned or not, another project).

## Decision
- **`MoveThread{thread_id, index}`** (protocol 1.21) moves a thread in the one stored
  list, the index counted without it and clamped to the end, exactly like `MoveFolder`.
  The client computes the index the way it does for projects: the dragged thread takes
  the target's place (`organize::thread_drop_index`). Each group keeps the list's order,
  so the move shows within the group, and the order is saved with the workspace.
- **A thread dropped on another joins the target's group:** the client sends one
  `UpdateThread` with the project, pin and status that differ (`organize::join_group`),
  then the `MoveThread`. The two are independent: the list's order doesn't depend on
  the groups. A thread dropped on a pinned thread is pinned and keeps its status, since
  pinned threads aren't grouped by status. The parent-branch thread takes no drops.

## Alternatives considered
- **An index within the thread's project, or within its group.** The daemon would have
  to repeat the sidebar's grouping rules (pins, statuses, missing projects) to map it to
  the stored list, which puts view logic in the daemon and breaks when the grouping
  changes. The global index needs no grouping knowledge.
- **`before_thread_id` instead of an index.** Robust against concurrent edits, but the
  other reorder requests take an index; one rule for all of them is simpler for clients.
- **Only reorder within the thread's own group**, ignoring drops on other groups. The drop
  line would show a place the thread never goes to. Joining the group puts it where the
  line shows, which is what the user picked.
- **One request that also sets the group.** Would make the drop atomic, but duplicates
  `UpdateThread`; a failed `UpdateThread` (e.g. a project deleted meanwhile) only leaves
  the thread in its old group, which is harmless.

## Consequences
- The order between threads in different groups is kept too, so a thread that changes
  group later lands where the list puts it, not at the group's end.
- A drop is two requests; a client that sees the first `WorkspaceChanged` briefly shows the
  thread in its new group before it moves.
- Older daemons (minor < 21) get no thread drop targets and no Move up / Move down.
