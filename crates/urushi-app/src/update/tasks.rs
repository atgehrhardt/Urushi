//! The task editor: creating and editing a task (title, notes, column), its subtasks,
//! and starting a thread for a task or a subtask.
//!
//! An existing task's subtasks change right away (each check, rename, add or delete is a
//! request), like a checklist. A new task's subtasks are drafts in the editor until the
//! task is created with them (`CreateTask.subtasks`). Starting a thread opens the
//! new-thread dialog with the task's title; the daemon links the thread to the task when
//! it creates it, and from then on the same button goes to that thread.

use urushi_client::v1::{self, request::Body};
use urushi_ui::TextField;

use super::request;
use crate::msg::{Effect, Followup, Msg};
use crate::state::{AppState, Dialog, Field, TaskLink};

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
                    let body = Body::DeleteTask(v1::DeleteTaskRequest {
                        task_id: task.clone(),
                    });
                    fx.push(request(body, Followup::CloseDialog));
                }
            }
            Msg::ToggleSubtask(id) => {
                if let Some((_, subtask)) = self.workspace.subtask(&id) {
                    let body = Body::UpdateSubtask(v1::UpdateSubtaskRequest {
                        subtask_id: id,
                        title: None,
                        done: Some(!subtask.done),
                    });
                    fx.push(request(body, Followup::None));
                }
            }
            Msg::RenameSubtask(id) => {
                let title = self.workspace.subtask(&id).map(|(_, s)| s.title.clone());
                if let (Some(title), Some(Dialog::Task { subtasks, .. })) =
                    (title, &mut self.dialog)
                {
                    subtasks.renaming = Some((id, TextField::new(title)));
                    self.focus = Some(Field::SubtaskTitle);
                }
            }
            Msg::DeleteSubtask(id) => {
                if let Some(Dialog::Task { subtasks, .. }) = &mut self.dialog
                    && subtasks.renaming.as_ref().is_some_and(|(r, _)| *r == id)
                {
                    subtasks.renaming = None;
                    self.focus = Some(Field::NewSubtask);
                }
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
            _ => {}
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
            },
            Some(Field::TaskTitle),
        );
    }

    /// Opens the editor on an existing task.
    pub(super) fn edit_task(&mut self, id: &str) {
        let Some((board, column, task)) = self.workspace.task(id) else {
            return;
        };
        let dialog = Dialog::Task {
            board: board.id.clone(),
            column: column.id.clone(),
            task: Some(task.id.clone()),
            title: TextField::new(task.title.clone()),
            notes: TextField::multiline(task.notes.clone()),
            subtasks: Box::default(),
        };
        self.open(dialog, Some(Field::TaskTitle));
    }

    /// Enter in the task editor's subtask fields: add the typed subtask, or finish a
    /// rename. Returns whether it did (otherwise Enter saves the task).
    pub(super) fn task_editor_enter(&mut self, fx: &mut Vec<Effect>) -> bool {
        let focus = self.focus;
        let Some(Dialog::Task { task, subtasks, .. }) = &mut self.dialog else {
            return false;
        };
        match focus {
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
            Some(Field::SubtaskTitle) => {
                if let Some((id, field)) = subtasks.renaming.take() {
                    let title = field.text().trim().to_owned();
                    let changed = self
                        .workspace
                        .subtask(&id)
                        .is_some_and(|(_, s)| s.title != title);
                    if changed && !title.is_empty() {
                        let body = Body::UpdateSubtask(v1::UpdateSubtaskRequest {
                            subtask_id: id,
                            title: Some(title),
                            done: None,
                        });
                        fx.push(request(body, Followup::None));
                    }
                }
                self.focus = Some(Field::NewSubtask);
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
        }) = &self.dialog
        else {
            return;
        };
        let (title, notes) = (title.text().trim().to_owned(), notes.text().to_owned());
        if title.is_empty() {
            return;
        }
        // A subtask typed but not yet added goes with the task too.
        let typed = Some(subtasks.new.text().trim().to_owned()).filter(|t| !t.is_empty());
        let Some(task) = task else {
            let body = Body::CreateTask(v1::CreateTaskRequest {
                board_id: board.clone(),
                column_id: column.clone(),
                title,
                notes,
                subtasks: subtasks.drafts.iter().cloned().chain(typed).collect(),
            });
            fx.push(request(body, Followup::CloseDialog));
            return;
        };
        let Some((_, current_column, current)) = self.workspace.task(task) else {
            // Deleted meanwhile (e.g. from another client).
            self.dialog = None;
            self.focus = None;
            return;
        };
        if let Some(title) = typed {
            let body = Body::CreateSubtask(v1::CreateSubtaskRequest {
                task_id: task.clone(),
                title,
            });
            fx.push(request(body, Followup::None));
        }
        if current_column.id != *column {
            let body = Body::MoveTask(v1::MoveTaskRequest {
                task_id: task.clone(),
                column_id: column.clone(),
                index: u32::MAX,
            });
            fx.push(request(body, Followup::None));
        }
        let update = v1::UpdateTaskRequest {
            task_id: task.clone(),
            title: (title != current.title).then_some(title),
            // Compared as the editor shows them (tabs as spaces), so opening and saving
            // a task changes nothing.
            notes: (notes.trim_end() != TextField::multiline(current.notes.clone()).text())
                .then_some(notes),
        };
        if update.title.is_some() || update.notes.is_some() {
            fx.push(request(Body::UpdateTask(update), Followup::CloseDialog));
        } else {
            self.dialog = None;
            self.focus = None;
        }
    }

    /// The thread started for a task or subtask, if it still exists.
    pub(crate) fn linked_thread(&self, link: &TaskLink) -> Option<&v1::Thread> {
        let id = match &link.subtask {
            Some(s) => &self.workspace.subtask(s)?.1.thread_id,
            None => &self.workspace.task(&link.task)?.2.thread_id,
        };
        self.workspace.thread(id)
    }

    /// Goes to the thread already started for a task or subtask, or opens the new-thread
    /// dialog for it: titled after it, in the repo (and project) of the parent task's
    /// thread or else the thread on screen.
    fn start_thread(&mut self, link: TaskLink, fx: &mut Vec<Effect>) {
        self.menu = None;
        if let Some(thread) = self.linked_thread(&link).map(|t| t.id.clone()) {
            self.dialog = None;
            self.focus = None;
            self.board.open = false;
            self.select_thread(thread, fx);
            return;
        }
        let Some((_, _, task)) = self.workspace.task(&link.task) else {
            return;
        };
        let title = match &link.subtask {
            Some(s) => match task.subtasks.iter().find(|x| x.id == *s) {
                Some(subtask) => subtask.title.clone(),
                None => return,
            },
            None => task.title.clone(),
        };
        let near = self.workspace.thread(&task.thread_id).or_else(|| {
            self.selected_thread
                .as_ref()
                .and_then(|t| self.workspace.thread(t))
        });
        let repo = near.map(|t| t.project_id.clone());
        let project = near
            .map(|t| t.folder_id.clone())
            .filter(|f| self.workspace.project(f).is_some());
        self.dialog = None;
        self.open_new_thread(repo, project, fx);
        if let Some(Dialog::NewThread {
            title: field,
            task: for_task,
            ..
        }) = &mut self.dialog
        {
            field.set(title);
            *for_task = Some(link);
            self.after_field_edit();
        }
    }
}
