# Feature: Organizing threads (projects, pins, statuses)

- **Status:** Shipped
- **Owners (crates):** `urushi-core` (`project`, `status`, `thread`), `urushi-daemon` (`service/projects.rs`, `server/organize.rs`, `store.rs`), `urushi-client` (`thread_links.rs`), `urushi-app` (`organize.rs`, `update/organize.rs`, `view/projects.rs`, `view/thread_card.rs`, `brand_icons.rs`, `view/menu/`, `view/new_thread.rs`), `urushi-ui` (positioned nodes, secondary click, drag and drop, selectors), `urushi-cli`
- **Related ADRs:** [0010](../adr/0010-repos-and-projects.md), [0002](../adr/0002-daemon-source-of-truth.md), [0036](../adr/0036-pull-requests-from-jira-dev-info-and-local-links.md)

## What it does
- **Repos** are git repositories (what used to be called projects). **Projects** are groups of threads you make yourself, and can hold threads from any repo. A thread is in at most one project; the rest are listed under **No project**, below the projects. Projects are listed in the order you give them; a new project goes at the bottom.
- The sidebar has two tabs:
  - **Projects:** each project is collapsible. Inside it come its [parent-branch thread](project-parent-branch.md), then pinned threads, then one collapsible section per **status** in use, then threads without a status. Thread cards show the repo and branch.
  - **Links on cards** (UR-24, ADR 0036): a card whose thread was started from a Jira
    issue shows Jira's mark beside its title, and one whose task has a pull request
    shows GitHub's mark. Clicking a mark opens the issue or pull request in the browser
    and doesn't open the thread. The pull request is a local task's own link
    ([task boards](task-boards.md)), or a Jira issue's open one (else its newest) from
    Jira's development info ([Jira boards](jira-boards.md)).
  - **Repos:** each repo with its path, thread count, and **+** for a new thread.
