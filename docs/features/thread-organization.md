# Feature: Organizing threads (projects, pins, statuses)

- **Status:** Shipped
- **Owners (crates):** `urushi-core` (`project`, `status`, `thread`), `urushi-daemon` (`service/projects.rs`, `server/organize.rs`, `store.rs`), `urushi-app` (`update/organize.rs`, `view/projects.rs`, `view/menu/`, `view/new_thread.rs`), `urushi-ui` (positioned nodes, secondary click, drag and drop, selectors), `urushi-cli`
- **Related ADRs:** [0010](../adr/0010-repos-and-projects.md), [0002](../adr/0002-daemon-source-of-truth.md)

## What it does
- **Repos** are git repositories (what used to be called projects). **Projects** are groups of threads you make yourself, and can hold threads from any repo. A thread is in at most one project; the rest are listed under **No project**, below the projects.
- The sidebar has two tabs:
  - **Projects:** each project is collapsible. Inside it come pinned threads first, then one collapsible section per **status** in use, then threads without a status. Thread cards show the repo and branch.
  - **Repos:** each repo with its path, thread count, and **+** for a new thread.
- **Right-click** (two-finger click on a trackpad) a thread for: Rename…, Pin/Unpin, Status ›, Move to project ›, Delete thread…. Right-click a project for: New thread…, Rename…, Delete project. Deleting a project keeps its threads, which become ungrouped.
- **Statuses are user-defined**: the Status page lists every status in use, plus "New status…" and "Clear status". A status (and its sidebar section) exists while some thread has it.
- **Drag** a thread onto a project, or onto "No project", to move it.
- **New thread** (from a project's **+**, a repo's **+**, Cmd+N, or a task's Start thread): the repo, the **Project** and the **Start from** branch are chosen from lists. The project defaults to the one it was started from (none from a repo or Cmd+N); **None** leaves the thread ungrouped. The project selector is hidden while there are no projects.

## How it works
- Wire (ADR 0010: projects are `Folder`s): `CreateFolder`, `RenameFolder`, `DeleteFolder`, `UpdateThread{title?, pinned?, status?, folder_id?}` (empty status / folder clears), `CreateThread.folder_id`, `Workspace.folders`, `Thread.{folder_id, pinned, status}`. Protocol 1.2.
- The daemon validates everything in `service/projects.rs`: names are trimmed and at most 80 characters, statuses go through `urushi_core::Status` (trimmed, one line, at most 40 characters), and an unknown project is `NOT_FOUND`. `State::repair_interrupted` ungroups threads whose project is gone.
- The app turns UI gestures into those requests (`update/organize.rs`) and redraws from `WorkspaceChanged`. View-only state (which tab is open, collapsed sections, the open menu, an ongoing drag) lives in `AppState`.
- Toolkit (`urushi-ui`): `Node::at` places a stack child at a point (menus, the drag label); `on_secondary_click`, `on_drag` and `on_drop` feed `Frame::secondary_click`, `drag_source` and `drop_target`. The runner turns a left press that moves 5 px into a drag (`runner.rs::drag_to`). `widgets::select_button` / `select_options` make the selectors.
- CLI: `urushi project new|rename|rm`, `urushi thread pin|unpin|status|move`, `urushi thread new <repo> --project <id>`; `urushi ls` shows pins, statuses and projects.

## Invariants
- Organizing never touches worktrees, branches or terminals (`service/projects.rs` only edits state; e2e test checks the worktree survives deleting the project).
- An `UpdateThread` is all or nothing: if any field is invalid, nothing changes (`threads_and_agents.rs`).
- Old workspaces load unchanged (`store.rs` and `thread.rs` tests).

## Tests
- `crates/urushi-core/src/{status,thread}.rs`: validation; loading threads saved before the rename.
- `crates/urushi-daemon/src/store.rs`: loading old documents. `tests/threads_and_agents.rs::threads_are_organized_into_projects_with_pins_and_statuses`: the whole flow over the wire, including a daemon restart.
- `crates/urushi-client/src/workspace.rs`: grouping and the status list.
- `crates/urushi-ui/tests/layout.rs`: positioned children, secondary clicks, drag sources and drop targets.
- `crates/urushi-app/src/update/tests/organize.rs` and `view/mod.rs`: menus, prompts, drag and drop, selectors, sidebar order and collapsing.
- Snapshot scenarios `menu`, `menu-status`, `drag`, `repos`, `new-thread-base`, `new-thread-project` (see the GUI doc).

## Limitations and follow-ups
- Projects and statuses can't be reordered; projects keep creation order, statuses sort alphabetically.
- Dragging onto a status section doesn't set the status (it moves the thread to that project only).
- Context menus aren't reachable from the keyboard or accessibility actions yet.
- Removing a repo is only available from the CLI (`urushi repo rm`).
