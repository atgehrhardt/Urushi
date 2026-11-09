# Feature: Task boards

- **Status:** Shipped (manual boards, subtasks, threads started from tasks)
- **Owners (crates):** `urushi-core` (`board`, `board/subtasks.rs`), `urushi-daemon` (`service/boards.rs`, `server/boards.rs`, the link in `service/threads.rs`, unlinking in `service/deletion.rs`), `urushi-proto` (`convert/boards.rs`), `urushi-client` (`boards.rs`, `WorkspaceMirror::{boards, board, task, subtask}`), `urushi-app` (`board.rs`, `update/board.rs`, `update/tasks.rs`, `view/board.rs`, `view/tasks.rs`), `urushi-cli` (`urushi board`, `urushi thread new --task`)
- **Related ADRs:** [0002](../adr/0002-daemon-source-of-truth.md), [0008](../adr/0008-json-document-store.md), [0012](../adr/0012-subtasks-as-checklists-and-thread-links.md)

## What it does
A **Tasks** button sits under the sidebar's Projects | Repos tabs. It shows the number of
open tasks, meaning tasks not in a board's last column. The button, or **Cmd+B**
(Ctrl+Shift+B off Apple platforms), slides a kanban panel out from under the sidebar, over
the terminals, which show faintly through it. Escape, ×, the button or the shortcut slide
it back.
- Boards are tabs across the top. **+** creates a board, and **Rename** and **Delete**
  act on the board shown. Deleting asks first and says how many tasks go with the board.
- A new board has the columns **To do**, **In progress** and **Done**.
- **+** on a column adds a task. Clicking a card opens the editor: title, notes, column,
  subtasks, **Start thread**, and Delete.
- **Notes** take several lines: the box is five lines tall, wraps, and scrolls. Enter
  starts a new line there; Cmd+Enter (Ctrl+Enter elsewhere) saves from any field, and
  Enter in the title still saves.
- **Cards are all the same size** whatever they hold: the title on one line, up to two
  lines of notes (blank lines skipped, an ellipsis when there's more), and a footer row
  for subtask progress and the thread chip. The copy that follows the pointer while
  dragging is the same card.
- **Subtasks** are a checklist inside a task. In the editor: type in "Add a subtask…"
  and press Enter; click the box to check one off, its title to rename it, × to delete
  it. A new task's subtasks are created with it. Cards show progress ("✓ 1/3", green when
  all are done).
- **Start thread** (on a task, or on any subtask row) opens the new-thread dialog with
  the title filled in, in the repo of the parent task's thread or else the thread on
  screen. The new thread is linked to the task or subtask: the button then reads **Go to
  thread**, and the card shows a "→ thread" chip that goes there. Deleting the thread
  clears the link.
- **Drag** a card onto another card to put it above that card, or onto a column to put it
  at the bottom. While dragging, the card's place stays as a faint outline, an accent line
  shows where it would land (only where it would move), the column under the pointer is
  outlined, and a raised copy of the card follows the pointer.
- On phones the panel covers the whole screen.

Boards live in the daemon, so every client sees the same boards, and they survive
restarts. Jira boards are planned as further tabs in the same panel.

## How it works
- **Model** (`urushi_core::board`): `Board{columns[Column{tasks[Task{subtasks[Subtask],
  thread}]}]}`. The rules live here: names and titles are validated, `move_task` takes an
  index counted without the moved task, clamped to the bottom, a task has at most
  `MAX_SUBTASKS`, and `link_thread` / `unlink_thread` record and forget threads. Ids and
  timestamps are passed in.
- **Daemon:** `Service::{create_board, rename_board, delete_board, create_task,
  update_task, move_task, delete_task, create_subtask, update_subtask, delete_subtask}`
  (`service/boards.rs`) change `State.boards` through `mutate`. That persists the change
  in the JSON document (`Document.boards`; older files have none, and older tasks no
  subtasks or link) and wakes `WorkspaceChanged` subscribers.
- **Linking threads:** `CreateThread{task_id, subtask_id}` checks the task exists before
  making the worktree, then links it in the same `mutate` that records the thread
  (`threads.rs::finish_thread_creation`). If the task was deleted meanwhile, the thread
  is kept unlinked. Deleting a thread unlinks it from every board (`deletion.rs`). See
  ADR 0012 for why the link lives on the task and is made by the daemon.
