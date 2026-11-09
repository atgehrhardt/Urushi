# 0015. Make subtasks full tasks nested inside their parent

- **Status:** Accepted
- **Date:** 2026-11-09
- **Supersedes:** the checklist part of [0012](0012-subtasks-as-checklists-and-thread-links.md)
  (the daemon-made thread links stand)

## Context
ADR 0012 made a subtask a checklist item: a title, a check mark and a thread link. Users
now want to open a subtask as a card of its own, with notes, a color, tags and
subtasks of its own, to make any card a subtask of another, and to split a subtask back
out onto the board. A checklist item can't hold any of that, and the two kinds of item
can't be turned into each other without copying.

## Decision
- A **subtask is a `Task`** kept in its parent's `subtasks`, so the board is a forest:
  columns hold top-level tasks, and each task holds its subtasks in order. A task is in
  exactly one place, either a column or one parent, and it keeps its id when it moves
  between them.
- A nested task has a **done** check mark; a top-level task's progress is its column.
  Nesting a card that was in the board's last column marks it done; moving a subtask
  onto the board clears it.
- **Moving** is two operations: `MoveTask` into a column (which now also takes a nested
  task out of its parent) and `NestTask` under a parent. Nesting refuses a task's own
  subtree (no cycles), keeps within one board, and is bounded: at most `MAX_DEPTH`
  levels and `MAX_SUBTASKS` children each.
- On the wire, the existing `Subtask` message gains the task fields and its own
  `subtasks`, so older clients still read the first level as a checklist. Subtask ids
  are task ids, and every task request accepts a nested id.
- Thread links stay on the task (0012), at any depth.

## Alternatives considered
- **Cards with a parent id, shown on the board.** A subtask would be a card in a column
  and also part of its parent. Rejected again for 0012's reasons: column and done state
  can disagree, and every board operation must reason about trees spread over columns.
- **Keep checklists and add "convert to task".** Two kinds of item with copy-based
  conversion loses ids and thread links, and still can't give a subtask subtasks.
- **A new nested message on the wire** (`Task.children`). Older clients would show no
  subtasks at all; extending `Subtask` keeps them working.

## Consequences
- Old documents load unchanged: a checklist item is a task with defaults for the new
  fields.
- Board operations walk a tree (bounded by `MAX_DEPTH`), and property tests cover that
  nest, move and remove never lose, duplicate or cycle a task.
- The task editor navigates into subtasks and back.
- Reordering subtasks within a parent is still not offered.
