# 0004. One worktree per thread, managed via the `git` CLI

- **Status:** Accepted
- **Date:** 2026-11-06

## Context
Every thread must **always** have its own git worktree. Deleting a thread must remove the worktree and all branches it owns, local and remote by default (configurable), and must always warn the user first.

## Decision
- **Run the system `git`** (behind a `Git` trait) instead of linking a git library.
- **Worktrees live under the app data dir:** `<data>/worktrees/<project>/<thread>`.
- **Branch ownership is explicit:** the primary branch, plus branches created after the thread began and checked out in its worktree (read from the worktree's own HEAD reflog). It never includes the base or default branch, branches that existed before the thread, or branches checked out elsewhere.
- **Deletion is two steps:** `PlanThreadDeletion` returns a plan and a token, the client must show a warning, then `DeleteThread(token)` runs. The plan is re-validated at execution time, so nothing can change between warning and deletion.

## Alternatives considered
- **libgit2 (`git2`):** GPLv2 with a linking exception, which doesn't meet our permissive-only policy.
- **gitoxide (`gix`):** MIT/Apache, but its worktree and push support isn't complete enough yet. We can revisit it behind the `Git` trait.
- **Worktrees inside the repo:** confuse tools that walk the repo.
- **Worktrees next to the repo:** clutter the user's folders.

## Consequences
- The user's git config, hooks and credentials just work, but git must be installed. We report clearly when it isn't.
- A branch created with `git branch x` and never checked out in the worktree isn't detected as owned. This is a documented limitation.
