//! The task-board panel: opening it, choosing a board, dragging tasks between columns,
//! and deleting boards. The task editor, subtasks and starting threads from tasks are in
//! `tasks.rs`. Every change is a request to the daemon (`CreateTask`, `MoveTask`, …);
//! the panel updates when the workspace comes back.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::board::drop_position;
use crate::msg::{Effect, Followup, Msg};
use crate::organize::Prompt;
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
                // The terminals are covered: no half-made selection or drag survives.
                self.selection = None;
                self.drag = None;
            }
            Msg::BoardReveal(reveal) => self.board.reveal = reveal.clamp(0.0, 1.0),
            Msg::BoardBackground => self.board.pressed = None,
            Msg::SelectBoard(id) => {
                let shown = self.board.board(&self.workspace).map(|b| b.id.as_str());
                if shown != Some(id.as_str()) {
                    self.board.tag_filter = None;
                    self.board.epic_filter = None;
                }
                self.board.selected = Some(id);
            }
            Msg::NewTask { board, column } => self.new_task(board, column),
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
