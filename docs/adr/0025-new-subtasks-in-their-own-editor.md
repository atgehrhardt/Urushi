# 0025. Write a new subtask in an editor of its own, kept as a draft until Save

- **Status:** Accepted
- **Date:** 2026-11-10

## Context
Since ADR 0015 a subtask is a full task: notes, kind, color, tags, its own subtasks and,
on a Jira board, a Sub-task issue with its own fields. But a new subtask could only be
added by its title ("Add a subtask…" + Enter); everything else had to be set after it
was created, by saving, reopening the parent and opening the subtask. The editor's rule
is that nothing reaches the board, or Jira, before **Save** (ADR 0020, 0022), and that
moving between a task and its subtasks keeps every editor's unsaved edits.

## Decision
- **New subtask…** opens the ordinary task editor for a new task, stacked on the
  parent's editor through `EditorNav::back` like an existing subtask opened there.
  `EditorNav::draft_of_previous` marks it as a subtask of the editor below, and it has
  no column or subtasks of its own. On a Jira board its type is fixed to Sub-task, and
  its fields come from `GetJiraCreateFields(TASK_KIND_SUBTASK)`.
- **A draft is the whole editor.** Back, Done or opening another task puts it into the
  parent's `SubtaskEditor::drafts` as a `SubtaskDraft` (the `Dialog::Task` itself), so it
  reopens exactly as left, Jira field drafts included. Save from either editor folds
  it in first and saves everything, as before.
- **The protocol carries the details (1.14).** `CreateSubtaskRequest` gains notes, color,
  tags, kind and Jira fields; `CreateTaskRequest.subtask_details` carries `NewSubtask`s
  for a new task. Both are additive: older daemons create the subtask from its title,
  and the app offers **New subtask…** only from 1.14.

## Alternatives considered
- **Create the subtask at once, then open it.** Simplest, but breaks "nothing is saved
  before Save": a subtask would reach Jira half written, and Cancel couldn't undo it.
  It also can't work under a task that doesn't exist yet.
- **More fields inline in the subtask list.** Crowds the parent's editor, and can't show
  a Jira sub-task's own (site-defined, possibly required) fields.
- **Create, then `UpdateTask` from the client.** No protocol change, but two writes per
  subtask (two Jira round trips), non-atomic, and impossible for a new parent, whose id
  the client doesn't have until `CreateTask` answers.

## Consequences
- A draft holds a full editor, so drafts are heavier than titles; there are few per task.
- A draft subtask can't get subtasks of its own before it is created (it has no list);
  open it after Save to nest further.
- Attachments still need the subtask to exist first, as for a new task.
