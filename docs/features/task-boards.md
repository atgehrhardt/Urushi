# Feature: Task boards

- **Status:** Shipped (manual boards, columns, colors, tags, kinds, nested subtasks, attachments, threads started from tasks; Jira boards in [jira-boards](jira-boards.md))
- **Owners (crates):** `urushi-core` (`board`, `board/{tree,labels,columns,order,prompt,quick}.rs`), `urushi-daemon` (`service/{boards,columns,attachments}.rs`, `attachments.rs`, `server/boards.rs`, the link in `service/threads.rs` and `service/task_tabs.rs`, unlinking in `service/deletion.rs`), `urushi-proto` (`convert/boards/`), `urushi-client` (`boards.rs`, `tasks.rs`, `WorkspaceMirror::{boards, board, column, task, find_task}`), `urushi-app` (`board.rs`, `update/board.rs`, `update/cards.rs`, `update/tasks.rs`, `update/task_nav.rs`, `update/task_threads.rs`, `update/attachments.rs`, `files.rs`, `platform/`, `view/board/` (tabs in `tabs.rs`, the header in `header.rs`, the phone layout in `phone.rs`), `scrolling.rs`, `view/tasks/`, `view/labels.rs`, `view/menu/{tasks,boards}.rs`), `urushi-ui` (`Theme::labels`), `urushi-cli` (`urushi board`, `urushi thread new --task`, `urushi tab new --task`)
- **Related ADRs:** [0050](../adr/0050-draw-what-shows-and-redraw-only-on-change.md), [0047](../adr/0047-board-columns-keep-a-width-and-scroll-sideways.md), [0046](../adr/0046-one-board-column-at-a-time-on-phones.md), [0037](../adr/0037-quick-filters-written-to-jira-or-kept-here.md), [0025](../adr/0025-new-subtasks-in-their-own-editor.md), [0002](../adr/0002-daemon-source-of-truth.md), [0008](../adr/0008-json-document-store.md), [0012](../adr/0012-subtasks-as-checklists-and-thread-links.md), [0015](../adr/0015-subtasks-are-nested-tasks.md), [0016](../adr/0016-daemon-stores-attachments.md), [0014](../adr/0014-task-prompt-via-pane-environment.md), [0018](../adr/0018-start-tasks-in-existing-threads.md)

## What it does
A **Tasks** button sits under the sidebar's Projects | Repos tabs. It shows the number of
open tasks, meaning tasks not in a board's last column. The button, or **Cmd+B**
(Ctrl+Shift+B off Apple platforms), slides a kanban panel out from under the sidebar, over
the terminals, as frosted glass that blurs them. Escape, ×, the button or the shortcut slide
it back.
- Boards are tabs across the top. **+** creates a board, and **Rename** and **Delete**
  act on the board shown. Deleting asks first and says how many tasks go with the board.
- **Reordering boards** (1.17 daemons): drag a board's tab onto another tab to take its
  place: it lands right of a tab further right and left of one further left. While
  dragging, the dragged tab is highlighted, an accent line at the edge of the tab under
  the pointer shows where it would land (only where it would move), and the board's
  name follows the pointer; Escape or releasing elsewhere cancels. Right-clicking a tab
  offers **Move left** / **Move right** where the board can go that way. Jira boards move
  like any other, and nothing is sent to Jira. New and imported boards go last. The order
  is the daemon's, so every client shows the same one.
- A new board has the columns **To do**, **In progress** and **Done**. Columns can be
  changed from their **⋯** button or a right-click on the header: **Rename…**,
  **Color ›**, **Move left/right**, **Add column…** and **Remove column…**. **+ Column**
  after the last one adds another (at most 20). Removing a column asks first and says
  where its tasks go: the column on its left, or on its right for the first column.
  The last column can't be removed.
- **Colors** come from a fixed palette (red, orange, yellow, green, teal, blue, purple,
  pink, gray). A colored card has a stripe down its left edge and a tinted background;
  a colored column has a bar across its top and a faint tint.
- **Tags** are free text. Each tag has a color picked from its name, so the same tag
  looks the same everywhere. Cards show up to three tags in their footer, then "+n". In
  the editor, type in "Add a tag…" and press Enter, × removes one, and the board's other
  tags are offered under "On this board".
- **Filtering:** under the tabs, a search field (title and notes; Escape clears it) and
  **Filters ▾**, with a page for the types and one for the tags the board's tasks have;
  several can be chosen, and a card shows when it or a subtask passes. The column
  counts then read "shown/all", and **Clear** drops every filter. Choosing another board
  clears them. Jira boards filter by much more ([jira-boards](jira-boards.md)). A field's
  page also lists values chosen that no task has any more, so they can be unticked. A
  chip that is on keeps its accent color under the pointer. A long **Filters ▾** menu
  stops at the window's edge and scrolls, and each field's page keeps its own scroll.
