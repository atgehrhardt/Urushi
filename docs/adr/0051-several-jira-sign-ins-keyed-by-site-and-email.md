# 0051. Keep several Jira sign-ins, keyed by site and email, and tie each imported board to one

- **Status:** Accepted
- **Date:** 2026-11-12
- **Amends:** [0021](0021-https-through-curl-and-a-private-credentials-file.md) (the credentials file's shape)

## Context
The daemon kept one Jira sign-in (ADR 0021): signing in again replaced it, and every
Jira board was read and written with it. People work with more than one Jira (a client's
site and their own, or two accounts on one site), and want to import boards from each
(UR-61). The Jira client already took the sign-in as an argument on every call
(`JiraApi`), so only the daemon's state, the file it is kept in, the boards and the wire
had a single sign-in built in.

## Decision
- **A sign-in is named by its site and email:** `Account::key()` is
  `<site host>/<email in lower case>`, e.g. `acme.atlassian.net/ada@acme.com`
  (`JiraAccount.key` on the wire). It is known before Jira is asked. Signing in again with
  the same site and email replaces that sign-in (a new token); anything else adds one.
- **The file holds a list:** `<home>/jira-credentials.json` becomes
  `{"accounts": [...]}`, still 0600 and replaced atomically. A file written before this,
  holding one bare account, is read as a list of one and rewritten in the new shape at the
  next change. Signing out of the last sign-in removes the file.
- **Each board names its sign-in:** `JiraBoard.account` (`JiraBoardSource.account`) is the
  key, set at import. A board imported earlier has none and uses the first sign-in for its
  site (`service::jira::account_for`), so existing boards keep working with no migration.
  Every read and write of a board (sync, edits, moves, creating and deleting issues, quick
  filters, issue details, attachments, pull requests) uses its board's sign-in. Searching
  people for a new issue (no task yet) takes the board (`SearchJiraUsers.board_id`).
- **Signing out is per sign-in:** only the boards of that sign-in stop syncing, and say
  `Not signed in to Jira at <host>`. Boards with no sign-in aren't due for syncing.
- **Per-issue caches are keyed by board** (field kinds, when pull requests were last
  read), as two sites can share an issue key or id.
- **Wire (1.29), additive:** `ListJiraAccounts` → `JiraAccountList`, and an `account` on
  `DisconnectJira`, `ListJiraBoards` and `ImportJiraBoard`. Empty means the first
  sign-in, which is exactly what an older client meant. `GetJiraAccount` answers the
  first. Clients treat an older daemon's one sign-in as a list of one.
- **App:** Settings lists every sign-in with its own **Disconnect**, and **Add Jira
  account…**. The import dialog picks the site to list (a chip per sign-in) and offers
  **+ Account**. A board decides between **Sync now** and **Connect…** (pre-filled with its
  site and email) by its own sign-in, and "Only my issues" and "Assign to me" use the
  account id of the board on screen's sign-in.

## Alternatives considered
- **Key by site only:** two accounts on one site (a personal and a work login) would
  collide, and the site alone doesn't say which account to sign back in with.
- **Key by Atlassian account id:** it is known only after Jira answers, and it is the same
  for one person on every site, so it doesn't tell sites apart.
- **A generated id per sign-in:** it would need a lookup to show what it is, and signing
  in again with the same site and email would add a duplicate unless matched by site and
  email anyway.
- **Move credentials to the OS keychain at the same time:** worthwhile, but a separate
  decision with per-platform work. ADR 0021's file stays, now holding a list.

## Consequences
- A daemon from before 1.29 can't read a file holding a list, and would act as signed out
  if the user went back to it. Downgrades are rare, and signing in again fixes it.
- Every Jira code path now resolves a board's sign-in first. A new Jira feature must do
  the same (`Service::jira_account(board)`), never "the" sign-in.
- Two boards following the same Jira board on one site, through two sign-ins, aren't
  possible: importing it again answers the board already there.
