# 0020. Follow Jira boards from the daemon: Jira is the source of truth, edits write through, polling keeps them current

- **Status:** Accepted; its polling and write-through moves are amended by [0023](0023-near-real-time-jira-sync.md), and epics are cards since [0027](0027-epics-are-cards-on-jira-boards.md), and issues are deleted from Urushi since [0033](0033-delete-jira-issues-after-confirming.md)
- **Date:** 2026-11-09

## Context
Users want any Jira Cloud board as a task board in Urushi: its columns, issues, epics,
stories, bugs and subtasks, kept up to date as Jira changes, and edits made in Urushi
(moving a card, renaming, notes, labels, checking a subtask off) reflected in Jira.

Boards live in the daemon (ADR 0002), and clients may be phones on a tailnet. The daemon
runs on a laptop or a home server with no public address, so Jira's webhooks can't reach
it. Jira workflows decide which status changes are allowed, and a site may rename issue
types or add statuses. Jira boards can hold thousands of issues, while every workspace
change resends the whole workspace to clients and rewrites the JSON document (ADR 0008).

## Decision
- **A Jira board is a mirror kept by the daemon.** `Board.source` says which Jira board it
  follows; each column records the Jira status ids it shows; each task records the issue
  it shows (`Issue`: key, type, status, assignee, priority, epic, rank). Epics aren't cards:
  the board keeps their names and colors, and cards name them. Jira subtasks are nested
  subtasks (ADR 0015).
- **Jira is the source of truth.** A pure merge in `urushi-core`
  (`Board::merge_issues`) places every task from its issue: the column showing its
  status, in the board's rank order. It matches tasks by issue id, so a task keeps its id,
  thread link and color (Urushi's own fields) across syncs. Changes Jira owns are refused
  on these boards (`BoardError::FollowsJira`): columns, the board's name, nesting,
  deleting issues and attachments.
- **Edits write through.** A change to a Jira task is made in Jira first (edit, transition,
  create, rank); then the issue is read back and merged. If Jira refuses, nothing changes
  here and the client is told why. A move picks a transition to a status the destination
  column shows; if the workflow has none, the move is refused.
- **Polling, not webhooks.** The daemon reads issues updated in the last few minutes every
  15 s while a client watches the workspace and every 2 min otherwise, and reads the whole
  board (configuration included) every 5 min, on import and on `SyncBoard`. A merge that
  changes nothing wakes no client. A rate-limited Jira is left alone for as long as it
  asks; failures back off and are shown on the board.
- **Bounded.** At most 500 tasks per board; done issues only from the last 14 days (as
  Jira's kanban boards show them); scrum boards show their open sprints. What doesn't fit
  is counted on the board.

## Alternatives considered
- **Webhooks.** Instant, but the daemon has no public HTTPS address. Exposing one (Tailscale
  Funnel, a relay) adds setup and an internet-facing listener for every user. Polling can
  be joined by webhooks later without changing the model.
- **Optimistic local edits with a queue.** Cards would move instantly, but edits Jira
  refuses (workflow rules, validation) would have to be undone after the fact, and offline
  queues need conflict resolution. Writing through keeps one truth; a move settles in about
  a second.
- **Import once (a copy).** Simple, but the board goes stale immediately; the user asked for
  sync.
- **Epics as cards with their stories nested.** Matches Jira's hierarchy, but hides each
  story's real column inside its epic. Epics as chips and a filter keep the board readable.
- **Sync in clients.** Every client (phones included) would need Jira credentials and the
  logic, against ADR 0002.

## Consequences
- Jira boards look and behave like other boards, with a kind marker, issue key, epic chip
  and assignee on cards, and a read-only Jira section in the editor.
- Changes made in Jira appear within about 15 s while the app is open.
- The daemon depends on Jira Cloud's REST APIs (`/rest/api/3`, `/rest/agile/1.0`). If they
  change, `jira/cloud.rs` and `jira/parse.rs` change; parsing is lenient so odd fields read
  as empty.
- Editing a rich-text description through plain notes loses its formatting; notes are only
  written back when changed, and descriptions too long to show whole are read-only.
- Data Center (self-hosted Jira), OAuth sign-in and Jira attachments are not supported yet.