- **Large boards** (UR-66): a column builds only the cards in sight (all cards are the
  same height; spacers stand in for the rest), the filter's results are kept until the
  board or the filter changes (`filter/cache.rs`), and moving the pointer redraws only
  when it enters or leaves something clickable (ADR 0050). A 500-issue board takes about
  2 ms of view and layout per frame, down from about 18 ms.
- **Quick filters** (1.23 daemons, ADR 0037): **Save filter…** in the bar (or **Save as
  quick filter…** in **Filters ▾**) saves the search, types and tags chosen under a name.
  It becomes a chip after the search field, and an entry in **Filters ▾**, that turns
  it on and off. Right-click a chip for **Edit…** (rename it, or **Use current filters**)
  and **Delete…** (asks first). A board keeps up to 20. On a Jira board quick filters
  are JQL and are saved to Jira ([jira-boards](jira-boards.md)).
- **+** on a column adds a task. Clicking a card opens the editor: title, notes and
  subtasks on the left; column, color and tags on the right (one scrolling column on
  phones); **Start thread**, and Delete. Color and tags are saved with Save, like the
  title and notes. The editor grows with what it holds up to what leaves its buttons
  on screen, then scrolls, so **Save** is always visible. Clicking blank space inside
  it does nothing; only a click outside it closes it.
- **Notes** take several lines: the box is five lines tall, wraps, and scrolls. Enter
  starts a new line there; Cmd+Enter (Ctrl+Enter elsewhere) saves from any field. Enter
  in the title moves on to the notes: it never saves, so a half-typed task is never
  created (on a Jira board, never created in Jira).
- **Nothing is saved before Save.** Closing an editor with unsaved edits (Cancel,
  Escape, a click outside) asks **Discard unsaved changes?** with **Keep editing** and
  **Discard**; closing again keeps editing.
