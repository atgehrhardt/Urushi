# Feature: Thread worktrees and safe deletion

- **Status:** In progress (ownership rules done, git + daemon pending)
- **Owners (crates):** `urushi-core` (`thread`, `ownership`, `naming`), `urushi-daemon` (`git`, deletion)
- **Related ADRs:** [0004](../adr/0004-git-cli-worktrees.md)

## What it does
Every thread **always** gets its own git worktree on a new branch `<prefix><slug>-<short id>`. Deleting a thread removes the worktree and every branch the thread owns, locally and (by default, configurable) on remotes. It always shows a warning listing exactly what will be destroyed.

## How it works
- **Creation** is all-or-nothing:
  1. Snapshot the existing branches.
  2. `git worktree add -b`.
  3. Persist the thread.
  4. Open the first tab.

  If any step fails, everything is rolled back.
- **Ownership** is decided in `urushi_core::ownership::owned_branches` from facts the daemon gathers. Those facts include the worktree's own HEAD reflog and the branches that existed before the thread.
- **Deletion** has two steps:
  1. `PlanThreadDeletion` returns a `DeletionPlan` and a token.
  2. The client shows the warning, then sends `DeleteThread{token}`.

  The daemon re-computes the plan and refuses with `CONFLICT` if anything changed.

## Invariants
- A thread never owns: the base branch, the default branch, branches that existed before it, branches checked out in other worktrees, or branches that no longer exist.
- Nothing is deleted without a matching token. The token is only issued with a plan.

## Tests
- `crates/urushi-core/src/ownership.rs` (unit tests).
- Git integration tests in temporary repos with a local bare "remote" (planned in `urushi-daemon`).

## Limitations and follow-ups
- A branch created with `git branch x` and never checked out in the worktree is not detected as owned.