- **Right-click** (two-finger click on a trackpad) a thread for: Rename…, Pin/Unpin, Status ›, Move to project ›, Delete thread…. Right-click a project for: New thread…, Rename…, Project settings… (its [parent branch](project-parent-branch.md), protocol 1.18), Move up, Move down, Delete project. Move up/down only appear where the project can go that way. Deleting a project keeps its threads, which become ungrouped.
- **Statuses are user-defined**: the Status page lists every status in use, plus "New status…" and "Clear status". A status (and its sidebar section) exists while some thread has it.
- **Drag** a thread onto a project, or onto "No project", to move it.
- **Drag a project's header** onto another project to reorder: it takes that project's place, so it lands below a project further down and above one further up. While dragging, the dragged project is highlighted and an accent line shows where it would land (only where it would move). Escape or releasing elsewhere cancels.
- **New thread** (from a project's **+**, a repo's **+**, Cmd+N, or a task's Start thread): the repo, the **Project** and the **Start from** branch are chosen from lists (local branches, then remote ones). The project defaults to the one it was started from (none from a repo or Cmd+N), and the repo to that project's parent-branch repo; "Start from" defaults to the project's parent branch in that repo, else the repo's current branch; **None** leaves the thread ungrouped. The project selector is hidden while there are no projects.

## How it works
- Wire (ADR 0010: projects are `Folder`s): `CreateFolder`, `RenameFolder`, `DeleteFolder`, `UpdateThread{title?, pinned?, status?, folder_id?}` (empty status / folder clears), `CreateThread.folder_id`, `Workspace.folders`, `Thread.{folder_id, pinned, status}`. Protocol 1.2. `MoveFolder{folder_id, index}` since 1.11: the index counts positions without the moved project and is clamped to the end, like `MoveColumn`. `Workspace.folders` is in display order.
- The daemon validates everything in `service/projects.rs`: names are trimmed and at most 80 characters, statuses go through `urushi_core::Status` (trimmed, one line, at most 40 characters), and an unknown project is `NOT_FOUND`. The order lives in the stored list itself; `urushi_core::project::move_project` reorders it. `State::repair_interrupted` ungroups threads whose project is gone.
- The app turns UI gestures into those requests (`update/organize.rs`) and redraws from `WorkspaceChanged`. View-only state (which tab is open, collapsed sections, the open menu, an ongoing drag of a thread or project as `organize::Dragged`, and the project a dragged project would replace) lives in `AppState`. A project drop goes through `organize::project_drop_index`, which gives the target's index, or nothing when the project would stay put. `Msg::DragOver` while a project is dragged updates `project_over` for the drop line. Older daemons (minor < 11) get neither the drag nor the menu items (`AppState::daemon_supports_project_order`).
- Toolkit (`urushi-ui`): `Node::at` places a stack child at a point (menus, the drag label); `on_secondary_click`, `on_drag` and `on_drop` feed `Frame::secondary_click`, `drag_source` and `drop_target`. The runner turns a left press that moves 5 px into a drag (`runner.rs::drag_to`). `widgets::select_button` / `select_options` make the selectors.
- Links on cards: `WorkspaceMirror::thread_links` (`urushi-client`) reads every task's
  `thread_id`, `issue` and `pr_url` once per sidebar group into `ThreadLinks{issue,
  pull_request}` per thread. The first linked task (parents first) with each link gives
  it, since several tasks can be linked to one thread (ADR 0018).
  `view/thread_card.rs::link_marks` draws `brand_icons::mark`. These are Simple Icons'
  glyphs, pre-rendered as white 48 px masks in `assets/brands/` and tinted to
  `theme.text_muted`, each tint cached under its own image id. A click sends
  `Msg::OpenUrl` → `Effect::OpenUrl`, and the platform opens only `https://` URLs.
- CLI: `urushi project new|rename|rm|move <project> <index>`, `urushi thread pin|unpin|status|move`, `urushi thread new <repo> --project <id>`; `urushi ls` shows pins, statuses and projects.

## Invariants
- Organizing never touches worktrees, branches or terminals (`service/projects.rs` only edits state; e2e test checks the worktree survives deleting the project).
- An `UpdateThread` is all or nothing: if any field is invalid, nothing changes (`threads_and_agents.rs`).
- Old workspaces load unchanged (`store.rs` and `thread.rs` tests).

## Tests
- `crates/urushi-core/src/{status,thread}.rs`: validation; loading threads saved before the rename. `project.rs`: moving projects (up, down, clamped, unknown).
- `crates/urushi-daemon/src/store.rs`: loading old documents. `tests/threads_and_agents.rs::threads_are_organized_into_projects_with_pins_and_statuses`: the whole flow over the wire, including a daemon restart. `tests/project_order.rs`: reordering over the wire, `NOT_FOUND`, and the order surviving a restart.
- `crates/urushi-client/src/workspace.rs`: grouping and the status list. `thread_links.rs`: a thread's issue and pull request (open first, invalid links ignored, the first linked task wins).
- `crates/urushi-app/src/view/thread_card.rs`: cards show the marks only when linked; a click on one opens its page and not the thread. `brand_icons.rs`: tinted masks with their own ids.
- `crates/urushi-ui/tests/layout.rs`: positioned children, secondary clicks, drag sources and drop targets.
- `crates/urushi-app/src/update/tests/organize.rs` and `view/mod.rs`: menus, prompts, drag and drop, selectors, sidebar order and collapsing. `update/tests/project_order.rs`: dragging projects, the drop line, Move up/down, drop targets and older daemons.
- Snapshot scenarios `menu`, `menu-status`, `drag`, `drag-project`, `repos`, `new-thread-base`, `new-thread-project`, `thread-links` (cards with Jira and GitHub marks; see the GUI doc).

## Limitations and follow-ups
- Statuses can't be reordered; they sort alphabetically.
- A dragged project can't be dropped onto "No project" to send it to the bottom; drop it on the last project instead.
- A dragged project doesn't scroll a sidebar that is taller than the window.
- Dragging onto a status section doesn't set the status (it moves the thread to that project only).
- Context menus aren't reachable from the keyboard or accessibility actions yet.
- Removing a repo is only available from the CLI (`urushi repo rm`).
- The marks have no tooltip (the toolkit has none); their accessible labels name the issue or pull request.
- Only GitHub pull requests show; GitLab and Bitbucket ones from Jira are left out (ADR 0036).