- **Cards are all the same size** whatever they hold: the title on one line, up to two
  lines of notes (blank lines skipped, an ellipsis when there's more), and a footer row
  for subtask progress and the thread chip. The copy that follows the pointer while
  dragging is the same card.
- **Subtasks** are tasks too, kept inside their parent (ADR 0015). In the editor: type
  in "Add a subtask…" and press Enter to add one by its title, or press **New
  subtask…** to write one in an editor of its own (ADR 0025): title, notes, type, color,
  tags, and on a Jira board the sub-task's own fields (required ones asked for, as for
  a new issue). **Done** or **‹** puts it in the task's list, where it opens again as it
  was left ("details" marks one with more than a title); **Remove subtask** drops it;
  **Save** in either editor saves the task with it. Added subtasks are created with
  everything they have on Save (with a new task, or under an existing one). Click the box to check one off
  and × to delete one; those are immediate. Cards show progress ("✓ 1/3", green when
  all are done), and subtask rows their own (with the kind and key of a Jira issue).
  - **Clicking a subtask's title opens it** as a task of its own ("Edit subtask"), with
    notes, color, tags, its own subtasks and thread. The editor it came from is kept,
    unsaved edits and all: the top bar shows **‹** (back to it, as it was left) and
    where the task is, as Jira does: the epic on a Jira board (opening it in Jira), then
    each task it is part of (an issue's kind and key, otherwise its title), each opening
    that editor again, then the task itself. On a phone only the parent shows
    (`‹ … / PARENT-1 / SUB-2`). **Save** saves every editor on the way; **Discard**
    drops them all. Instead of a column, a
    subtask has **Done / Not done** and **Make standalone**, which moves it onto the
    board in its card's column, unchecked, with its subtasks. Subtasks nest up to five
    levels under a card.
  - **Make subtask of…** (in a task's menu, card or subtask) lists the board's other
    tasks at any depth ("Card › Subtask"), minus the task's own subtree and its current
    parent; type to narrow it, click one or press Enter for the first. A card from the
    last column arrives checked off.
  - Right-clicking a subtask row opens the same menu as a card, over the editor.
- **Start thread** (on a task, or on any subtask row) opens the new-thread dialog with
  the title filled in, in the repo and project of the parent task's thread or else the
  thread on screen. Its **Project** selector files the thread under any project, or
  **None** to leave it ungrouped (`Msg::PickProject`). The new thread is linked to the
  task or subtask: the button then reads **Go to thread**, and the card shows a
  "→ thread" chip that goes there. Deleting the thread clears the link. With a task agent chosen (setup wizard or Settings), the thread starts
  it with the task, its notes and its subtasks as the first prompt; otherwise it opens on
  the launcher. See [task agent](task-agent.md).
- **Start in an existing thread:** the same dialog has a **New thread | Existing thread**
  switch (1.10 daemons). Existing lists the ready threads ("repo › title") with the
  card's or on-screen thread picked first. **Start in thread** opens a new tab there,
  running the task agent with the task as its prompt (a plain terminal without one), and
  links the task to that thread (ADR 0018).
- **Pull request** (ADR 0036, 1.22 daemons): an existing task on a local board has a
  **Pull request** field under its tags. It takes the pull request's `https://` address
  (anything else is pointed out, and the daemon refuses it) and is saved with Save;
  **Open** opens it. Emptying it unlinks it. The thread linked to the task then shows
  GitHub's mark on its card ([thread organization](thread-organization.md)). A Jira
  board has no such field, because its issues' pull requests come from Jira
  ([jira-boards](jira-boards.md)).
- **Agent attention on cards:** when an agent in a thread linked to the task, or to any
  of its subtasks at any depth, finishes or waits for the user, the card gets a
  **Finished** (green) or **Needs input** (amber) badge by its title and a border in that
  color. Waiting outranks finished (`AppState::task_attention`). The thread chip and the
  editor's subtask rows show the same dot. See [agent attention](agent-attention.md).
- **Attachments:** the editor's **Attachments** list shows each file (an image's
  thumbnail, or the file's type), its name and size, and × to delete it; clicking one
  opens it with the system's app. **Add files…** opens the file picker, and files
  dropped on the window while a task is open are attached to it. Files are at most
  6 MiB, 20 per task. A card shows its first image as a small thumbnail beside the notes,
  and "n files" in its footer. A new task gets attachments once it is added. Phones
  show attachments but can't attach or open them yet.
- **Right-click or two-finger click** a card for its menu: **Open**, **Move to ›** (a
  column), **Make subtask of…**, **Color ›**, **Start thread…** / **Go to thread**, and
  **Delete task**, which deletes it
  right away, as the editor's Delete does. On a Jira board it is **Delete issue…**, and
  it asks first: the issue is deleted in Jira (see [Jira boards](jira-boards.md)).
- Long names are cut short with an ellipsis instead of running over their neighbours:
  subtask titles in the editor, column names, board tabs and the card's thread chip.
- **Drag** a card onto another card to put it above that card, or onto a column to put it
  at the bottom. While dragging, the card's place stays as a faint outline, an accent line
  shows where it would land (only where it would move), the column under the pointer is
  outlined, and a raised copy of the card follows the pointer.
- **Width** (ADR 0047): columns share the panel's width but never get narrower than
  `BOARD_COLUMN_MIN_WIDTH` (280 pt). With more columns than fit (an unfolded foldable, a
  narrow window, a wide sidebar), the board scrolls sideways: trackpad, Shift+wheel or a
  finger. A card dragged near the left or right edge scrolls the board that way. The
  board tabs and the filter chips scroll sideways too.
- **A narrow panel** (under 880 pt, `view::board::narrow`) has the board's actions behind
  **⋯** (as on a phone) and only the search and **Filters ▾** in the filter bar.
- **On phones** (ADR 0046) the panel covers the whole screen and shows one column at a
  time, at full width (`view/board/phone.rs`):
  - The header is the board picker (**Name ▾**: every board, then New board… and Import
    from Jira…), **⋯** (Rename… and Delete…, or a Jira board's sync state, Sync now and
    Remove…, then Move left / right) and ×.
  - Under the search and **Filters ▾**, a tab per column shows how many of its tasks the
    filter shows, and its name. Tap a tab, or swipe sideways on the board, to change
    column; another board starts at its first. A local board's tabs end in **+** for a
    new column.
  - Long-press a card and drag it onto a column's tab to move it to the bottom of that
    column, or within the column to reorder it.

- **Kinds** (1.12): the editor's **Type** makes a task a task, story, bug or epic (click
  the chosen one again to clear it). Cards show a small marker in the kind's color before
  the title.
- **Jira boards** are further tabs in the same panel: see [Jira boards](jira-boards.md).

Boards live in the daemon, so every client sees the same boards, and they survive
restarts.

## How it works
- **Model** (`urushi_core::board`): `Board{columns[Column{color, tasks[Task{done, color,
  tags, attachments[Attachment], subtasks[Task], thread}]}]}`, a forest (`tree.rs`): `task`, `path_to`,
  `column_of`, `remove_task` and `link_thread` work at any depth; `move_task` takes any
  task into a column (clearing `done`); `nest_task` puts one under a parent, refusing its
  own subtree (`Cycle`), more than `MAX_DEPTH` levels (`TooDeep`) or `MAX_SUBTASKS`
  children, and checks room before the task moves. The rules live here: names, titles and tags are
  validated (`labels.rs::task_tags`: whitespace collapsed, repeats ignoring case
  dropped, at most `MAX_TAGS` of `MAX_TAG` characters), `update_task` takes a
  `TaskPatch`, columns are added, recolored, moved and removed in `columns.rs` (at
  least one, at most `MAX_COLUMNS`; `column_heir` is where a removed column's tasks
  go), `tag_color` hashes a tag's name (FNV-1a, stable across releases) to a palette
  color, `move_task` takes an
  index counted without the moved task, clamped to the bottom, a task has at most
  `MAX_SUBTASKS`, and `link_thread` / `unlink_thread` record and forget threads.
  `Task::pr_url` is a `PrUrl` (`pull_request.rs`: `https://` with a host, no
  whitespace, at most `MAX_PR_URL`), set through `TaskPatch::pr_url` and refused on a
  Jira board.
  The list of boards is kept in tab order; `move_board` (`order.rs`) reorders it.
  `Board.quick_filters` (`quick.rs`) holds `QuickFilter{id, name, rule}`, where
  `QuickRule::Fields{search, kinds, tags}` applies to boards of Urushi's own and
  `QuickRule::Jql` to Jira boards; `add`/`update`/`remove_quick_filter` validate the name
  and rule (`checked`) and keep at most `MAX_QUICK_FILTERS`. The daemon's
  `service/quick_filters.rs` applies them in `mutate`; clients evaluate `Fields` rules
  (`filter/quick.rs::passes`, through `filter::Context.kept`). Ids and
  timestamps are passed in. Older documents' checklist items load as tasks (every new
  field has a serde default; `created_at` reads as 0).
