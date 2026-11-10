# 0037. Write quick filters to Jira through its board-settings endpoint, and keep them here when Jira refuses

- **Status:** Accepted
- **Date:** 2026-11-10

## Context
Users want to make quick filters the way Jira does: named filters shown as chips above
a board, each turned on and off with a click (UR-14). On a Jira board they should sync,
meaning a quick filter made in Urushi shows on the board in Jira too. On a board of
Urushi's own there is nothing to sync with.

Jira boards already show Jira's own quick filters. They are read with the agile API
(`GET /rest/agile/1.0/board/{id}/quickfilter`), and Jira evaluates their JQL
(`QueryJiraFilter`, ADR 0020). Jira Cloud documents no way to *write* quick filters.
The agile API only reads them, and Atlassian has an open request to add writes
(JSWCLOUD-21718). Jira's own board settings page writes them through an internal endpoint,
`/rest/greenhopper/1.0/quickfilters`. It takes the same Basic sign-in but is undocumented,
may change, and is only open to board admins.

Every sync replaces the board's list of Jira quick filters with what Jira returns, so
anything kept only in that list would be lost.

## Decision
- **A Jira board's quick filters are written to Jira** through the board-settings
  endpoint: create, edit and delete. Every request it makes lives in one file,
  `jira/cloud/quick_filters.rs`. After each write the daemon reads the list back with
  the agile API, under the board's lock. It stores the list in the board's mirror and in
  the cached configuration that syncs copy it from, so the next sync agrees.
- **JQL is checked first**, with a one-issue search of the board (`issue_ids`). JQL Jira
  can't read is refused as `INVALID_ARGUMENT` and never kept.
- **If Jira won't take a new filter, the board keeps it.** That covers `404` (the endpoint
  is gone), `403` (not a board admin) and any other `4xx`. The board stores it in
  `Board.quick_filters` as JQL that Jira still evaluates, and the answer
  (`QuickFilterSaved.kept_here`) says why, so the app can tell the person. Sign-in
  failures, rate limits and Jira being out of reach are errors, not reasons to keep a
  filter here, because trying again later is right. Editing or deleting a Jira filter
  has no fallback; Jira's refusal is shown.
- **A board of Urushi's own keeps its quick filters itself** (`Board.quick_filters`). It
  filters by fields every task has: a search, kinds and tags. Clients evaluate these
  rules, as they do the rest of the filter. `urushi-core` refuses a rule that doesn't
  suit the board: JQL off Jira, or fields on Jira.
- **Filters are defined from the selection.** "Save as quick filter…" turns what is chosen
  on the board into JQL (Jira) or a search, kinds and tags (others). On a Jira board the
  JQL is shown and can be edited before saving.
- Wire: protocol 1.23 adds `CreateQuickFilter`, `UpdateQuickFilter`,
  `DeleteQuickFilter`, `Board.quick_filters`, `JiraQuickFilter.jql` and
  `QueryJiraFilter.kept_quick_filter_ids`.

## Alternatives considered
- **Keep every quick filter in Urushi, with JQL that Jira evaluates.** This relies only on
  documented APIs, but the filter never shows in Jira. The issue asked for sync, and this
  remains the fallback.
- **A Jira saved filter (`POST /rest/api/3/filter`) behind each chip.** That API is
  documented, but the result is a site-wide saved filter, not a quick filter on the
  board. Someone would still have to add it to the board by hand.
- **Write through the internal endpoint with no fallback.** This is simpler, but when the
  endpoint changes or the person isn't a board admin, nothing can be saved at all.
- **Accept JQL without checking it.** A typo would be written to Jira, or kept here, and
  then match nothing, which is confusing.
- **Store quick filters in each client.** Every client would need its own copy, against
  ADR 0002. Phones and desktops wouldn't agree.

## Consequences
- Quick filters made here show on the board in Jira for board admins. They come back on
  every sync, and Jira's own edits arrive the same way.
- The daemon depends on an undocumented Jira endpoint. If Atlassian changes it, writes
  start failing. New filters then fall back to being kept here, and edits and deletes
  show Jira's error. Only `jira/cloud/quick_filters.rs` needs to change, and its tests
  pin the requests it sends.
- A Jira board can show quick filters from two places: Jira's own, and those kept here.
  Both evaluate through `QueryJiraFilter`.
- Clients evaluate local boards' rules themselves, which keeps the daemon's filtering
  code limited to JQL.
