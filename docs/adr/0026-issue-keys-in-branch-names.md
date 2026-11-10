# 0026. Name a Jira issue's thread branch after its key, and let Jira's integration link the code

- **Status:** Accepted
- **Date:** 2026-11-10

## Context
Teams want the branches and pull requests for an issue to show on it in Jira. Jira does
this itself through its code-host integrations (GitHub for Jira, GitLab for Jira,
Bitbucket): they report branches, commits and pull requests, and Jira attaches each to
every issue whose key (`UR-3`) appears in the branch name, commit message or pull
request title. Jira's own **Create branch** suggests `UR-3-summary`. Urushi named a
thread's branch after the task's title only (`urushi/invoice-pdfs`), so nothing linked
unless someone typed the key.

## Decision
- A thread started from a task that is a Jira issue gets a branch led by **that issue's
  key**, written as Jira writes it: `<prefix><KEY>-<slug>`. The app suggests it in the
  new-thread form; the daemon uses `<prefix><KEY>-<slug>-<short id>` when no branch is
  given. A subtask's thread uses the subtask's own key.
- `IssueKey` (`urushi_core::naming`) accepts only Jira's key shape, so a key always makes
  a valid branch name; a key the title already starts with isn't repeated.
- The agent's first prompt asks it to put the key in commit messages and pull request
  titles.
- Urushi doesn't call GitHub or write links into Jira: the integration the site already
  has does the linking.

## Alternatives considered
- **Link pull requests from Urushi** (Jira remote links, or a GitHub integration of our
  own): new credentials, a second integration that duplicates Jira's, and nothing for
  commits or builds.
- **Lowercase the key with the slug** (`ur-3-…`): Jira's integrations look for the key
  as Jira writes it; uppercase is what Jira itself suggests.
- **Key after the title**: works for matching, but sorts and reads worse, and differs
  from Jira's own convention.

## Consequences
- Linking depends on the site having a code-host integration installed.
- Branch names carry uppercase letters, where Urushi's slugs were all lowercase.
- A task started in an existing thread keeps that thread's branch; only commits and
  pull request titles link it.
