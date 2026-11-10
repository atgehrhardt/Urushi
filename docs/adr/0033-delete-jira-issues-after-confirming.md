# 0033. Delete Jira issues from Urushi, with their sub-tasks, after confirming

- **Status:** Accepted; amends [0020](0020-jira-boards-as-daemon-mirrors.md)
- **Date:** 2026-11-10

## Context
ADR 0020 left deleting issues to Jira. A Jira card or subtask had no Delete in Urushi; the
daemon refused `DeleteTask`/`DeleteSubtask` on Jira boards with `FAILED_PRECONDITION`.
Users manage Jira boards from Urushi and asked to delete tasks and subtasks there too
(UR-43). Every other edit to a Jira issue already writes through to Jira.

Deleting a Jira issue is not like deleting a task of Urushi's own. It is permanent, and it
removes the issue for everyone on the Jira project, not only from this board. Jira won't
delete an issue that has sub-tasks unless asked to delete them too. Many sites limit the
"Delete issues" permission to admins.

## Decision
- **Deleting writes through, like every edit.** `DeleteTask` and `DeleteSubtask` on a Jira
  board delete the issue in Jira (`DELETE /rest/api/3/issue/{key}?deleteSubtasks=true`).
  The daemon then takes it off the board with `Board::forget_issue`. This runs under the
  board's lock, so a sync can't merge the issue back as it goes. An issue that Jira
  answers 404 for (already deleted) is taken off too. If Jira refuses, nothing changes
  here and the client is told why (`PERMISSION_DENIED` for a missing permission).
- **Sub-tasks go with their issue.** The question says how many.
- **Clients always ask first.** In the app, Delete on a Jira issue asks in place of the
  editor's buttons, or in a dialog from the card's menu. Enter doesn't confirm.
  `urushi board rm-task` and `board sub rm` ask too; `--yes` skips the question, and
  without a terminal they need `--yes`. Tasks of Urushi's own are still deleted at once.
- **Detected by version.** Protocol 1.21 adds no messages. Clients offer Delete on a Jira
  issue only to daemons at 1.21 or later.

## Alternatives considered
- **Keep leaving it to Jira.** This is the status quo that UR-43 asks to change: a trip
  to Jira for a routine clean-up.
- **Delete at once, like local tasks.** This is consistent, but a misclick would destroy
  shared work that Urushi can't restore.
- **Refuse issues that have sub-tasks.** Safer, but it makes the user delete each
  sub-task first. A question that names the count protects just as well.
- **Hide Delete when the account lacks the permission.** That needs another permission
  request per project (`/mypermissions`), and permissions can change. Jira's own refusal,
  shown as the error, is accurate and costs nothing until it happens.
- **A daemon-side confirmation token** (like `PlanThreadDeletion`). Thread deletion
  removes local branches that Urushi decides to remove. Here Jira decides what goes and
  enforces its own permissions. Asking in each client is enough, and needs no wire
  change.

## Consequences
- A daemon can now delete shared Jira data. The question in each client is the guard:
  any new client (mobile, a script) must ask too.
- `Board::remove_task` still refuses on Jira boards. `forget_issue` is the only way a
  Jira task leaves its board apart from a sync, and it runs only after Jira has deleted
  the issue.
- Deleting an epic leaves its stories without an epic in Jira. The board shows that
  after the sync that `poke` starts.
