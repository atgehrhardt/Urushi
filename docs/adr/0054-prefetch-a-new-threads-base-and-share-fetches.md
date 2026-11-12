# 0054. Fetch a new thread's base while its dialog is open, share fetches for 60 s, and adopt the parent branch after the reply

- **Status:** Accepted
- **Date:** 2026-11-12

## Context
Creating a thread fetches its base's branch from the remote before the worktree is made
(ADR 0028). The New Thread dialog waited for that fetch, often a second and up to 30 s.
A project's first thread also made the parent-branch thread's worktree (ADR 0029)
before replying. Creating a thread was the slowest thing in the GUI (UR-68). The user
types a title for a few seconds before submitting, which is time a fetch could use.

## Decision
- **Prefetch.** `PrepareThreadBase{project_id, base_ref, folder_id}` (protocol 1.31)
  answers `Ok` at once. With `update_base_from_remote` on, the daemon starts fetching
  the base a `CreateThread` with the same fields would start from. The GUI sends it
  when the dialog opens and when its repo, base or project changes.
- **Shared fetches** (`service/base_fetch.rs`), keyed by repo, remote and branch:
  - A **successful** fetch that started at most 60 s ago is reused.
  - A fetch still running is joined, and its result is shared, whether it succeeds or
    fails.
  - A **failed** fetch is never reused afterwards, so the create makes its own attempt
    before falling back to the local commit (ADR 0028).
  - At most 4 background fetches run at once, and at most 64 finished ones are kept.
- **Unchanged at create time:** the behind / equal / ahead / diverged decision and the
  fast-forward still run on the refs as they are then.
- **Adoption after the reply.** A project's first thread adopts its base as the
  parent branch in a background task. "First" is now the project's oldest
  non-parent-branch thread (thread ids are time-ordered). Of several threads created at
  once, exactly one adopts; before, neither did if both were recorded before either
  checked.

## Alternatives considered
- **Fetch in the background after creating the thread, then move its branch:** this
  changes a branch under an agent that may already be working on it.
- **Create the thread first, in the `Creating` state, and fetch afterwards:** a thread
  without its worktree breaks invariant 8. The GUI's placeholder (ADR 0055) gives the
  same feel without that.
- **A shorter window (e.g. 10 s):** this would often miss the fetch started when the
  dialog opened. **A longer one:** threads would start further behind the remote.
- **Reuse failed fetches too:** a brief network drop while the dialog was open would
  make the create skip the remote.
- **Prefetch every repo's default base periodically:** network traffic for threads
  nobody creates.

## Consequences
- A thread created within 60 s of another on the same base starts from the earlier
  fetch: a push made in between is not seen. Its `base_commit` records which commit it
  started from.
- The new thread's reply no longer waits for the parent-branch thread. That thread
  arrives through `WorkspaceChanged`.
- Older daemons answer `PrepareThreadBase` with an error, so clients send it only to
  1.31 and later, and ignore what comes back.
- Amends ADR 0028, which said every create fetches, and ADR 0029, under which adoption
  happened before the reply.
