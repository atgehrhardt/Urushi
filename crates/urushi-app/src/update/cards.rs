//! Acting on a card from its context menu: open, move, delete, without going through
//! the task editor. Each change is a request to the daemon; the board updates when the
//! workspace comes back.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{BoardMsg, Effect, Followup};
use crate::state::AppState;

impl AppState {
    /// Handles a [`BoardMsg`]. Every action closes the menu it came from.
    pub(super) fn card_action(&mut self, msg: BoardMsg, fx: &mut Vec<Effect>) {
        self.menu = None;
        self.board.pressed = None;
        match msg {
            BoardMsg::OpenTask(task) => self.edit_task(&task),
            BoardMsg::DeleteTask(task_id) => {
                let body = Body::DeleteTask(v1::DeleteTaskRequest { task_id });
                fx.push(request(body, Followup::None));
            }
            BoardMsg::MoveTask { task, column } => {
                let current = self.workspace.task(&task).map(|(_, c, _)| c.id.clone());
                if current.is_some_and(|c| c != column) {
                    let body = Body::MoveTask(v1::MoveTaskRequest {
                        task_id: task,
                        column_id: column,
                        index: u32::MAX,
                    });
                    fx.push(request(body, Followup::None));
                }
            }
        }
    }
}
