//! The task editor: creating and editing a task or a subtask at any depth (title,
//! notes, color, tags, its column or check mark), its subtasks, and moving between
//! a task and its subtasks.
//!
//! An existing task's subtasks change right away (each check, add or delete is a
//! request). A new task's subtasks are drafts until the task is created with them
//! (`CreateTask.subtasks`). Opening a subtask from the editor first saves what changed
//! in the task on screen, so moving down and back up loses nothing. Threads are in
//! `task_threads.rs`.

use urushi_client::FoundTask;
use urushi_client::v1::{self, request::Body};
use urushi_ui::TextField;

use super::request;
use crate::board::TaskLabels;
use crate::msg::{Effect, Followup, Msg};
use crate::state::{AppState, Dialog, Field};

impl AppState {
    /// Handles the task-editor messages (see the arm in `board_msg`).
    pub(super) fn task_msg(&mut self, msg: Msg, fx: &mut Vec<Effect>) {
        match msg {
            Msg::PickColumn(id) => {
                if let Some(Dialog::Task { column, .. }) = &mut self.dialog {
                    *column = id;
                }
            }
            Msg::DeleteTask => {
                if let Some(Dialog::Task {
                    task: Some(task), ..
                }) = &self.dialog
                {
                    let body = self.delete_body(task.clone());
                    fx.push(request(body, Followup::CloseDialog));
                }
            }
            Msg::ToggleSubtask(id) => {
                if let Some(found) = self.workspace.find_task(&id) {
                    let body = Body::UpdateSubtask(v1::UpdateSubtaskRequest {
                        subtask_id: id,
                        title: None,
                        done: Some(!found.task().done()),
                    });
                    fx.push(request(body, Followup::None));
                }
            }
            Msg::DeleteSubtask(id) => {
                let body = Body::DeleteSubtask(v1::DeleteSubtaskRequest { subtask_id: id });
                fx.push(request(body, Followup::None));
            }
            Msg::RemoveDraftSubtask(i) => {
                if let Some(Dialog::Task { subtasks, .. }) = &mut self.dialog
                    && i < subtasks.drafts.len()
                {
                    subtasks.drafts.remove(i);
                }
            }
            Msg::StartThread(link) => self.start_thread(link, fx),
            Msg::UseExistingThread(existing) => self.use_existing_thread(existing),
            Msg::PickExistingThread(thread) => self.pick_existing_thread(thread),
            _ => {}
        }
    }

    /// Deleting a task: a subtask goes through `DeleteSubtask`, which every daemon with
    /// subtasks knows.
    pub(super) fn delete_body(&self, task_id: String) -> Body {
        match self.workspace.find_task(&task_id) {
            Some(found) if !found.task().is_card() => {
                Body::DeleteSubtask(v1::DeleteSubtaskRequest {
                    subtask_id: task_id,
                })
            }
            _ => Body::DeleteTask(v1::DeleteTaskRequest { task_id }),
        }
    }

    /// Opens the editor for a new task in `column`.
    pub(super) fn new_task(&mut self, board: String, column: String) {
        self.open(
            Dialog::Task {
                board,
                column,
                task: None,
                title: TextField::default(),
                notes: TextField::multiline(""),
                subtasks: Box::default(),
                labels: Box::default(),
            },
            Some(Field::TaskTitle),
        );
    }

    /// Opens the editor on an existing task or subtask. From the editor of another
    /// task, what changed there is saved first.
    pub(super) fn edit_task(&mut self, id: &str, fx: &mut Vec<Effect>) {
        let Some(found) = self.workspace.find_task(id) else {
            return;
        };
        let dialog = editor_for(&found);
        if matches!(self.dialog, Some(Dialog::Task { task: Some(_), .. })) {
            fx.extend(self.task_changes(Followup::None));
        }
        self.open(dialog, Some(Field::TaskTitle));
    }

    /// Enter in the task editor's tag or subtask field: add what's typed. Returns
    /// whether it did (otherwise Enter saves the task).
    pub(super) fn task_editor_enter(&mut self, fx: &mut Vec<Effect>) -> bool {
        let focus = self.focus;
        let Some(Dialog::Task {
            task,
            subtasks,
            labels,
            ..
        }) = &mut self.dialog
        else {
            return false;
        };
        match focus {
            Some(Field::TaskTag) if !labels.input.text().trim().is_empty() => {
                let typed = labels.input.text().to_owned();
                labels.add(&typed);
                labels.input.clear();
                true
            }
            Some(Field::NewSubtask) if !subtasks.new.text().trim().is_empty() => {
                let title = subtasks.new.text().trim().to_owned();
                subtasks.new.clear();
                match task {
                    Some(task_id) => {
                        let body = Body::CreateSubtask(v1::CreateSubtaskRequest {
                            task_id: task_id.clone(),
                            title,
                        });
                        fx.push(request(body, Followup::None));
                    }
                    None => subtasks.drafts.push(title),
                }
                true
            }
            _ => false,
        }
    }

