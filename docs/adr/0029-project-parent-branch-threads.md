# 0029. Give projects a parent branch, and a parent-branch thread that works on it

- **Status:** Accepted
- **Date:** 2026-11-10

## Context
New threads started from the main checkout's current branch, usually `main`, unless
someone picked another branch every time (UR-15). Work for one project often lives
under its own long-running branch (`feature/billing`), and users also want a place to
work *on* that branch: to merge, test, or make small fixes. Every thread, though, has
been a new branch in a new worktree (ADR 0004). Git won't check one branch out in two
worktrees, and the branch people care most about, `main`, is usually checked out in the
repo's own folder.

## Decision
- A **project** (ADR 0010) can have a **parent branch**: a local branch in one repo
  (`ProjectSettings.base`). New threads of that repo filed under the project start from
  it unless another base is asked for. Picking a remote branch (`origin/x`) stores `x`
  and creates it, tracking the remote one, if needed.
- A project also has a switch, **on by default**, for a **parent-branch thread**
  (`ThreadKind::ProjectBase`). This thread works on the parent branch itself and is
  always listed first in its project:
  - If some worktree already has the branch checked out (usually the repo's own folder),
    the thread runs **there**. It is *attached*, and Urushi never removes that checkout.
  - Otherwise Urushi makes a worktree that checks the branch out
    (`git worktree add <path> <branch>`, without `-b`).
  - It **never owns a branch**. Its plan lists no branches, and an attached thread's
    plan sets `keeps_worktree`, so nothing on disk is removed.
- **Closing the thread is deleting it** through the usual plan → confirm →
  `DeleteThread(token)` flow (invariant 7), and that also switches it off in the
  project's settings. `UpdateFolderSettings` refuses (`FAILED_PRECONDITION`) any change
  that would need an open parent-branch thread closed. The app runs the deletion warning
  first, then saves.
- So that "on by default" means something, a project with no parent branch adopts its
  first thread's base when that base is a local branch.
- Every project's parent branch is protected in every thread's deletion plan.
- Wire (protocol 1.18): `Folder.settings`, `UpdateFolderSettingsRequest`,
  `Thread.{project_base, attached_checkout}`, `Branches.remote_branches`,
  `DeletionPlan.keeps_worktree`.

## Alternatives considered
- **A per-repo default base** (`Repo.default_base`, already in the model): one base for
  every project of a repo, while the issue asks for one per project. It also offers no
  place to work on the branch itself.
- **Always run the parent thread in the repo's own folder**: simple, but the folder may
  have another branch checked out, so the thread wouldn't be on the parent branch.
- **Always give it its own worktree**: fails for `main` (checked out in the repo folder)
  until the user switches that folder away, which is the common case.
- **Close it directly from the settings switch** (no deletion warning): it can remove a
  worktree with uncommitted changes, which invariant 7 reserves for the warned, tokened
  flow.
- **Turn it into an ordinary thread when switched off**: an ordinary thread owns its
  primary branch, so deleting it later would delete the parent branch.

## Consequences
- ADR 0004's "one new branch per thread" no longer holds for this one kind of thread.
  Every thread still has a worktree (invariant 8): an attached thread's is the user's
  own checkout.
- Terminals of an attached thread run in the user's main checkout. Changes made there
  are the user's, and closing the thread leaves them.
- Moving a parent-branch thread to another project is refused. Deleting its project
  ungroups it like any thread, and it can then be closed normally.
- Picking a remote parent branch creates a local branch even if saving then fails.
  This isn't destructive, and the branch is never deleted by Urushi.
