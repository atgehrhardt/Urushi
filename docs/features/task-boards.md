# Feature: Task boards

- **Status:** Shipped (manual boards, subtasks, threads started from tasks)
- **Owners (crates):** `urushi-core` (`board`, `board/{tree,labels,columns,prompt}.rs`), `urushi-daemon` (`service/{boards,columns}.rs`, `server/boards.rs`, the link in `service/threads.rs`, unlinking in `service/deletion.rs`), `urushi-proto` (`convert/boards/`), `urushi-client` (`boards.rs`, `tasks.rs`, `WorkspaceMirror::{boards, board, column, task, find_task}`), `urushi-app` (`board.rs`, `update/board.rs`, `update/cards.rs`, `update/tasks.rs`, `update/task_threads.rs`, `view/board/`, `view/tasks/`, `view/labels.rs`, `view/menu/tasks.rs`), `urushi-ui` (`Theme::labels`), `urushi-cli` (`urushi board`, `urushi thread new --task`)
- **Related ADRs:** [0002](../adr/0002-daemon-source-of-truth.md), [0008](../adr/0008-json-document-store.md), [0012](../adr/0012-subtasks-as-checklists-and-thread-links.md), [0015](../adr/0015-subtasks-are-nested-tasks.md), [0014](../adr/0014-task-prompt-via-pane-environment.md)

## What it does
A **Tasks** button sits under the sidebar's Projects | Repos tabs. It shows the number of
open tasks, meaning tasks not in a board's last column. The button, or **Cmd+B**
(Ctrl+Shift+B off Apple platforms), slides a kanban panel out from under the sidebar, over
the terminals, which show faintly through it. Escape, ×, the button or the shortcut slide
it back.
- Boards are tabs across the top. **+** creates a board, and **Rename** and **Delete**
  act on the board shown. Deleting asks first and says how many tasks go with the board.
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
  tags are offered under "On this board". The **All tags ▾** button in the header
  filters the board to one tag; the column counts then read "shown/all". Choosing
  another board clears the filter.
- **+** on a column adds a task. Clicking a card opens the editor: title, notes and
  subtasks on the left; column, color and tags on the right (one scrolling column on
  phones); **Start thread**, and Delete. Color and tags are saved with Save, like the
  title and notes.
- **Notes** take several lines: the box is five lines tall, wraps, and scrolls. Enter
  starts a new line there; Cmd+Enter (Ctrl+Enter elsewhere) saves from any field, and
  Enter in the title still saves.
- **Cards are all the same size** whatever they hold: the title on one line, up to two
  lines of notes (blank lines skipped, an ellipsis when there's more), and a footer row
  for subtask progress and the thread chip. The copy that follows the pointer while
  dragging is the same card.
- **Subtasks** are tasks too, kept inside their parent (ADR 0015). In the editor: type
  in "Add a subtask…" and press Enter; click the box to check one off, × to delete it. A
  new task's subtasks are created with it. Cards show progress ("✓ 1/3", green when all
  are done), and subtask rows their own.
  - **Clicking a subtask's title opens it** as a task of its own ("Edit subtask"), with
    notes, color, tags, its own subtasks and thread, saving what changed in the task on
    screen first. A breadcrumb ("Card › Subtask ›") leads back up. Instead of a column, a
    subtask has **Done / Not done** and **Make standalone**, which moves it onto the
    board in its card's column, unchecked, with its subtasks. Subtasks nest up to five
    levels under a card.
  - **Make subtask of…** (in a task's menu, card or subtask) lists the board's other
    tasks at any depth ("Card › Subtask"), minus the task's own subtree and its current
    parent; type to narrow it, click one or press Enter for the first. A card from the
    last column arrives checked off.
  - Right-clicking a subtask row opens the same menu as a card, over the editor.
- **Start thread** (on a task, or on any subtask row) opens the new-thread dialog with
  the title filled in, in the repo of the parent task's thread or else the thread on
  screen. The new thread is linked to the task or subtask: the button then reads **Go to
  thread**, and the card shows a "→ thread" chip that goes there. Deleting the thread
  clears the link. With a task agent chosen (setup wizard or Settings), the thread starts
  it with the task, its notes and its subtasks as the first prompt; otherwise it opens on
  the launcher. See [task agent](task-agent.md).
- **Right-click or two-finger click** a card for its menu: **Open**, **Move to ›** (a
  column), **Make subtask of…**, **Color ›**, **Start thread…** / **Go to thread**, and
  **Delete task**, which deletes it
  right away, as the editor's Delete does.
- Long names are cut short with an ellipsis instead of running over their neighbours:
  subtask titles in the editor, column names, board tabs and the card's thread chip.
- **Drag** a card onto another card to put it above that card, or onto a column to put it
  at the bottom. While dragging, the card's place stays as a faint outline, an accent line
  shows where it would land (only where it would move), the column under the pointer is
  outlined, and a raised copy of the card follows the pointer.
- On phones the panel covers the whole screen.

Boards live in the daemon, so every client sees the same boards, and they survive
restarts. Jira boards are planned as further tabs in the same panel.

## How it works
- **Model** (`urushi_core::board`): `Board{columns[Column{color, tasks[Task{done, color,
  tags, subtasks[Task], thread}]}]}`, a forest (`tree.rs`): `task`, `path_to`,
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
  `MAX_SUBTASKS`, and `link_thread` / `unlink_thread` record and forget threads. Ids and
  timestamps are passed in. Older documents' checklist items load as tasks (every new
  field has a serde default; `created_at` reads as 0).
