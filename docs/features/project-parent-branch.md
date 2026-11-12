# Feature: A project's parent branch and its parent-branch thread

- **Status:** Shipped (daemon, CLI and GUI; protocol 1.18)
- **Owners (crates):** `urushi-core` (`project`: `ProjectSettings`, `ProjectBase`, `base_checkout`; `thread`: `ThreadKind`), `urushi-proto` (`convert/folders.rs`), `urushi-daemon` (`service/project_base.rs`, `service/threads.rs`, `service/deletion.rs`, `service/repos.rs`, `server/organize.rs`, `git/`), `urushi-client` (`projects.rs`, `WorkspaceMirror::project_base_thread`), `urushi-app` (`project_settings.rs`, `update/project_settings.rs`, `view/project_settings.rs`, `view/projects.rs`, `view/thread_card.rs`, `view/menu/`, `view/new_thread.rs`, `view/dialogs.rs`), `urushi-cli` (`project_settings.rs`)
- **Related ADRs:** [0029](../adr/0029-project-parent-branch-threads.md), [0010](../adr/0010-repos-and-projects.md), [0004](../adr/0004-git-cli-worktrees.md)

## What it does
- A project can name a **parent branch**: a branch in one repo, chosen from its local and remote branches. New threads of that repo in the project start from it ("Start from" shows "`<branch>` (project)").
- While its settings ask for it (**on by default**), the project has a **parent-branch thread**, always first in the project, with a terminal on the parent branch itself:
  - If the branch is already checked out somewhere (usually the repo's own folder), it runs there.
  - Otherwise it runs in a worktree of its own.
  - Its branch is never deleted.
- A project without a parent branch takes its first thread's base, when that base is a local branch.
- **Right-click a project → Project settings…** to change the repo and branch, or to switch the thread off. Switching it off, or picking another parent branch while the thread is open, first shows the deletion warning to close it.
- **Right-click the parent-branch thread** for Rename… or Close parent-branch thread…. It has no × button, and it can't be dragged, pinned or moved.

## How it works
- **Core:** `Project.settings: ProjectSettings{base: Option<ProjectBase{repo, branch}>, base_thread}`. `base_thread` defaults to true, and projects saved earlier load with the defaults. `Thread.kind` is `Branch` (the default) or `ProjectBase{attached}`. `base_checkout(branch, worktrees)` decides where the thread runs: `Attach(path)` or `NewWorktree`.
- **`UpdateFolderSettings`** (`server/organize.rs` → `Service::update_project_settings`):
  1. Checks the project and repo.
  2. Turns the picked branch into a local one (`local_parent_branch`):
     - a local branch is used as is;
     - for `origin/x`, `x` is used, created with `git branch --track` if needed;
     - anything else is `INVALID_ARGUMENT`.
  3. Refuses with `FAILED_PRECONDITION` if an open parent-branch thread would have to close.
  4. Saves, creating the thread when wanted (`create_project_base_thread`). The thread and the settings are recorded in one mutation. A worktree it made is removed again if that fails, but never a branch.
- **Creating a thread** (`service/threads.rs::start_ref`): the requested base, else the project's parent branch in this repo, else `Repo.default_base`, else the current branch. Afterwards `adopt_project_base` gives a project without a parent branch its first thread's base. This runs in the background after the thread's reply, so the new thread doesn't wait for a second worktree (ADR 0055). The first thread is the project's oldest one, so of threads created at once exactly one adopts (`tests/project_base.rs::concurrent_first_threads_adopt_the_parent_branch_once`).
- **Deleting** (`service/deletion.rs::compute_plan`): a parent-branch thread owns no branches. When attached, `keeps_worktree` is set, no changes are counted, and `execute_plan` skips `worktree remove`. A successful delete sets the project's `base_thread = false`. Every project's parent branch is in the `protected` set of every thread's plan.
- **Organizing:** `update_thread` refuses to move a parent-branch thread to another project. `delete_project` ungroups it like any thread. `remove_repo` clears parent branches in that repo.
- **Branches:** `ListBranches` also returns `remote_branches`. The new-thread "Start from" list and the parent-branch picker show them after the local ones.
- **App:**
  - `Dialog::ProjectSettings(ProjectSettingsDraft)`. On Save, if `ProjectSettingsDraft::needs_closing` says the open thread has to close, the app requests its plan with `Followup::DeletionPlan(Some(PendingSettings))`.
  - The warning keeps those pending settings (`Dialog::DeleteThread.then`). Confirming sends `DeleteThread` with `Followup::ProjectBaseClosed`, then `UpdateFolderSettings`.
  - The warning says "Closes the project's parent-branch thread" and, when attached, that the checkout is kept.
  - Older daemons (minor < 18) get no menu item.
- **CLI:**
  - `urushi project settings <project> [--repo R --branch B | --clear] [--base-thread on|off]` prints or changes them. A refusal points to `urushi thread rm <id>`.
  - `urushi ls` shows `(parent: <branch> in <repo>)` and marks the thread.
  - `urushi repo branches` lists remote branches.
  - `print_plan` says when nothing on disk is removed.

## Invariants
- A parent-branch thread never owns a branch (`compute_plan`), and an attached one never removes its checkout (`DeletionPlan.keeps_worktree`, `execute_plan`). Both are checked by `tests/project_base.rs`.
- Closing it goes only through `PlanThreadDeletion` → confirmation → `DeleteThread(token)` (AGENTS.md invariant 7). `update_project_settings` refuses anything that would otherwise need it closed.
- A project has at most one parent-branch thread (`State::project_base_thread`, re-checked in `record_project_base_thread`).
- Project settings are all or nothing: if the thread can't be made, nothing is saved.

## Tests
- `crates/urushi-core/src/project.rs`: defaults, round trip, `base_in`, and `base_checkout`.
- `crates/urushi-core/src/thread.rs`: the kind and its JSON.
- `crates/urushi-proto/src/convert/tests.rs` and `tests/round_trip.rs`: folder settings, thread kind, new fields and the request.
- `crates/urushi-daemon/src/git/cli/tests.rs::existing_branches_get_worktrees_and_remote_only_ones_are_tracked`.
- `crates/urushi-daemon/tests/project_base.rs`:
  - attached to the repo folder, with a dirty file and `main` kept;
  - new threads start from the parent branch;
  - moving the thread and changing settings while it is open are refused;
  - an own worktree is removed but its branch kept;
  - surviving a restart;
  - remote parent branches, and invalid names refused;
  - adoption by the first thread, with the switch on and off;
  - deleting the project.
- `crates/urushi-client/src/workspace.rs`: `project_base_thread`.
- `crates/urushi-app/src/project_settings.rs` (draft and `needs_closing`), `update/tests/project_settings.rs` (dialog, Save, close-then-save, new-thread defaults), `view/project_settings.rs` (sidebar order, card, menus, version gate), `update/tests/organize.rs` (remote branches in Start from).
- Snapshot scenarios `project-base-thread` and `project-settings`.

## Limitations and follow-ups
- There is one parent branch per project. A project spanning several repos only uses it for threads of that repo.
- Existing projects get no parent branch until one is set, or until a new first thread is created in an empty project.
- Picking a remote parent branch creates the local branch even when saving fails afterwards (e.g. git refuses the worktree).
- An attached thread runs in the user's own checkout. If they switch that checkout to another branch, the thread follows it, and nothing re-checks it.
- "Start from" doesn't accept a typed ref (tag or commit) in the GUI; `urushi thread new --base` does.
- No `git fetch` happens before listing remote branches.
