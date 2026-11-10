# 0036. Show a thread's pull request: read a Jira issue's from Jira's development info, and let a local task link one

- **Status:** Accepted
- **Date:** 2026-11-10

## Context
A thread card should show, and open, the Jira issue a thread was started from and that
work's pull request. The issue is already known, because the task links the thread (ADR
0012). Pull requests are not known.

ADR 0026 has Jira's own code-host integration link branches and pull requests to an
issue by its key, but Urushi never reads that link back. Local boards have no Jira, so
nothing there knows a task's pull request.

## Decision
- **Jira boards:** the daemon reads an issue's pull requests from Jira's development
  info, the data Jira's Development panel shows.
  - It calls `/rest/dev-status/latest/issue/summary`, then
    `…/issue/detail?applicationType=<GitHub type>&dataType=pullrequest` for each GitHub
    host the summary names.
  - The result is kept on the issue (`Issue::pull_requests`). A board sync never
    overwrites it.
- **Only issues linked to a thread are read** (`service/jira_dev.rs`).
  - An issue is read when it is linked, then every 60 s while a client watches the
    workspace and every 10 min otherwise.
  - At most 20 issues are read per round.
  - A failure backs off like a board sync, and is never shown as a board error.
  - An issue that loses its thread loses its pull requests.
- **Local boards:** a task has one optional pull request link (`Task::pr_url`), typed in
  the task editor or with `urushi board edit --pr`. It must be an `https://` URL
  (`PrUrl`), the only kind the app opens.
  - Jira boards refuse a typed link, because Jira already says which pull requests an
    issue has.
- **Display:** clients show Simple Icons' Jira and GitHub marks (CC0-1.0) on the card. A
  click opens the page in the browser. Only GitHub hosts are read, because the mark is
  GitHub's.

## Alternatives considered
- **Call GitHub ourselves (its API or `gh`):** this needs a second sign-in and
  per-repository mapping from branch to pull request. It would also duplicate the
  integration the site already runs, and ADR 0026 rejected it for the same reasons.
- **A typed link on every task, Jira issues included:** people would retype what Jira
  already knows, and the link would go stale when a new pull request is opened.
- **Read development info for every issue on the board:** that costs one or two requests
  per issue per round, against a board sync that reads only what changed. Only linked
  issues appear on a thread card.
- **Jira remote links (`/remotelink`):** GitHub for Jira doesn't write them. They hold
  whatever someone pasted, not pull requests.

## Consequences
- The dev-status endpoints aren't part of Jira's published API and could change. Parsing
  is defensive: a missing or odd answer, a 403 or a 404 all read as "no pull requests".
- The marks are third-party trademarks, used only to point at the brand's own pages.
  `docs/dependencies.md` lists them, with Simple Icons' disclaimer.
- GitLab and Bitbucket pull requests don't show. Showing them later needs their marks
  and the other instance types read.
- Protocol 1.22 adds `Task.pr_url`, `Subtask.pr_url`, `UpdateTaskRequest.pr_url`,
  `JiraIssue.pull_requests` and `PullRequest`.
