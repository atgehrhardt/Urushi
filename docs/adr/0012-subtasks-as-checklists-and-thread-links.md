# 0012. Keep subtasks as a task's checklist, and let the daemon link threads to tasks

- **Status:** Accepted
- **Date:** 2026-11-07

## Context
Tasks on a board need smaller steps, and users want to start a thread (a worktree for an
agent) straight from a task or one of its steps, then find that thread again from the
task. Two questions follow: what a subtask is, and where the task ↔ thread link lives and
who makes it.

## Decision
- A **subtask is a checklist item inside its task** (`Task.subtasks: Vec<Subtask{id,
  title, done, thread}>`), one level deep. It moves and is deleted with its task.
- The **link is stored on the task or subtask** (`thread: Option<ThreadId>`), not on the
  thread. It is set by the daemon as part of creating the thread
  (`CreateThread{task_id, subtask_id}`), in the same state mutation that records the
  thread, and cleared when the thread is deleted.

## Alternatives considered
- **Subtasks as full tasks with a parent id.** They would show up as cards, need column
  rules (can a child be "Done" while its parent is "To do"?), and make every board
  operation reason about trees. A checklist covers "smaller steps" without that.
- **A `task_id` on the thread.** Threads would carry board concepts, and one task
  couldn't easily show which threads belong to it and its subtasks without a scan of all
  threads. Keeping it on the board keeps threads unaware of boards.
- **The client links after creation** (`CreateThread`, then an `UpdateTask` with the
  thread). Two requests can half-succeed (a crash or disconnect in between leaves an
  unlinked thread), and the client would be doing workspace bookkeeping, against ADR 0002.

## Consequences
- Linking is atomic with thread creation, and works the same from the GUI and the CLI
  (`urushi thread new --task`).
- A link can point at a thread that no longer exists if it is removed some other way;
  clients treat a link to an unknown thread as no link.
- Reordering subtasks, nesting deeper, or many threads per task would need new fields
  and requests.