- **Daemon:** `Service::{create_board, rename_board, move_board, delete_board, create_task,
  update_task, move_task, nest_task, delete_task, create_subtask}` (`service/boards.rs`;
  `UpdateSubtask` and `DeleteSubtask` are served by `update_task` and `delete_task`) and `Service::{create_column, update_column, move_column,
  delete_column}` (`service/columns.rs`) change `State.boards` through `mutate`. That persists the change
  in the JSON document (`Document.boards`; older files have none, and older tasks no
  subtasks or link) and wakes `WorkspaceChanged` subscribers.
- **Attachments** (ADR 0016): `Service::{add_attachment, get_attachment,
  delete_attachment}` (`service/attachments.rs`) over the store in `attachments.rs`
  (`<home>/attachments/<id>`, thumbnails in `<id>.thumb`). Adding checks the size and
  name (`board::file_name` keeps the last path component), takes the media type from
  the contents (`board::media_type`), decodes images with `image` under decode limits
  (refusing broken ones) and writes the file and thumbnail off the runtime, then records
  the attachment; if recording fails the file is removed. Deleting a task, subtask or
  board removes its files after the change is saved, and `sweep_attachments` deletes
  unnamed files at start.
- **Linking threads:** `CreateThread{task_id, subtask_id}` checks the task exists (and
  that `subtask_id`, if set, is somewhere under it) before making the worktree, then links it in the same `mutate` that records the thread
  (`threads.rs::finish_thread_creation`). If the task was deleted meanwhile, the thread
  is kept unlinked. `CreateTab{task_id, subtask_id}` links the task to an existing thread
  in the same `mutate` that records the new tab (`task_tabs.rs::create_task_tab`), moving
  the link from any earlier thread. Deleting a thread unlinks it from every board
  (`deletion.rs`). See ADR 0012 for why the link lives on the task and is made by the
  daemon, and ADR 0018 for linking through `CreateTab`.
