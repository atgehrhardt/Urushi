# Feature: Organizing threads (projects, pins, statuses, unread, notes, colors)

- **Status:** Shipped
- **Owners (crates):** `urushi-core` (`project`, `status`, `thread`, `order`), `urushi-daemon` (`service/projects.rs`, `server/organize.rs`, `store.rs`), `urushi-client` (`thread_links.rs`), `urushi-app` (`organize.rs`, `update/organize.rs`, `update/sidebar_drag.rs`, `update/thread_extras.rs`, `view/projects.rs`, `view/thread_card/`, `view/thread_notes.rs`, `brand_icons.rs`, `view/menu/`, `view/new_thread.rs`), `urushi-ui` (positioned nodes, secondary click, drag and drop, selectors), `urushi-cli`
- **Related ADRs:** [0010](../adr/0010-repos-and-projects.md), [0002](../adr/0002-daemon-source-of-truth.md), [0036](../adr/0036-pull-requests-from-jira-dev-info-and-local-links.md), [0038](../adr/0038-reorder-threads-in-the-stored-list.md), [0048](../adr/0048-thread-unread-notes-and-project-colors-in-the-daemon.md)

## What it does
- **Repos** are git repositories (what used to be called projects). **Projects** are groups of threads you make yourself, and can hold threads from any repo. A thread is in at most one project; the rest are listed under **No project**, below the projects. Projects are listed in the order you give them; a new project goes at the bottom.
- The sidebar has two tabs:
  - **Projects:** each project is collapsible. Inside it come its [parent-branch thread](project-parent-branch.md), then pinned threads, then one collapsible section per **status** in use, then threads without a status. Each of those groups lists its threads in the order you give them; a new thread goes at the bottom. Thread cards show the repo and branch.
  - **Links on cards** (UR-24, ADR 0036): a card whose thread was started from a Jira
    issue shows Jira's mark beside its title, and one whose task has a pull request
    shows GitHub's mark. Clicking a mark opens the issue or pull request in the browser
    and doesn't open the thread. The pull request is a local task's own link
    ([task boards](task-boards.md)), or a Jira issue's open one (else its newest) from
    Jira's development info ([Jira boards](jira-boards.md)).
  - **Repos:** each repo with its path, thread count, and **+** for a new thread.
