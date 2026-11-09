# 0018. Start a task in an existing thread through `CreateTab`, linking it there

- **Status:** Accepted
- **Date:** 2026-11-09

## Context
**Start thread** on a board task always made a new thread: a new branch and worktree, with
the task agent running the task (ADR 0014), and the task linked to that thread (ADR 0012).
Users also want to hand a task to a thread they already have, for example a follow-up to
work already under way in that worktree. ADR 0012 links a task only when its thread is
created, and no request could start the task agent anywhere but a new thread's first tab.

## Decision
- **`CreateTab` gains `task_id`, `subtask_id` and `start_task_agent`** (protocol 1.10),
  with the same meaning as on `CreateThread`. The daemon checks the task and resolves the
  task agent and prompt before any terminal starts. It records the new tab and the task's
  link to the tab's thread in **one state mutation**, then types the agent's command.
  Without `start_task_agent`, the tab is a plain shell and the task is still linked.
- **A task links to its most recent thread.** If the task was linked to another thread,
  the link moves. The app only offers this for a task that has no live link (its button
  says "Go to thread" otherwise), but the CLI can relink.
- In the app, the new-thread dialog opened from a task has a **New thread | Existing
  thread** switch. "Existing" lists the ready threads and sends `CreateTab`.

## Alternatives considered
- **A new `StartTaskInThread` request.** It would carry exactly the fields above and
  answer with a `Tab`. That makes it the same operation as `CreateTab` under a second name,
  and the daemon would need a second path that opens tabs.
- **Link with a separate request, then `CreateTab`.** This has the problem ADR 0012
  rejected: two requests can half-succeed, and the client would do the workspace
  bookkeeping.
- **Type the task into an agent already running in the thread.** We can't tell whether
  that agent is idle or what its input expects, and a stray prompt would be typed into
  someone's work. A new tab is predictable and keeps the existing tabs as they were.
- **Many threads per task.** The UI shows a single thread per task. Moving the link keeps
  `Task.thread_id` a single value.

## Consequences
- ADR 0012's "linked as part of creating the thread" now also covers "as part of creating
  a tab for the task". Deleting a thread still clears its links.
- A task started in an existing thread no longer points at the thread it was linked to
  before. That earlier thread is not changed.
- Older daemons (< 1.10) ignore the new fields and would open an unlinked plain tab, so
  the app shows the switch only on 1.10 daemons.