- **Wire (1.5, 1.6):** `Workspace.boards`, plus `CreateBoard` → `Board`, `RenameBoard`,
  `DeleteBoard`, `CreateTask{subtasks}` → `Task`, `UpdateTask{optional title, optional
  notes}`, `MoveTask{column_id, index}`, `DeleteTask`, and since 1.6 `CreateSubtask` →
  `Subtask`, `UpdateSubtask{optional title, optional done}`, `DeleteSubtask`,
  `Task.thread_id`, `Subtask.thread_id`. Since 1.8: the `LabelColor` enum
  (`UNSPECIFIED` is no color, and unknown values read as none), `BoardColumn.color`,
  `Task.{color, tags}`, `CreateTask.{color, tags}`, `UpdateTask.{optional color, TagList
  tags}` (a set `UNSPECIFIED` clears the color; an unset `tags` keeps them), and
  `CreateColumn` → `BoardColumn`, `UpdateColumn{optional name, optional color}`,
  `MoveColumn`, `DeleteColumn` (the last column is `FAILED_PRECONDITION`). Also since
  1.8, `Subtask.{notes, created_at_ms, color, tags, subtasks}` (a subtask carries every
  task field, so 1.6 clients still read the first level as a checklist), `NestTask{task_id,
  parent_id, index}`, and nested ids in `UpdateTask`, `MoveTask` (which takes a subtask
  out onto the board), `DeleteTask` and `CreateSubtask`. Since 1.14 a subtask is
  created with its details: `NewSubtask{title, notes, color, tags, kind, jira_fields}`
  in `CreateTask.subtask_details` (after any `subtasks` titles), and the same fields on
  `CreateSubtaskRequest` (daemon: `service::NewSubtask`, `server/boards.rs`). Since
  1.17, `MoveBoard{board_id, index}`: the index counts positions without the board and
  is clamped to the end, like `MoveColumn` and `MoveFolder`; an unknown board is
  `NOT_FOUND`. `Workspace.boards` is in tab order.