- **Right-click** (two-finger click on a trackpad) a thread for: Rename…, Pin/Unpin, Mark as unread/read, Add/Edit notes…, Status ›, Move to project ›, Move up, Move down, Delete thread…. Move up/down swap it with its neighbour in its group, and only appear where it has one. Right-click a project for: New thread…, Rename…, Project settings… (its [parent branch](project-parent-branch.md), protocol 1.18), Color ›, Move up, Move down, Delete project. Move up/down only appear where the project can go that way. Deleting a project keeps its threads, which become ungrouped.
- **Unread** (UR-63, ADR 0048): *Mark as unread* makes a thread's title bold with an accent dot before it, on every client, until the thread is opened (selected) again; *Mark as read* clears it by hand. Marking the open thread unread keeps it unread until it is opened again. This is separate from [agent attention](agent-attention.md), which marks threads whose agent finished or waits.
- **Notes** (UR-64): *Add notes…* / *Edit notes…* opens an editor for several lines of plain text. Save (or Cmd/Ctrl+Enter) keeps them, and Cancel or Esc leaves them as they were. A card with notes shows a small note mark after its title, and the notes' first non-blank line under the repo and branch, cut to the card's width. Saving empty notes removes them.
- **Project colors** (UR-65): a project's *Color ›* page offers None and the board palette (red, orange, yellow, green, teal, blue, purple, pink, gray). A colored project shows a dot of its color before its name and a stripe of it down the left edge of each of its thread cards. "No project" has no color.
- **Statuses are user-defined**: the Status page lists every status in use, plus "New status…" and "Clear status". A status (and its sidebar section) exists while some thread has it.
- **Drag** a thread onto a project, or onto "No project", to move it.
- **Drag a thread onto another thread** to reorder: it takes that thread's place, so it lands below a thread further down and above one further up, and it joins that thread's group (its project, pin and status). Dropped on a pinned thread, it is pinned and keeps its status. While dragging, the dragged thread is highlighted and an accent line shows where it would land. The parent-branch thread takes no drops.
- **Drag a project's header** onto another project to reorder: it takes that project's place, so it lands below a project further down and above one further up. While dragging, the dragged project is highlighted and an accent line shows where it would land (only where it would move). Escape or releasing elsewhere cancels.
- **New thread** (from a project's **+**, a repo's **+**, Cmd+N, or a task's Start thread): the repo, the **Project** and the **Start from** branch are chosen from lists (local branches, then remote ones). The project defaults to the one it was started from (none from a repo or Cmd+N), and the repo to that project's parent-branch repo; "Start from" defaults to the project's parent branch in that repo, else the repo's current branch; **None** leaves the thread ungrouped. The project selector is hidden while there are no projects.

## How it works
- Wire (ADR 0010: projects are `Folder`s): `CreateFolder`, `RenameFolder`, `DeleteFolder`, `UpdateThread{title?, pinned?, status?, folder_id?}` (empty status / folder clears), `CreateThread.folder_id`, `Workspace.folders`, `Thread.{folder_id, pinned, status}`. Protocol 1.2. `MoveFolder{folder_id, index}` since 1.11: the index counts positions without the moved project and is clamped to the end, like `MoveColumn`. `Workspace.folders` is in display order.
- The daemon validates everything in `service/projects.rs`: names are trimmed and at most 80 characters, statuses go through `urushi_core::Status` (trimmed, one line, at most 40 characters), and an unknown project is `NOT_FOUND`. The order lives in the stored list itself; `urushi_core::project::move_project` reorders it. `State::repair_interrupted` ungroups threads whose project is gone.
- The app turns UI gestures into those requests (`update/organize.rs`) and redraws from `WorkspaceChanged`. View-only state (which tab is open, collapsed sections, the open menu, an ongoing drag of a thread or project as `organize::Dragged`, and the project a dragged project would replace) lives in `AppState`. A project drop goes through `organize::project_drop_index`, which gives the target's index, or nothing when the project would stay put. `Msg::DragOver` while a project is dragged updates `project_over` for the drop line. Older daemons (minor < 11) get neither the drag nor the menu items (`AppState::daemon_supports_project_order`).
- Threads (ADR 0038): `MoveThread{thread_id, index}` since 1.24 moves a thread in the daemon's one stored thread list (`Workspace.threads`, in display order), with the same index rule as `MoveFolder`; `urushi_core::order::move_where` does the move (and backs `move_project` and `move_board`). The sidebar shows each group in that order, so the daemon needs no grouping rules. A drop on a thread (`Msg::DropOnThread`, handled in `update/sidebar_drag.rs`) sends an `UpdateThread` with whatever of project, pin and status differs (`organize::join_group`), then a `MoveThread` to the index from `organize::thread_drop_index`. `organize::same_group` and `thread_neighbours` decide groups and Move up/down (`view/menu/order.rs`). `thread_over` holds the thread whose place a dragged thread would take, for the drop line (`view/thread_card.rs`, which also makes every other card a drop target, drawn above the project's). Older daemons (minor < 21) get neither (`AppState::daemon_supports_thread_order`).
- Toolkit (`urushi-ui`): `Node::at` places a stack child at a point (menus, the drag label); `on_secondary_click`, `on_drag` and `on_drop` feed `Frame::secondary_click`, `drag_source` and `drop_target`. The runner turns a left press that moves 5 px into a drag (`runner.rs::drag_to`). `widgets::select_button` / `select_options` make the selectors.
- Links on cards: `WorkspaceMirror::thread_links` (`urushi-client`) reads every task's
  `thread_id`, `issue` and `pr_url` once per sidebar group into `ThreadLinks{issue,
  pull_request}` per thread. The first linked task (parents first) with each link gives
  it, since several tasks can be linked to one thread (ADR 0018).
  `view/thread_card.rs::link_marks` draws `brand_icons::mark`. These are Simple Icons'
  glyphs, pre-rendered as white 48 px masks in `assets/brands/` and tinted to
  `theme.text_muted`, each tint cached under its own image id. A click sends
  `Msg::OpenUrl` → `Effect::OpenUrl`, and the platform opens only `https://` URLs.
- Unread, notes and colors (ADR 0048, protocol 1.27):
  - Wire: `Thread.{unread, notes}`, `UpdateThread.{optional unread, optional notes}`, `Folder.color` (`LabelColor`), and `SetFolderColor{folder_id, color}`.
  - Validation: `urushi_core::thread_notes` drops trailing whitespace and refuses more than `MAX_THREAD_NOTES` (10 000) characters. `update_thread` checks it before changing anything, so a refused update changes nothing. `Service::set_project_color` sets or clears the color, and an unknown project is `NOT_FOUND`.
  - Storage: the stored JSON gets `unread`, `notes` and a project's `color` only when they are set, so older workspaces load unchanged.
  - App:
    - `ThreadMsg` (`msg/threads.rs`) is handled in `update/thread_extras.rs`.
    - `select_thread` calls `mark_read`, which sends `UpdateThread{unread: false}` only for an unread thread.
    - The notes editor is `Dialog::ThreadNotes` with `Field::ThreadNotes` (`view/thread_notes.rs`).
    - The thread menu lives in `view/menu/thread.rs`. The Color page (`view/menu/mod.rs::color_page`) is shared with tasks and columns.
    - `view/thread_card/title.rs` draws the unread dot, the note mark (drawn, since the bundled fonts have no note glyph) and the notes' first line.
    - `view/thread_card/mod.rs::with_stripe` draws the project's stripe. `view/projects.rs` draws the header dot and passes the color to its cards.
    - Older daemons (minor < 27) get none of the menu items and are never sent the requests (`AppState::daemon_supports_thread_extras`).