    /// Enter or "Save" in the task editor. A blank title does nothing.
    pub(super) fn submit_task(&mut self, fx: &mut Vec<Effect>) {
        let Some(Dialog::Task {
            board,
            column,
            task,
            title,
            notes,
            subtasks,
            labels,
        }) = &self.dialog
        else {
            return;
        };
        if title.text().trim().is_empty() {
            return;
        }
        if task.is_none() {
            // A subtask typed but not yet added goes with the task too.
            let typed = Some(subtasks.new.text().trim().to_owned()).filter(|t| !t.is_empty());
            let body = Body::CreateTask(v1::CreateTaskRequest {
                board_id: board.clone(),
                column_id: column.clone(),
                title: title.text().trim().to_owned(),
                notes: notes.text().to_owned(),
                subtasks: subtasks.drafts.iter().cloned().chain(typed).collect(),
                color: labels.color,
                tags: labels.with_typed(),
                kind: labels.kind,
            });
            fx.push(request(body, Followup::CloseDialog));
            return;
        }
        let changes = self.task_changes(Followup::CloseDialog);
        let closes = changes.iter().any(|e| {
            matches!(
                e,
                Effect::Request {
                    followup: Followup::CloseDialog,
                    ..
                }
            )
        });
        fx.extend(changes);
        if !closes {
            self.dialog = None;
            self.focus = None;
        }
    }

    /// The requests that save the existing task on screen: a subtask still typed in its
    /// field, a new column, and its changed fields (the last one carries `last`, so the
    /// editor closes when it is done). Nothing if it was deleted meanwhile.
    fn task_changes(&self, last: Followup) -> Vec<Effect> {
        let Some(Dialog::Task {
            column,
            task: Some(task),
            title,
            notes,
            subtasks,
            labels,
            ..
        }) = &self.dialog
        else {
            return Vec::new();
        };
        let Some(found) = self.workspace.find_task(task) else {
            return Vec::new();
        };
        let current = found.task();
        let mut out = Vec::new();
        let typed = subtasks.new.text().trim().to_owned();
        if !typed.is_empty() {
            let body = Body::CreateSubtask(v1::CreateSubtaskRequest {
                task_id: task.clone(),
                title: typed,
            });
            out.push(request(body, Followup::None));
        }
        if current.is_card() && found.column.id != *column {
            let body = Body::MoveTask(v1::MoveTaskRequest {
                task_id: task.clone(),
                column_id: column.clone(),
                index: u32::MAX,
            });
            out.push(request(body, Followup::None));
        }
        let title = title.text().trim().to_owned();
        let notes = notes.text().to_owned();
        let tags = labels.with_typed();
        let update = v1::UpdateTaskRequest {
            task_id: task.clone(),
            title: (!title.is_empty() && title != current.title()).then_some(title),
            // Compared as the editor shows them (tabs as spaces), so opening and saving
            // a task changes nothing.
            notes: (notes.trim_end() != TextField::multiline(current.notes().to_owned()).text())
                .then_some(notes),
            color: (labels.color != current.color()).then_some(labels.color),
            tags: (tags != current.tags()).then_some(v1::TagList { tags }),
            // Jira owns an issue's type.
            kind: (labels.kind != current.kind() && current.issue().is_none())
                .then_some(labels.kind),
        };
        if update.title.is_some()
            || update.notes.is_some()
            || update.color.is_some()
            || update.tags.is_some()
            || update.kind.is_some()
        {
            out.push(request(Body::UpdateTask(update), last));
        }
        out
    }
}

/// The editor on an existing task or subtask, filled in from the workspace.
fn editor_for(found: &FoundTask<'_>) -> Dialog {
    let task = found.task();
    Dialog::Task {
        board: found.board.id.clone(),
        column: found.column.id.clone(),
        task: Some(task.id().to_owned()),
        title: TextField::new(task.title().to_owned()),
        notes: TextField::multiline(task.notes().to_owned()),
        subtasks: Box::default(),
        labels: Box::new(TaskLabels {
            color: task.color(),
            kind: task.kind(),
            tags: task.tags().to_vec(),
            input: TextField::default(),
        }),
    }
}
