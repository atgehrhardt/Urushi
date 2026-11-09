//! Acting on cards and columns from their context menus (open, move, color, delete; a
//! column's rename, order and removal), the tag filter, and the task editor's color and
//! tag drafts. Each change to the board is a request to the daemon; the board updates
//! when the workspace comes back.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{BoardMsg, Effect, Followup};
use crate::state::{AppState, Dialog};

impl AppState {
    /// Handles a [`BoardMsg`]. Every action closes the menu it came from.
    pub(super) fn card_action(&mut self, msg: BoardMsg, fx: &mut Vec<Effect>) {
        self.menu = None;
        self.board.pressed = None;
        match msg {
            BoardMsg::PickColor(_) | BoardMsg::AddTag(_) | BoardMsg::RemoveTag(_) => {
                self.edit_labels(msg);
            }
            BoardMsg::AddFiles(_)
            | BoardMsg::Attach { .. }
            | BoardMsg::AttachFailed(_)
            | BoardMsg::OpenAttachment(_)
            | BoardMsg::DeleteAttachment(_) => self.attachment_action(msg, fx),
            BoardMsg::ColorTask { task, color } => {
                let body = Body::UpdateTask(v1::UpdateTaskRequest {
                    task_id: task,
                    color: Some(color),
                    ..Default::default()
                });
                fx.push(request(body, Followup::None));
            }
            BoardMsg::ColorColumn { column, color } => {
                let body = Body::UpdateColumn(v1::UpdateColumnRequest {
                    column_id: column,
                    name: None,
                    color: Some(color),
                });
                fx.push(request(body, Followup::None));
            }
            BoardMsg::MoveColumn { column, index } => {
                let body = Body::MoveColumn(v1::MoveColumnRequest {
                    column_id: column,
                    index,
                });
                fx.push(request(body, Followup::None));
            }
            BoardMsg::OpenDeleteColumn(column) => self.open_delete_column(column),
            BoardMsg::ConfirmDeleteColumn => {
                if let Some(Dialog::DeleteColumn { column, .. }) = &self.dialog {
                    let body = Body::DeleteColumn(v1::DeleteColumnRequest {
                        column_id: column.clone(),
                    });
                    fx.push(request(body, Followup::CloseDialog));
                }
            }
            BoardMsg::FilterTag(tag) => self.board.tag_filter = tag,
            BoardMsg::OpenTask(task) => self.edit_task(&task, fx),
            BoardMsg::MakeStandalone(task) => {
                if let Some(found) = self.workspace.find_task(&task) {
                    let body = Body::MoveTask(v1::MoveTaskRequest {
                        task_id: task,
                        column_id: found.column.id.clone(),
                        index: u32::MAX,
                    });
                    fx.push(request(body, Followup::None));
                }
            }
            BoardMsg::OpenNest(task) => {
                let dialog = Dialog::NestTask {
                    task,
                    query: urushi_ui::TextField::default(),
                };
                self.open(dialog, Some(crate::state::Field::NestQuery));
            }
            BoardMsg::Nest { task, parent } => {
                let body = Body::NestTask(v1::NestTaskRequest {
                    task_id: task,
                    parent_id: parent,
                    index: u32::MAX,
                });
                let followup = if matches!(self.dialog, Some(Dialog::NestTask { .. })) {
                    Followup::CloseDialog
                } else {
                    Followup::None
                };
                fx.push(request(body, followup));
            }
            BoardMsg::DeleteTask(task_id) => {
                let body = self.delete_body(task_id);
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

impl AppState {
    /// Asks before removing a column, saying where its tasks go.
    fn open_delete_column(&mut self, column: String) {
        let Some((board, c)) = self.workspace.column(&column) else {
            return;
        };
        let Some(at) = board.columns.iter().position(|x| x.id == column) else {
            return;
        };
        // The same neighbour the daemon picks: the left one, or the right for the first.
        let heir = if at > 0 { at - 1 } else { at + 1 };
        let Some(into) = board.columns.get(heir).map(|h| h.name.clone()) else {
            self.error = Some("A board needs at least one column.".into());
            return;
        };
        let dialog = Dialog::DeleteColumn {
            column,
            name: c.name.clone(),
            tasks: c.tasks.len(),
            into,
        };
        self.open(dialog, None);
    }

    /// The editor's color and tag drafts.
    fn edit_labels(&mut self, msg: BoardMsg) {
        let Some(Dialog::Task { labels, .. }) = &mut self.dialog else {
            return;
        };
        match msg {
            BoardMsg::PickColor(color) => labels.color = color,
            BoardMsg::AddTag(tag) => labels.add(&tag),
            BoardMsg::RemoveTag(tag) => labels.tags.retain(|t| *t != tag),
            _ => {}
        }
    }
}
