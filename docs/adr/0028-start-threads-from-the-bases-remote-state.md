# 0028. Start a new thread from its base's state on the remote, without touching the user's checkout

- **Status:** Accepted
- **Date:** 2026-11-10
- **Amended by:** [0054](0054-prefetch-a-new-threads-base-and-share-fetches.md) (fetches shared and reused for 60 s, prefetched while the dialog is open)

## Context
A thread branches from a base, usually `main`. Urushi resolved that base to the
**local** ref, so unless the user had pulled, every new thread started from stale code.
The agent in it then worked against old files, and its branch needed a rebase before
review. The user asked (UR-13) for a setting, on by default, that updates the base from
the remote before the worktree is made.

The base is usually checked out in the user's main clone, where they may be working.
Changing that clone's files, or moving its branch without its files, would surprise them
or corrupt their view of the repo.

## Decision
- A global setting, `update_base_from_remote`, **defaults to on**. When it is on, the
  daemon does the following before `git worktree add`:
  - It fetches **only the base's branch** (`git fetch --no-tags <remote>
    +refs/heads/<b>:refs/remotes/<remote>/<b>`) from the remote it tracks. That means the
    upstream of a local branch, or the remote that a name like `origin/main` points to.
    Tags, commits, `HEAD` and branches without an upstream aren't fetched.
  - It starts the thread from the remote's commit when the local branch is behind it.
  - If the local branch is equal, ahead, or **diverged**, it keeps the local commit, so
    the user's own commits always win.
  - It **never changes a checkout.** It fast-forwards the local branch ref (a
    compare-and-swap `update-ref`) only when no worktree has that branch checked out.
- Updating is **best effort**. If the remote is offline, refuses the sign-in, or the
  fetch passes its 30 s timeout, the daemon logs it and the thread starts from the local
  commit. Thread creation never fails because of the update.
- The thread records the base's name as given (`base_ref`) and the commit it actually
  started from (`base_commit`).

## Alternatives considered
- **`git pull` / `merge --ff-only` in the main clone:** changes files under the user's
  feet and fails on a dirty tree. The user chose to leave the clone alone.
- **`git fetch --all`, or fetching the whole remote:** slower on big repos, and the
  thread waits for it. Only the base matters here.
- **Always start from the remote's commit:** this would silently drop local, unpushed
  commits on the base. A diverged base is the user's call, not ours.
- **A setting naming one branch to update (e.g. `main`):** this misses threads based on
  other branches, and is one more thing to configure. The thread's own base is what
  matters.
- **Fail thread creation when the fetch fails:** this makes Urushi unusable offline for
  the sake of freshness.

## Consequences
- Creating a thread now waits for one network fetch when the base tracks a remote. This
  usually takes well under a second, and at most 30 s before falling back.
- The main clone's `main` can stay behind while threads start ahead of it. That is
  expected, and the user's own `git pull` catches it up.
- Credentials prompts can't be answered (`GIT_TERMINAL_PROMPT=0`). Remotes that need
  them fall back to the local commit, with a log line.
- The setting is global. Per-repo overrides can be added later if needed.
