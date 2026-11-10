# Feature: Thread worktrees and safe deletion

- **Status:** Shipped (daemon, CLI and GUI).
- **Owners (crates):** `urushi-core` (`thread`, `ownership`, `naming`), `urushi-daemon` (`git/`, `service/threads.rs`, `service/deletion.rs`)
- **Related ADRs:** [0004](../adr/0004-git-cli-worktrees.md), [0028](../adr/0028-start-threads-from-the-bases-remote-state.md)

## What it does
Every thread **always** gets its own git worktree on a new branch: the name the user gave (`CreateThread.branch`), or `<prefix><slug>-<short id>` (for a thread started from a Jira issue, `<prefix><KEY>-<slug>-<short id>`, e.g. `urushi/UR-3-invoice-pdfs-1a2b3c4d`; ADR 0026). The branch must not exist yet. By default the thread starts from its base **as it is on the remote**, so it doesn't start from stale code. Deleting a thread removes the worktree and every branch the thread owns, locally and (by default, configurable) on remotes. It always shows a warning listing exactly what will be destroyed.

## How it works
- **Creation** is all-or-nothing:
  1. Resolve the base. Then, if `update_base_from_remote` is on (the default; `urushi settings set update-base-from-remote off`, or the switch in Settings), update it from the remote (`service/fresh_base.rs`, ADR 0028):
     - The daemon fetches only the base's branch: the upstream of a local branch, or the branch a name like `origin/main` stands for. Tags, commits, `HEAD` and branches without an upstream are left as they are.
     - The thread starts from the remote's commit when the local branch is behind it. When the local branch is equal, ahead or diverged, it starts from the local commit.
     - The local branch ref is fast-forwarded only when no worktree has it checked out. The user's checkout is never changed.
     - A failed or timed-out fetch (30 s) is logged, and the thread starts from the local commit.
  2. Snapshot the existing branches.
  3. `git worktree add -b`.
  4. Persist the thread.
  5. Open the first tab, unless the client asked for none (`skip_initial_tab`; the GUI does, and shows the [agent launcher](agent-launcher.md)).

  If any step fails, everything is rolled back. A base ref that doesn't resolve is reported as `INVALID_ARGUMENT` ("no branch, tag or commit has that name"), an invalid branch name as `INVALID_ARGUMENT`, an existing one as `CONFLICT`.
- **Ownership** is decided in `urushi_core::ownership::owned_branches` from facts the daemon gathers. Those facts include the worktree's own HEAD reflog and the branches that existed before the thread.
- **Deletion** has two steps:
  1. `PlanThreadDeletion` returns a `DeletionPlan` and a token.
  2. The client shows the warning, then sends `DeleteThread{token}`.

  The daemon re-computes the plan and refuses with `CONFLICT` if anything changed.

  `DeleteThread` returns once git is done, which can take seconds with remote branches. Clients need not wait for it. The thread is `Deleting` from the moment the daemon starts, and the request runs to the end even if the client disconnects. The GUI hides the thread as soon as the user confirms, and goes on working ([GUI workspace](gui-workspace.md)).

## Invariants
- A thread never owns: the base branch, the default branch, branches that existed before it, branches checked out in other worktrees, or branches that no longer exist.
- Nothing is deleted without a matching token. The token is only issued with a plan.

## Tests
- `crates/urushi-core/src/ownership.rs` (unit tests).
- `crates/urushi-daemon/src/git/cli/tests.rs`: every git operation against temp repos with a bare `origin`.
- `crates/urushi-daemon/tests/e2e.rs`:
  - full lifecycle: worktree, env, push, extra branch, dirty file, plan contents, stale token refused, everything deleted except `main` locally and on the remote
  - remote deletion disabled
  - failed creation leaves no branches behind
- `crates/urushi-daemon/tests/threads_and_agents.rs`: named branch, no first tab, duplicate and invalid names, unknown base.
- `crates/urushi-app/src/update/tests/deletion.rs` and `crates/urushi-app/src/deletions.rs`: the GUI hides a thread on confirm, and keeps selection and focus. A refusal brings the thread back.
- `crates/urushi-daemon/tests/fresh_base.rs` covers updating the base from its remote:
  - a checked-out base is fetched and its checkout left alone
  - a base checked out nowhere is fast-forwarded
  - an `origin/main` base
  - local-ahead and diverged bases keep the local commit
  - tags and branches without an upstream
  - an unreachable remote
  - the setting off

## Limitations and follow-ups
- A branch created with `git branch x` and never checked out in the worktree is not detected as owned.
- A base that exists only on the remote and was never fetched (e.g. `origin/new-feature`) still fails to resolve. The update happens after the base resolves locally.
- A remote that needs a password or passphrase prompt can't be fetched (`GIT_TERMINAL_PROMPT=0`), so threads on it start from the local commit.
- `update_base_from_remote` is global, with no per-repo override yet.
- `main`, `master`, `develop` and `trunk` are always protected, as are the base branch and remote default branches.
- If the daemon crashes mid-create or mid-delete, the thread is marked `DeletionFailed` on restart, so the user finishes the job through the normal warned flow.
