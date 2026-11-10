# 0027. Show a Jira board's epics as cards, and create them like any issue

- **Status:** Accepted; amends [0020](0020-jira-boards-as-daemon-mirrors.md)
- **Date:** 2026-11-10

## Context
ADR 0020 kept epics off a Jira board's columns: the merge dropped epic issues, kept only
their key, summary and color in `Board.epics` for the chips on their stories and the Epic
filter, and forgot an epic once no story named it. The editor's **Type** didn't offer
Epic on Jira boards, and the daemon refused to create one.

Users plan work by creating an epic first, then its stories (UR-12). Under 0020 they
couldn't create an epic here. Even if they could, a new epic has no stories, so it would
have disappeared from the board as soon as Jira answered.

## Decision
- **Epics are cards.** `Board::merge_issues` places an epic like any issue: in the column
  that shows its status, in rank order. An epic has no `Issue.parent` (a story's epic is
  `Issue.epic`), so it is always a card and never a subtask. Its stories stay in their own
  columns with the epic chip.
- **`Board.epics` still names them.** It lists every epic that is a card on the board or
  that a task names, so chips and the filter keep their labels. An epic card passes its
  own Epic filter, so filtering on an epic shows the epic with its stories.
- **Created like any issue.** **+** on a column with Type **Epic** creates an issue of
  the project's level-1 type, moved to that column. The fields Jira asks for come first,
  as for other types.
- **No sub-tasks under epics.** In Jira, an epic's children are standard issues; sub-tasks
  sit under those. The daemon refuses sub-tasks for an epic (`INVALID_ARGUMENT`) before
  asking Jira, and the editor says "Jira epics hold stories, not sub-tasks." instead of
  offering one.
- **Detected by protocol 1.15.** There is no wire change, but older daemons refuse epics,
  so apps offer Epic on Jira boards only when `protocol_minor ≥ 15`.

## Alternatives considered
- **A separate epics list (a menu in the board bar with "New epic…").** It matches how
  Jira's own kanban boards keep epics in a panel. It loses because it needs a new place in
  the UI and protocol additions to return and open an issue that isn't a task, for little
  gain over a card.
- **Create only, with a notice.** This is the smallest change. It loses because the new
  epic wouldn't show in Urushi until a story named it.
- **Nest stories under their epic card.** This mirrors Jira's hierarchy, but it hides
  each story's real column (the reason 0020 gave against it).

## Consequences
- Every epic the board's filter includes now shows as a card and counts toward the
  board's 500-task bound. Boards whose filter leaves epics out can't show new epics, and
  creating one there reports "Jira made KEY, but this board doesn't show it".
- An epic card can be moved, filtered, edited in full and used to start a thread, like any
  issue. Its editor doesn't list its stories; the Epic filter shows them.
- `jira_create.rs` and the editor's subtask section must keep honouring the "no sub-tasks
  under epics" rule.
