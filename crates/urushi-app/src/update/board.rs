//! The task-board panel: opening it, choosing a board, dragging tasks between columns,
//! dragging board tabs to reorder them, and deleting boards. The task editor, subtasks
//! and starting threads from tasks are in `tasks.rs`. Every change is a request to the
//! daemon (`CreateTask`, `MoveTask`, `MoveBoard`, …); the panel updates when the
//! workspace comes back.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::board::drop_position;
use crate::msg::{Effect, Followup, Msg};
use crate::organize::{Dragged, Prompt, board_drop_index};
use crate::state::{AppState, Dialog};

impl AppState {
    /// Handles the board messages (see the arm in `update`).
    pub(super) fn board_msg(&mut self, msg: Msg, fx: &mut Vec<Effect>) {
        match msg {
            Msg::ToggleBoard => {
                self.menu = None;
                self.board.open = !self.board.open;
                self.board.pressed = None;
                self.board.dragging = None;
                self.board.over = None;
                if matches!(self.dragging, Some((Dragged::Board(_), _))) {
                    self.dragging = None;
                }
                self.board.tab_over = None;
                // The terminals are covered: no half-made selection or drag survives.
                self.selection = None;
                self.drag = None;
            }
            Msg::BoardReveal(reveal) => self.board.reveal = reveal.clamp(0.0, 1.0),
            Msg::BoardBackground => {
                self.board.pressed = None;
                if self.searching() {
                    self.focus = None;
                }
            }
            Msg::SelectBoard(id) => {
                // Phones choose boards from a menu.
                // What belonged to the board shown before goes with it
                // (`update/shown_board.rs`).
                self.menu = None;
                self.board.selected = Some(id);
            }
            Msg::ShowColumn(column) => self.board.column = Some(column),
            Msg::Swipe(dx) => self.swipe_column(dx),
            Msg::NewTask { board, column } => self.new_task(board, column, fx),
            Msg::PressTask(task) => self.board.pressed = Some(task),
            Msg::PressEnded => {
                if let Some(task) = self.board.pressed.take() {
                    self.edit_task(&task, fx);
                }
            }
            Msg::DragTask(task) => {
                self.board.pressed = None;
                if self.dialog.is_none() {
                    self.board.dragging = Some((task, self.last_press.unwrap_or_default()));
                }
            }
            Msg::DropTask { column, before } => self.drop_task(&column, before.as_deref(), fx),
            Msg::DragOver(target) => self.drag_over(target.map(|m| *m)),
            Msg::DragBoard(board) => {
                self.board.pressed = None;
                if self.daemon_supports_board_order() && self.dialog.is_none() {
                    self.menu = None;
                    self.board.tab_over = None;
                    self.dragging =
                        Some((Dragged::Board(board), self.last_press.unwrap_or_default()));
                }
            }
            Msg::DropBoard(target) => {
                self.board.tab_over = None;
                if let Some((Dragged::Board(board), _)) = self.dragging.take()
                    && let Some(index) = board_drop_index(self.workspace.boards(), &board, &target)
                {
                    fx.push(move_board(board, index));
                }
            }
            msg @ (Msg::PickColumn(_)
            | Msg::DeleteTask
            | Msg::ToggleSubtask(_)
            | Msg::DeleteSubtask(_)
            | Msg::RemoveDraftSubtask(_)
            | Msg::StartThread(_)
            | Msg::UseExistingThread(_)
            | Msg::PickExistingThread(_)) => self.task_msg(msg, fx),
            Msg::Board(action) => self.card_action(action, fx),
            Msg::OpenDeleteBoard(board) => {
                self.menu = None;
                if let Some(b) = self.workspace.board(&board) {
                    let tasks = b.columns.iter().map(|c| c.tasks.len()).sum();
                    let name = b.name.clone();
                    self.open(Dialog::DeleteBoard { board, name, tasks }, None);
                }
            }
            Msg::ConfirmDeleteBoard => {
                if let Some(Dialog::DeleteBoard { board, .. }) = &self.dialog {
                    let body = Body::DeleteBoard(v1::DeleteBoardRequest {
                        board_id: board.clone(),
                    });
                    fx.push(request(body, Followup::CloseDialog));
                }
            }
            _ => {}
        }
    }