- **Daemon:** `Service::{create_board, rename_board, delete_board, create_task,
  update_task, move_task, nest_task, delete_task, create_subtask}` (`service/boards.rs`;
  `UpdateSubtask` and `DeleteSubtask` are served by `update_task` and `delete_task`) and `Service::{create_column, update_column, move_column,
  delete_column}` (`service/columns.rs`) change `State.boards` through `mutate`. That persists the change
  in the JSON document (`Document.boards`; older files have none, and older tasks no
  subtasks or link) and wakes `WorkspaceChanged` subscribers.
- **Linking threads:** `CreateThread{task_id, subtask_id}` checks the task exists (and
  that `subtask_id`, if set, is somewhere under it) before making the worktree, then links it in the same `mutate` that records the thread
  (`threads.rs::finish_thread_creation`). If the task was deleted meanwhile, the thread
  is kept unlinked. Deleting a thread unlinks it from every board (`deletion.rs`). See
  ADR 0012 for why the link lives on the task and is made by the daemon.
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
  out onto the board), `DeleteTask` and `CreateSubtask`.
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
  - **Editor** (`update/tasks.rs`): `Dialog::Task` edits a card or a subtask alike; the
    app reads both through `urushi_client::{TaskRef, FoundTask}`
    (`WorkspaceMirror::find_task`, which also gives the path for the breadcrumb).
    `BoardMsg::OpenTask` from the editor first sends what changed (`task_changes`, the
    same requests Save sends, without closing). A subtask's check mark changes right
    away; a subtask is deleted with `DeleteSubtask`. An existing task's subtasks change
    right away (each is a request); a new task's are drafts in `SubtaskEditor` until
    `CreateTask`. Enter in "Add a subtask…" or "Add a tag…" adds; Enter in the notes (a
    `TextField::multiline`, drawn with the toolkit's `text_area`) adds a line; Enter
    elsewhere, or Cmd/Ctrl+Enter anywhere, saves the task (a subtask still typed in the
    field goes with it). Notes are sent only if they differ from the task's notes as the
    editor shows them, so opening and saving changes nothing.
  - **Cards** (`view/board/card.rs::card_body`): a fixed `card_height` from the theme's line
    heights; the notes are a `widgets::wrapped_text` filling the room between the title
    and the footer.
  - **Nesting:** `Dialog::NestTask` lists `board::nest_candidates`, and choosing one
    sends `NestTask`; **Make standalone** sends `MoveTask` to the card's column.
    `TaskLink::of` names the card and the subtask for `CreateThread`.
  - **Menus:** the card's (or a subtask row's) `on_secondary_click` opens
    `MenuTarget::Task` (the only menu that opens over a dialog, the task editor), a column's
    header or ⋯ `MenuTarget::Column`, and the header's filter button
    `MenuTarget::TagFilter` (all in `view/menu/tasks.rs`; colors are a `MenuPage::Color`
    page). Their actions are `Msg::Board(BoardMsg::…)`, handled in `update/cards.rs`.
    Moving a card there sends `MoveTask` to the bottom of the column. Column names go
    through `Prompt::{NewColumn, RenameColumn}`, and removal through
    `Dialog::DeleteColumn`, which names the same neighbour the daemon picks.
  - **Labels:** the editor's color and tags are drafts in `TaskLabels`
    (`board.rs`), sent with Save only when they changed; a tag still typed in the field
    goes too. `view/labels.rs` maps wire colors to `Theme::labels` and draws swatches
    and tag chips, wrapping chips into rows by their approximate width (the layout
    doesn't wrap). The tag filter (`BoardPanel.tag_filter`, `board::shows`) only
    changes what is drawn; drops still use task ids, so they land right.
  - **Shortcut:** `B` in `update.rs::shortcut` sends `Msg::ToggleBoard`, the same message
    as the button.
  - **Input:** while the panel is out, `AppState::terminals_covered` keeps the mouse,
    wheel and typing away from the terminals.
  - Subtask controls and Start thread need a 1.6 daemon (`daemon_supports_subtasks`);
    colors, tags and column changes a 1.8 one (`daemon_supports_board_extras`).
- **CLI:** `urushi board ls|new|rename|rm|add|edit|mv|nest|rm-task` (`ls` prints
  subtasks as an indented tree; `mv` takes a subtask out onto the board; `edit`,
  `rm-task` and `sub add` take subtask ids) (`add --color --tag`,
  `edit --color <c|none> --tag --untag`), `urushi board col add|rename|color|mv|rm`,
  `urushi board sub add|done|undo|rename|rm`, and `urushi thread new <repo> --task <task or subtask>`.
  Columns can be named, e.g. `urushi board mv <task> done`.

## Invariants
- A task is in exactly one column, and no operation loses or duplicates one, adding,
  moving and removing columns included; a board always keeps a column
  (`crates/urushi-core/tests/board_props.rs`).
- A failed operation changes nothing (`board/tests.rs::failed_operations_change_nothing`).
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
- The panel never forwards input to the terminals under it
  (`update/tests/board.rs::the_panel_covers_the_terminals_until_closed`).

## Tests
- Core: `board/tests.rs`, `board/tree/tests.rs` (nesting, bounds, links, old
  documents), `board/prompt.rs` and `tests/board_props.rs`.
- Proto: `convert/boards/tests.rs` covers round trips and wire encoding of every 1.5,
  1.6 and 1.8 board message; `convert/boards/labels.rs` the color mapping.
- Daemon: `tests/board_columns.rs`
  (`columns_colors_and_tags_are_kept_and_survive_a_restart`), `tests/board_nesting.rs`
  (`cards_and_subtasks_move_into_and_out_of_each_other`), `tests/task_agents.rs` (a
  deep subtask's link and prompt),
  `tests/boards_and_usage.rs` (`boards_are_kept_mirrored_and_survive_a_restart`,
  `subtasks_are_created_checked_renamed_and_deleted`,
  `threads_started_from_tasks_are_linked_until_deleted`).
- App: `board.rs` (drop positions, open-task count), `update/tests/board.rs` (the card
  menu, editor,
  multiline notes and Cmd+Enter, the board shortcut, drag and drop, prompts, deletion,
  keyboard) and `update/tests/tasks.rs` (subtasks,
  drafts, opening subtasks, making them standalone, nesting, starting and going to
  threads, the drop line, older daemons) and
  `update/tests/labels.rs` (colors, tags, columns, the filter). `snapshot.rs` has
  the `board`, `board-drag`, `task`, `task-menu`, `column-menu`, `task-nested` and
  `nest` scenarios, which render at desktop and
  phone sizes, `task_cards_are_all_the_same_size` and
  `long_subtask_titles_are_cut_short_inside_the_editor`.
- Toolkit: `urushi-ui/tests/layout.rs::a_sliding_panel_moves_with_its_children_and_is_clipped`;
  `urushi-ui/tests/fields.rs` (text areas wrapping, clicks on lines, scrolling, and
  `wrapped_text`'s ellipsis); `textfield/tests.rs` (multiline editing and moves);
  `text.rs::wraps_at_line_breaks_then_between_words`.

## Limitations and follow-ups
- Tags are per task; there is no place to rename or recolor a tag across a board.
- With many columns, columns get narrow: the board doesn't scroll sideways yet.
- Subtasks can't be reordered by dragging (`NestTask` takes an index, but the editor
  doesn't offer it), and cards can't be dragged onto a task to nest them.
- On a 1.6 or 1.7 daemon subtasks are a checklist: their titles don't open, and they
  can't be renamed in the app (rename them with `urushi board sub rename`).
- In the notes, Up/Down move between lines separated by line breaks, not between the
  wrapped lines of a long paragraph (the editing model doesn't know the layout).
- A card's title is one line, cut short with an ellipsis.
- A dragged card doesn't scroll a column that is taller than the panel.
- Jira integration: boards from Jira as extra tabs, optional per user. It will need a
  board `source` field and a sync service in the daemon.
- The ⌘ shortcuts (e.g. close pane) still act on the terminals while the panel is out.