- **App:** `AppState.board: BoardPanel` holds what the panel is doing: open, slide
  position, the chosen board, the card pressed or dragged, and where it would land
  (`over`). The runner animates `reveal` over 180 ms (`runner.rs::animate`), and the view
  slides the panel with the toolkit's `Node::slide_x` inside a clipped stack
  (`view/mod.rs::with_board`).
  - **Clicks:** a press on a card only marks it. The runner sends `Msg::PressEnded` on a
    release that wasn't a drag, and that opens the editor. A drag sends `DragTask`, then
    `DragOver` with the drop target under the pointer as it moves, then `DropTask{column,
    before}`. `board::drop_position` turns a target into a `MoveTask`, or into nothing
    when the card would land where it already is; `DragOver` uses it to place the line.
  - **No flicker:** each card sits in a slot that includes the gap under it, and the drop
    line in a slot with the same target (`view/board/column.rs::slot`), so the pointer is always
    over a target and the line doesn't jump as it moves the cards.
  - **Editor** (`update/tasks.rs`, `update/task_nav.rs`): `Dialog::Task` edits a card or
    a subtask alike; the app reads both through `urushi_client::{TaskRef, FoundTask}`
    (`WorkspaceMirror::find_task`, which also gives the path for the top bar,
    `view/tasks/header.rs`, drawn with `widgets::modal_with_header`).
    `BoardMsg::OpenTask` from the editor sends nothing: the editor on screen goes onto
    `EditorNav::back` with its edits, and opening a task already there returns to it
    (those opened after it go); `BoardMsg::EditorBack` pops one. Save
    (`editor_changes`) sends `task_changes` for every editor in `back` and then the one
    on screen, whose `UpdateTask` carries `Followup::CloseDialog`, and clears the
    earlier editors' drafts so saving again can't add a subtask twice. Closing with
    edits (`editor_dirty`) sets `EditorNav::confirm_discard` instead
    (`hold_editor_close`); `BoardMsg::{KeepEditing, DiscardEdits}` answer it. A
    subtask's check mark changes right away; a subtask is deleted with `DeleteSubtask`.
    Subtasks added in the editor are drafts in `SubtaskEditor` until Save, each a
    `drafts::SubtaskDraft`: the whole `Dialog::Task` it was written in.
    `BoardMsg::NewSubtask` / `OpenSubtaskDraft(i)` open one on top of the editor, like
    `OpenTask`, with `EditorNav::draft_of_previous` naming its place in the list;
    `fold_draft` (`update/subtask_drafts.rs`) puts it back on Back, Done, Save and
    `OpenTask`, and `drafts_problem` holds Save while one has no title or misses a
    required Jira field. On a 1.14 daemon a new task sends them as
    `CreateTask.subtask_details` and an existing one a detailed `CreateSubtask` each;
    older daemons get `CreateTask.subtasks` titles and no **New subtask…**.
    Enter in "Add a subtask…" or "Add a tag…" adds; Enter in the notes (a
    `TextField::multiline`, drawn with the toolkit's `text_area`) adds a line; Enter in
    the title moves to the notes (`task_editor_enter`); Enter elsewhere, or
    Cmd/Ctrl+Enter anywhere, saves (a subtask still typed in the field goes with it).
    Notes are sent only if they differ from the task's notes as the editor shows them,
    so opening and saving changes nothing.
  - **Cards** (`view/board/card.rs::card_body`): a fixed `card_height` from the theme's line
    heights; the notes are a `widgets::wrapped_text` filling the room between the title
    and the footer.
  - **Attachments** (`update/attachments.rs`, `files.rs`): `update` never touches
    files. **Add files…** asks the runner for `Effect::PickFiles` (`platform::pick_files`,
    rfd), a drop on an open task's editor for `Effect::ReadFiles`; the runner reads each
    file (`files::read_attachment`, refusing folders and files over the limit) and
    sends it back as `BoardMsg::Attach`, which becomes `AddAttachment`. Opening sends
    `GetAttachment`, and the answer becomes `Effect::OpenFile`: the runner saves it to
    `<temp>/urushi-attachments/<id>/<name>` and calls `platform::open_path` (`open`,
    `xdg-open`, Explorer). Thumbnails: after every message, `want_thumbnails` asks for
    the ones on screen (`board::wanted_thumbnails`: each card's first image, and every
    image of the open task) that weren't asked for, and keeps at most `MAX_THUMBNAILS`;
    they arrive as `ImageData` with an id hashed from the attachment's.
  - **Nesting:** `Dialog::NestTask` lists `board::nest_candidates`, and choosing one
    sends `NestTask`; **Make standalone** sends `MoveTask` to the card's column.
    `TaskLink::of` names the card and the subtask for `CreateThread`.
  - **Menus:** the card's (or a subtask row's) `on_secondary_click` opens
    `MenuTarget::Task` (the only menu that opens over a dialog, the task editor), a column's
    header or ⋯ `MenuTarget::Column` (both in `view/menu/tasks.rs`; colors are a
    `MenuPage::Color` page), and the filter bar's **Filters ▾** `MenuTarget::Filter`
    (`view/menu/filter.rs`). Their actions are `Msg::Board(BoardMsg::…)`, handled in `update/cards.rs`.
    Moving a card there sends `MoveTask` to the bottom of the column. Column names go
    through `Prompt::{NewColumn, RenameColumn}`, and removal through
    `Dialog::DeleteColumn`, which names the same neighbour the daemon picks.
  - **Labels:** the editor's color and tags are drafts in `TaskLabels`
    (`board.rs`), sent with Save only when they changed; a tag still typed in the field
    goes too. `view/labels.rs` maps wire colors to `Theme::labels` and draws swatches
    and tag chips, wrapping chips into rows by their approximate width (the layout
    doesn't wrap). The filter (`BoardPanel.filter`, `filter.rs`) only
    changes what is drawn; drops still use task ids, so they land right.
  - **Board order** (`view/board/tabs.rs`, `update/board.rs`): a tab's `on_drag` sends
    `Msg::DragBoard`, which starts `AppState::dragging` as `organize::Dragged::Board`
    (so the pointer label, `DragMove` and `DragCancel` are shared with the sidebar's
    drags). While it lasts every tab is an `on_drop` target for `Msg::DropBoard`;
    `DragOver` keeps `BoardPanel::tab_over` for the line, and a drop goes through
    `organize::board_drop_index` (the same rule as `project_drop_index`) into
    `MoveBoard`, or nothing when the board would stay put. Each tab always has a 3 px
    slot at each end where the line is drawn, so showing it never moves the tabs. The
    tab's menu is `MenuTarget::Board` (`view/menu/boards.rs`), whose items send
    `BoardMsg::MoveBoard`. Closing the panel or Escape ends the drag. Older daemons
    (minor < 17, `daemon_supports_board_order`) get neither, and a single board offers
    neither.
  - **Shortcut:** `B` in `update.rs::shortcut` sends `Msg::ToggleBoard`, the same message
    as the button.
  - **Input:** while the panel is out, `AppState::terminals_covered` keeps the mouse,
    wheel and typing away from the terminals.
  - Subtask controls and Start thread need a 1.6 daemon (`daemon_supports_subtasks`);
    colors, tags and column changes a 1.8 one (`daemon_supports_board_extras`).
- **CLI:** `urushi board show <task|KEY>` prints a task in full (where it is, notes,
  subtasks, attachments; a Jira issue's fields, comments, links and files), as agents
  started from a task read it (ADR 0024). `urushi board ls|new|rename|order|rm|add|edit|mv|nest|rm-task` (`order <board> <index>`
  moves a board's tab; 0 is first) (`ls` prints
  subtasks as an indented tree; `mv` takes a subtask out onto the board; `edit`,
  `rm-task` and `sub add` take subtask ids), `urushi board attach <task> <file>`,
  `urushi board attachment get <id> [--thumbnail] [-o file]|rm <id>` (`add --color --tag`,
  `edit --color <c|none> --tag --untag --pr <url|"">`; `show` prints `Pull request:`
  lines), `urushi board col add|rename|color|mv|rm`,
  `urushi board sub add|done|undo|rename|rm`, `urushi board quick
  ls|add|edit|rm <board> …` (`--search`, `--kind`, `--tag`; a filter by name or id), and `urushi thread new <repo> --task <task or subtask>`.
  Columns can be named, e.g. `urushi board mv <task> done`.

## Invariants
- A task is in exactly one column, and no operation loses or duplicates one, adding,
  moving and removing columns included; a board always keeps a column
  (`crates/urushi-core/tests/board_props.rs`).
- A failed operation changes nothing (`board/tests.rs::failed_operations_change_nothing`).
- Quick-filter operations never lose, duplicate or reorder a board's quick filters, keep
  more than the bound, or change anything when they fail
  (`tests/quick_filter_props.rs`).
- Reordering boards never loses or duplicates one, and an unknown board changes nothing
  (`board/order.rs` tests, `tests/board_order.rs`).
- A task is in one place, a column or one parent; nesting, moving and removing never
  lose, duplicate or cycle a task, and stay within `MAX_DEPTH`
  (`tests/board_props.rs::nesting_never_loses_duplicates_or_cycles_a_task`). Subtasks
  move and are deleted with their task, and a task holds at most `MAX_SUBTASKS`
  (`board/tree/tests.rs`).
- A link never creates, changes or deletes a thread. A missing task refuses
  `CreateThread` before any worktree exists, and deleting a thread clears its links
  (`tests/boards_and_usage.rs::threads_started_from_tasks_are_linked_until_deleted`).
- Deleting a board needs the confirmation dialog. Enter doesn't confirm
  (`update/tests/board.rs::boards_are_created_selected_and_only_deleted_after_confirming`).
- A recorded attachment always has its file: the file is written first and removed if
  recording fails; refused files leave nothing behind, and deleted tasks take their
  files with them (`tests/board_attachments.rs`).
- The panel never forwards input to the terminals under it
  (`update/tests/board.rs::the_panel_covers_the_terminals_until_closed`).

## Tests
- Core: `board/tests.rs`, `board/tree/tests.rs` (nesting, bounds, links, old
  documents), `board/order.rs` (moving boards), `board/attachments.rs` (names, media types, bounds), `board/prompt.rs` and `tests/board_props.rs`.
- Pull requests: core `board/pull_request.rs` (URLs, labels, the one shown),
  `board/tests.rs` (linking at any depth, old documents); proto
  `convert/boards/tests.rs` (round trips, an invalid link reads as none); daemon
  `tests/jira_pull_requests.rs::a_local_task_links_a_pull_request_and_a_jira_issue_cannot`;
  app `update/tests/pull_requests.rs` (the field, Save, unlinking, Jira boards and
  older daemons).
- Proto: `convert/boards/tests.rs` covers round trips and wire encoding of every 1.5,
  1.6 and 1.8 board message (`tests/round_trip.rs` covers `MoveBoard`), and that the largest attachment fits in a frame;
  `convert/boards/labels.rs` the color mapping.
- Daemon: `tests/board_order.rs` (`boards_are_reordered_and_keep_their_order`: over
  the wire, `NOT_FOUND`, and across a restart), `tests/board_columns.rs`
  (`columns_colors_and_tags_are_kept_and_survive_a_restart`), `tests/board_nesting.rs`
  (`cards_and_subtasks_move_into_and_out_of_each_other`), `tests/board_attachments.rs`
  (`attachments_are_stored_fetched_and_deleted_with_their_task`), `attachments.rs`
  (decoding, thumbnails, the store and its sweep), `tests/task_agents.rs` (a
  deep subtask's link and prompt),
  `tests/boards_and_usage.rs` (`boards_are_kept_mirrored_and_survive_a_restart`,
  `subtasks_are_created_checked_renamed_and_deleted`,
  `subtasks_are_created_with_their_details`,
  `threads_started_from_tasks_are_linked_until_deleted`).
- App: `board.rs` (drop positions, open-task count), `update/tests/board.rs` (the card
  menu, editor,
  multiline notes and Cmd+Enter, the board shortcut, drag and drop, prompts, deletion,
  keyboard) and `update/tests/tasks.rs` (subtasks,
  drafts saved only on Save, opening subtasks, Back and the crumbs keeping unsaved edits,
  asking before discarding them, making them standalone, nesting, starting and going to
  threads, the drop line, older daemons), `update/tests/subtask_drafts.rs` (a new
  subtask's own editor: its details, Back keeping it, removing it, Jira's required
  fields, older daemons) and
  `update/tests/labels.rs` (colors, tags, columns, the filter),
  `update/tests/quick_filters.rs` (saving the selection, turning the filter on, renaming
  or replacing its rule), `filter/quick/tests.rs` (rules and the selection), the
  `board-quick-filters` snapshot, `tests/quick_filters.rs` in the daemon (kept across
  restarts, refusals),
  `update/tests/board_order.rs` (dragging tabs, the drop line, Move left/right,
  Escape and closing the panel, a single board, older daemons) and
  `update/tests/attachments.rs` (picking, dropping, opening, deleting, thumbnails);
  `files.rs` (reading and saving files). `snapshot.rs` has
  the `board`, `board-drag`, `board-tab-drag`, `task`, `task-menu`, `column-menu`, `task-nested`,
  `task-new-subtask` and `nest` scenarios, which render at desktop and
  phone sizes, `task_cards_are_all_the_same_size`,
  `snapshot/phone_board.rs` (one column of full-width cards and a header that fits at
  393 pt), `update/tests/board_phone.rs` (tabs, swipes and when they're ignored, dropping
  on a tab, the board picker and **⋯**), `board.rs::a_phone_shows_the_chosen_column_and_steps_within_the_board`,
  `snapshot/board_width.rs` (columns keep their width and scroll at 760 pt, share 1600 pt,
  and a narrow panel's header), `scrolling.rs` (wheel axes, clamping, edge scrolling),
  `a_new_subtask_has_an_editor_of_its_own`,
  `the_task_editors_buttons_stay_on_screen_in_a_short_window` and
  `long_subtask_titles_are_cut_short_inside_the_editor`.
- Large boards (UR-66): `snapshot/large_board.rs` (`jira-large-board`: only the cards
  in sight are built and a second frame shapes no new text; `jira-large-filter-menu`:
  the menu stops at the window and scrolls), `view/board/column.rs::window`,
  `filter/cache/tests.rs` (the memo answers as the filter does and starts again on a
  change), `view/board/filter.rs` (avatars by name, a hovered chip that is on),
  `urushi-ui/tests/layout.rs::a_long_scrolled_list_paints_and_hits_only_the_rows_that_show`
  and `only_moves_that_change_hover_need_a_new_frame`, `text_cache.rs`, and
  `text.rs::truncating_a_long_text_measures_only_what_could_fit`.
- Toolkit: `urushi-ui/tests/layout.rs::a_sliding_panel_moves_with_its_children_and_is_clipped`,
  `modal_covers_the_app_and_dismisses_on_backdrop` (a click inside the card doesn't),
  `a_max_height_caps_a_fit_box_which_then_scrolls`;
  `urushi-ui/tests/fields.rs` (text areas wrapping, clicks on lines, scrolling, and
  `wrapped_text`'s ellipsis); `textfield/tests.rs` (multiline editing and moves);
  `text.rs::wraps_at_line_breaks_then_between_words`.

## Limitations and follow-ups
- Attachments are at most 6 MiB (one frame each); bigger files need chunked transfer.
  Phones can't attach or open files yet, and images can't be pasted from the clipboard.
- Thumbnails share the renderer's glyph atlas; many large ones on screen could churn it.
- Tags are per task; there is no place to rename or recolor a tag across a board.
- A quick filter on a board of Urushi's own holds one search, kinds and tags. Quick
  filters that are on when saving aren't folded into the new one. Phones list them in
  **Filters ▾**, but can't edit or delete them yet (no right-click).
- Subtasks can't be reordered by dragging (`NestTask` takes an index, but the editor
  doesn't offer it), and cards can't be dragged onto a task to nest them.
- On a 1.6 or 1.7 daemon subtasks are a checklist: their titles don't open, and they
  can't be renamed in the app (rename them with `urushi board sub rename`).
- In the notes, Up/Down move between lines separated by line breaks, not between the
  wrapped lines of a long paragraph (the editing model doesn't know the layout).
- A card's title is one line, cut short with an ellipsis.
- A new task can't be given a pull request until it is saved: `CreateTask` has no
  `pr_url`, so the field shows only on an existing task.
- A local task links one pull request; the card can't show its state (open or merged),
  since nothing reads the code host.
- A dragged card doesn't scroll a column that is taller than the panel.
- There is no sideways scrollbar; the half-shown next column is the hint. Edge
  scrolling while dragging moves only while the pointer moves.
- The ⌘ shortcuts (e.g. close pane) still act on the terminals while the panel is out.