    /// A sideways swipe on a phone's board: the next column for a swipe to the left,
    /// the previous for one to the right. Only while the board is what the finger is on:
    /// the panel is out, nothing covers it and no card is being dragged.
    fn swipe_column(&mut self, dx: f32) {
        if !self.compact
            || !self.board.open
            || self.dialog.is_some()
            || self.menu.is_some()
            || self.board.dragging.is_some()
            || dx == 0.0
        {
            return;
        }
        let delta = if dx < 0.0 { 1 } else { -1 };
        if let Some(board) = self.board.board(&self.workspace)
            && let Some(column) = self.board.step_column(board, delta)
        {
            self.board.column = Some(column);
        }
    }

    /// Where the dragged task would land, for the drop indicator. Only drops that move
    /// it count.
    fn drag_over(&mut self, target: Option<Msg>) {
        let Some((task, _)) = &self.board.dragging else {
            return;
        };
        self.board.over = match target {
            Some(Msg::DropTask { column, before }) => self
                .workspace
                .task(task)
                .and_then(|(board, _, _)| drop_position(board, task, &column, before.as_deref()))
                .map(|_| (column, before)),
            _ => None,
        };
    }

    /// While a board's tab is dragged: remember which tab's place it would take, for the
    /// drop line. Only drops that move it count.
    pub(super) fn board_tab_over(&mut self, target: Option<Msg>) {
        let Some((Dragged::Board(board), _)) = &self.dragging else {
            return;
        };
        self.board.tab_over = match target {
            Some(Msg::DropBoard(t))
                if board_drop_index(self.workspace.boards(), board, &t).is_some() =>
            {
                Some(t)
            }
            _ => None,
        };
    }

    fn drop_task(&mut self, column: &str, before: Option<&str>, fx: &mut Vec<Effect>) {
        self.board.over = None;
        let Some((task, _)) = self.board.dragging.take() else {
            return;
        };
        let Some((board, _, _)) = self.workspace.task(&task) else {
            return;
        };
        if let Some((column_id, index)) = drop_position(board, &task, column, before) {
            let body = Body::MoveTask(v1::MoveTaskRequest {
                task_id: task,
                column_id,
                index,
            });
            fx.push(request(body, Followup::None));
        }
    }

    /// The request a board prompt stands for (`None`: not a board prompt).
    pub(super) fn board_prompt(prompt: &Prompt, text: String) -> Option<(Body, Followup)> {
        match prompt {
            Prompt::NewBoard => Some((
                Body::CreateBoard(v1::CreateBoardRequest { name: text }),
                Followup::BoardCreated,
            )),
            Prompt::RenameBoard(board_id) => Some((
                Body::RenameBoard(v1::RenameBoardRequest {
                    board_id: board_id.clone(),
                    name: text,
                }),
                Followup::CloseDialog,
            )),
            Prompt::NewColumn(board_id) => Some((
                Body::CreateColumn(v1::CreateColumnRequest {
                    board_id: board_id.clone(),
                    name: text,
                    index: u32::MAX,
                }),
                Followup::CloseDialog,
            )),
            Prompt::RenameColumn(column_id) => Some((
                Body::UpdateColumn(v1::UpdateColumnRequest {
                    column_id: column_id.clone(),
                    name: Some(text),
                    color: None,
                }),
                Followup::CloseDialog,
            )),
            _ => None,
        }
    }

    /// Escape with the panel open and nothing above it: close the panel.
    pub(super) fn board_escape(&mut self) -> bool {
        // The search clears first, then lets go of the keyboard.
        if self.searching() {
            if self.board.filter.search.text().is_empty() {
                self.focus = None;
            } else {
                self.board.filter.search.clear();
            }
            return true;
        }
        if self.board.dragging.take().is_some() {
            self.board.over = None;
            return true;
        }
        if self.board.open && self.dialog.is_none() {
            self.board.open = false;
            return true;
        }
        false
    }
}

/// The `MoveBoard` request (the order of the tabs is the daemon's).
pub(super) fn move_board(board_id: String, index: u32) -> Effect {
    request(
        Body::MoveBoard(v1::MoveBoardRequest { board_id, index }),
        Followup::None,
    )
}
