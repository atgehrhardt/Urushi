# 0054. Save the workspace from one saver that coalesces changes, and reply only once a change is saved

- **Status:** Accepted
- **Date:** 2026-11-12

## Context
Every change to the workspace (ADR 0008) serialized the whole document, wrote it,
fsynced it (`F_FULLFSYNC` on macOS), and renamed it before clients were told about the
change, and before the request that made it was answered. Concurrent changes queued one
save each. The document is no longer "only kilobytes": synced Jira boards put it past
100 KB. Some changes come in bursts: a divider drag sends a move per pointer event, and
agents change attention and status often. The GUI felt slow (UR-68). The user asked
that operations stay safe: a reply must still mean the change is on disk.

## Decision
- One `Saver` (`service/saver.rs`) owns writing the document, on its own OS thread.
  A change asks it for its revision (`request`) and waits for it (`wait`).
- The saver takes a snapshot of the **newest** state when a save starts. Every change
  made while the previous save ran is covered by one write.
- `Service::commit` wakes `WorkspaceChanged` subscribers **before** the save. The
  request still answers only after its revision (or a newer one) is on disk.
- `mutate` stays synchronous, so its ~50 callers didn't change. It waits through
  `off_the_runtime`. A save error still reaches the caller as `ServiceError::Save`, so
  all-or-nothing operations (thread creation) still roll back.
- A failed save fails its waiters, and the next change retries it.
- At shutdown, `close_store` writes what is pending, then refuses further saves.

## Alternatives considered
- **An async `mutate_durably` at every call site:** about 50 call sites in 17 files
  for the same guarantee a blocking wait gives.
- **Reply first, save later (deferred durability):** this breaks the promise that a
  reply means the change is saved. It also breaks thread creation's rollback, which
  needs the save error.
- **A periodic save (every N ms) instead of on demand:** this adds latency to every
  change in the quiet case, and a crash window that depends on the timer.
- **An embedded database:** more than the problem needs, and a C dependency (ADR 0008).

## Consequences
- A burst of changes costs one or two writes instead of one per change. Clients see a
  change as soon as it is made.
- A client may now see a change that later fails to save. That was already the case in
  memory; a failed save logs an error and the next change retries it.
- The saver is a thread, not a task, so it works under single-threaded test runtimes.
- Amends ADR 0008: the document is still written atomically, but by one writer, and
  less often than once per change.
