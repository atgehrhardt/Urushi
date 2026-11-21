# 0060. Shelve a thread by stopping its panes and keeping their restore points

- **Status:** Accepted
- **Date:** 2026-11-20

## Context
Users run many threads at once, each with shells, agents and dev servers in its panes.
When the machine runs short of memory or CPU, the only way to free a thread's share is
to close its tabs or delete it. Closing tabs loses the agents' conversations and
the layout. Deleting removes the worktree. UR-75 asked for a third way: "shelve" a
thread. Shelving closes its sessions but keeps what is needed to bring them back, and
keeps the thread listed as closed but available.

ADR 0013 already brings panes back after a daemon restart. Each pane has a restore
point: its size, directory, output, the agent and its conversation, or the foreground
job. Shelving is the same restore, done for one thread whenever the user asks.

## Decision
- **Shelving a thread stops every pane in every tab of the thread.**
  - The daemon first brings the panes' restore points up to date. This happens while
    the processes still run, because a stopped Claude no longer says which session it
    had open.
  - The thread is then marked shelved, and its processes are killed.
  - The worktree, branch, tabs, layout and restore points stay. The thread keeps its
    place in its project and status group.
- **Unshelving starts every pane again from its restore point**, as a restart does (ADR
  0013): the shell in its directory below its old output and an "unshelved" line, then
  the agent resuming its conversation or the job running again.
- **Shelved is a lifecycle state of its own**, `ThreadState::Shelved`, between `Ready`
  and `Deleting`:
  - `Ready → Shelved → Ready`, and a shelved thread can be deleted.
  - It is stored, so a shelved thread stays shelved across daemon restarts. Only ready
    threads are restored at startup.
  - Everything that requires a ready thread refuses a shelved one with
    `FAILED_PRECONDITION`: new tabs, splits, tasks started in it, and reading or typing
    into its panes.
- **The wire keeps `state = READY` and adds `Thread.shelved`** (protocol 1.34), with
  `SetThreadShelved{thread_id, shelved}`. Older clients reject an unknown `ThreadState`
  value, which would break their whole workspace. A bool lets them see a ready thread
  whose panes can't be attached.
- **Killing a shelved thread's sessions doesn't close their panes.**
  - Today a pane whose process ends is taken out of its layout. The daemon now keeps the
    handles of the sessions it stopped for shelving, and leaves those panes in place.
  - A pane exit only acts on the session that actually ended. A shelve followed at once
    by an unshelve then can't close the pane, or kill its new session.
- **The GUI shows a shelved thread dimmed, in its usual place**, with a "shelved" badge.
  - Opening it shows what shelving kept and an **Unshelve** button. Opening never
    unshelves, so a stray click can't restart agents.
  - Its panes aren't attached.
  - Shelving asks first while an agent is working in the thread.

## Alternatives considered
- **Suspend the processes (SIGSTOP) instead of ending them:** it frees CPU but no memory,
  and memory is what runs out. It also ties up file descriptors and PTYs, and doesn't
  survive a daemon restart.
- **A `shelved` flag next to `Ready`:** every check of "can terminals run here" would
  have to look at two fields. A state makes shelved and creating, deleting or failed
  combinations impossible, and the existing `is_ready()` guards refuse shelved threads
  without new code.
- **A new `THREAD_STATE_SHELVED` wire value:** cleaner, but every older client would
  fail to read the workspace once any thread is shelved.
- **Shelving a single tab:** finer grained, but it needs a per-tab state and a second
  list in the UI. The user asked for whole threads.
- **A separate "Shelved" section in the sidebar:** the user chose to keep shelved
  threads in place, dimmed, so their project and status still show.
- **Unshelve on open:** the user chose an explicit button, so browsing threads never
  starts agents.

## Consequences
- A shelved thread uses no processes or terminal memory. Its restore points (up to 2,000
  lines of output per shell pane) stay on disk until it is unshelved or deleted.
- Unshelving has the same limits as a restart: programs start again from the beginning,
  and Codex and Cursor resume their latest conversation in the directory.
- A daemon older than this one can't read a workspace with a shelved thread (an unknown
  state). Unshelve before going back to an older daemon.
- `on_pane_exited` now compares session handles (`SessionHandle::same_session`) rather
  than trusting the pane id. Any future code that kills a session and starts another in
  the same pane relies on that.