- CLI: `urushi project new|rename|rm|move <project> <index>`, `urushi project color <project> <color|none>`, `urushi thread unread|read <thread>`, `urushi thread notes <thread> [--set <text>|--set -|--clear]` (with neither option, it prints them), `urushi thread pin|unpin|status|move`, `urushi thread reorder <thread> <index>`, `urushi thread new <repo> --project <id>`; `urushi ls` shows pins, statuses, projects (with their color), `[unread]` and `[notes]`.

## Invariants
- Organizing never touches worktrees, branches or terminals (`service/projects.rs` only edits state; e2e test checks the worktree survives deleting the project).
- Unread marks, notes and colors are only organization: they never touch worktrees, branches or terminals (`tests/thread_extras.rs` checks the worktree survives).
- An `UpdateThread` is all or nothing: if any field is invalid, nothing changes (`threads_and_agents.rs`).
- Old workspaces load unchanged (`store.rs` and `thread.rs` tests).

## Tests
- Unread, notes and colors:
  - `urushi-core/src/thread.rs`, `thread/notes.rs` and `project.rs`: validation, and the new fields stored only when set.
  - `urushi-proto`: conversions and round trips, including an unknown color reading as none.
  - `urushi-daemon/tests/thread_extras.rs`: over the wire, setting and clearing each one, too-long notes refusing the whole update, an unknown project, and everything surviving a restart.
  - `urushi-app/src/update/tests/thread_extras.rs`: the menu items (and none for older daemons), opening an unread thread marking it read once, the notes editor saving and cancelling, the Color page, and how cards and headers look.
  - Snapshot scenarios `thread-extras`, `thread-notes` and `project-color-menu`.
- `crates/urushi-core/src/{status,thread}.rs`: validation; loading threads saved before the rename. `order.rs` and `project.rs`: moving items and projects (up, down, clamped, unknown).
- `crates/urushi-daemon/src/store.rs`: loading old documents. `tests/threads_and_agents.rs::threads_are_organized_into_projects_with_pins_and_statuses`: the whole flow over the wire, including a daemon restart. `tests/project_order.rs` and `tests/thread_order.rs`: reordering over the wire, `NOT_FOUND`, a thread keeping its project, pin and status, and the order surviving a restart.
- `crates/urushi-client/src/workspace.rs`: grouping and the status list. `thread_links.rs`: a thread's issue and pull request (open first, invalid links ignored, the first linked task wins).
- `crates/urushi-app/src/view/thread_card.rs`: cards show the marks only when linked; a click on one opens its page and not the thread. `brand_icons.rs`: tinted masks with their own ids.
- `crates/urushi-ui/tests/layout.rs`: positioned children, secondary clicks, drag sources and drop targets.
- `crates/urushi-app/src/update/tests/organize.rs` and `view/mod.rs`: menus, prompts, drag and drop, selectors, sidebar order and collapsing. `update/tests/project_order.rs`: dragging projects, the drop line, Move up/down, drop targets and older daemons. `update/tests/thread_order.rs`: the same for threads, plus joining another group (status, pin, project, ungrouped) and dropping on the parent-branch thread.
- Snapshot scenarios `menu`, `menu-status`, `drag`, `drag-thread`, `drag-project`, `repos`, `new-thread-base`, `new-thread-project`, `thread-links` (cards with Jira and GitHub marks; see the GUI doc).

## Limitations and follow-ups
- Notes are plain text, without formatting or history. Two clients editing the same notes at once: the last Save wins.
- Nothing marks a thread unread automatically. A finished or waiting agent shows through [agent attention](agent-attention.md) instead.
- Projects can't be colored from the new-project prompt; color one from its menu after creating it.
- Statuses can't be reordered; they sort alphabetically.
- A dragged project can't be dropped onto "No project" to send it to the bottom; drop it on the last project instead.
- A dragged project or thread doesn't scroll a sidebar that is taller than the window.
- Dropping a thread on a status section's header, or on a project's empty space, doesn't set the status (it moves the thread to that project only); drop it on a thread in the section instead. A thread can't be dropped at the end of a group, below its last thread; drop it on the last thread.
- A thread's drop is two requests (`UpdateThread`, then `MoveThread`), so it isn't atomic: if the first fails, the thread still moves in the list but keeps its group.
- Context menus aren't reachable from the keyboard or accessibility actions yet.
- Removing a repo is only available from the CLI (`urushi repo rm`).
- The marks have no tooltip (the toolkit has none); their accessible labels name the issue or pull request.
- Only GitHub pull requests show; GitLab and Bitbucket ones from Jira are left out (ADR 0036).
