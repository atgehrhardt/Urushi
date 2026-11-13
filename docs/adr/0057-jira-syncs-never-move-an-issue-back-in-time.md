# 0057. Never let a Jira sync move an issue back to an older version

- **Status:** Accepted
- **Date:** 2026-11-13

## Context
After a move or edit, the daemon reads the issue back with `GET /issue/{key}`, which is
up to date. It merges that, then pokes a sync (ADR 0023). Syncs read through
`/rest/api/3/search/jql`. Jira Cloud's search index lags behind its writes by a few
seconds, so the poked sync saw the issue's *previous* update time.

The look at what changed counted any time that differed from the last one seen (`!=`).
It therefore re-read the old issue and merged its old status and rank, and the card
jumped back. A few seconds later the index caught up and the card moved forward again.
A full read could do the same. UR-71 reported this as "slow and glitchy".

## Decision
A sync never replaces an issue's version with an older one (`service/jira_sync.rs`):
- **Only newer times count.** A look counts an issue as changed only when its update
  time is newer than the one seen. `BoardSync.seen` only moves forward (`BoardSync::see`).
- **Older versions are kept out of merges.** An issue that a read returns older than seen
  is left out of the merge and its id goes into `Read.keep`.
  `Board::merge_issues_keeping` keeps those records even when a full snapshot lacks
  them.
- **Recent writes are reconciled.** Issues written in the last `RECONCILE_FOR` (60 s,
  `BoardSync.written`, noted by `saw`) are sent as `reconcileIssues` (at most 50). Jira
  takes that only in a body, so those searches are a `POST /rest/api/3/search/jql`
  (`jira/cloud/search.rs::search_page`). Jira then answers with those issues as they
  are now.

The app also shows a dropped card where it was dropped until the daemon answers
(`moving.rs`, as ADR 0056 does for dividers), so the wait for Jira doesn't show.

## Alternatives considered
- **Don't poke a sync after a write:** the poke is there so that changes made by Jira's
  automation (a parent's status) show quickly. A later look can still meet the lagging
  index.
- **Only `reconcileIssues`:** Atlassian guarantees it only for the ids named, and at most
  50 of them. The newer-time guard also covers a reconcile Jira ignores and writes more
  than a minute old.
- **Ignore just-written issues for a fixed time:** this would also hide genuine changes
  made in Jira during that time. Update times say exactly which version is newer.
- **Webhooks:** they need an app registered with the site and a reachable address,
  which a desktop daemon doesn't have (ADR 0023).

## Consequences
- An issue moved to a status no column shows, or deleted, still leaves at the next full
  read that lacks it: only issues the read *returned* older are kept.
- A kept sub-task follows its parent: if the parent leaves the board, so does it.
- The seen times are per daemon run, as before. A restart reads everything once.