- **Wire (1.5, 1.6):** `Workspace.boards`, plus `CreateBoard` → `Board`, `RenameBoard`,
  `DeleteBoard`, `CreateTask{subtasks}` → `Task`, `UpdateTask{optional title, optional
  notes}`, `MoveTask{column_id, index}`, `DeleteTask`, and since 1.6 `CreateSubtask` →
  `Subtask`, `UpdateSubtask{optional title, optional done}`, `DeleteSubtask`,
  `Task.thread_id`, `Subtask.thread_id`.
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
    line in a slot with the same target (`view/board.rs::slot`), so the pointer is always
    over a target and the line doesn't jump as it moves the cards.
  - **Editor** (`update/tasks.rs`): an existing task's subtasks change right away (each
    is a request); a new task's are drafts in `SubtaskEditor` until `CreateTask`. Enter
    in "Add a subtask…" adds; Enter in the rename field saves; Enter in the notes (a
    `TextField::multiline`, drawn with the toolkit's `text_area`) adds a line; Enter
    elsewhere, or Cmd/Ctrl+Enter anywhere, saves the task (a subtask still typed in the
    field goes with it). Notes are sent only if they differ from the task's notes as the
    editor shows them, so opening and saving changes nothing.
  - **Cards** (`view/board.rs::card_body`): a fixed `card_height` from the theme's line
    heights; the notes are a `widgets::wrapped_text` filling the room between the title
    and the footer.
  - **Shortcut:** `B` in `update.rs::shortcut` sends `Msg::ToggleBoard`, the same message
    as the button.
  - **Input:** while the panel is out, `AppState::terminals_covered` keeps the mouse,
    wheel and typing away from the terminals.
  - Subtask controls and Start thread need a 1.6 daemon (`daemon_supports_subtasks`).
- **CLI:** `urushi board ls|new|rename|rm|add|edit|mv|rm-task`, `urushi board sub
  add|done|undo|rename|rm`, and `urushi thread new <repo> --task <task or subtask>`.
  Columns can be named, e.g. `urushi board mv <task> done`.

## Invariants
- A task is in exactly one column, and no operation loses or duplicates one
  (`crates/urushi-core/tests/board_props.rs`).
- A failed operation changes nothing (`board/tests.rs::failed_operations_change_nothing`).
- Subtasks move with their task, and a task holds at most `MAX_SUBTASKS`
  (`board/tests.rs::subtasks_are_added_checked_renamed_and_removed`,
  `a_task_holds_a_bounded_number_of_subtasks`).
- A link never creates, changes or deletes a thread. A missing task refuses
  `CreateThread` before any worktree exists, and deleting a thread clears its links
  (`tests/boards_and_usage.rs::threads_started_from_tasks_are_linked_until_deleted`).
- Deleting a board needs the confirmation dialog. Enter doesn't confirm
  (`update/tests/board.rs::boards_are_created_selected_and_only_deleted_after_confirming`).
- The panel never forwards input to the terminals under it
  (`update/tests/board.rs::the_panel_covers_the_terminals_until_closed`).

## Tests
- Core: `board/tests.rs` and `tests/board_props.rs`.
- Proto: `convert/boards.rs` covers round trips and wire encoding of every 1.5 and 1.6
  board message.
- Daemon: `tests/boards_and_usage.rs` (`boards_are_kept_mirrored_and_survive_a_restart`,
  `subtasks_are_created_checked_renamed_and_deleted`,
  `threads_started_from_tasks_are_linked_until_deleted`).
- App: `board.rs` (drop positions, open-task count), `update/tests/board.rs` (editor,
  multiline notes and Cmd+Enter, the board shortcut, drag and drop, prompts, deletion,
  keyboard) and `update/tests/tasks.rs` (subtasks,
  drafts, starting and going to threads, the drop line, older daemons). `snapshot.rs` has
  the `board`, `board-drag` and `task` scenarios, which render at desktop and phone sizes,
  and `task_cards_are_all_the_same_size`.
- Toolkit: `urushi-ui/tests/layout.rs::a_sliding_panel_moves_with_its_children_and_is_clipped`;
  `urushi-ui/tests/fields.rs` (text areas wrapping, clicks on lines, scrolling, and
  `wrapped_text`'s ellipsis); `textfield/tests.rs` (multiline editing and moves);
  `text.rs::wraps_at_line_breaks_then_between_words`.

## Limitations and follow-ups
- Columns are fixed to the three defaults. Adding, renaming and reordering columns needs
  new requests.
- Subtasks can't be reordered, and are one level deep (a subtask has no subtasks).
- In the notes, Up/Down move between lines separated by line breaks, not between the
  wrapped lines of a long paragraph (the editing model doesn't know the layout).
- A card's title is one line, cut short with an ellipsis.
- Starting a thread doesn't pass the task's notes to the agent; the thread opens on the
  launcher like any other.
- A dragged card doesn't scroll a column that is taller than the panel.
- Jira integration: boards from Jira as extra tabs, optional per user. It will need a
  board `source` field and a sync service in the daemon.
- The ⌘ shortcuts (e.g. close pane) still act on the terminals while the panel is out.
