//! The task editor: creating and editing a task or a subtask at any depth (title,
//! notes, color, tags, its column or check mark), its subtasks, and moving between
//! a task and its subtasks.
//!
//! Nothing is saved before **Save**: Enter in the title moves on to the notes, and
//! subtasks typed in the editor are drafts, created with a new task
//! (`CreateTask.subtasks`) or one by one for an existing one. Checking an existing
//! subtask off and deleting one are immediate. Moving between a task and its subtasks
//! keeps every editor's unsaved edits (`task_nav.rs`). Threads are in `task_threads.rs`.

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
    pub(super) fn new_task(&mut self, board: String, column: String, fx: &mut Vec<Effect>) {
        self.open(
            Dialog::Task {
                board,
                column,
                task: None,
                title: TextField::default(),
                notes: TextField::multiline(""),
                subtasks: Box::default(),
                labels: Box::default(),
                nav: Box::default(),
                issue: Box::default(),
            },
            Some(Field::TaskTitle),
        );
        self.request_create_fields(fx);
    }

    /// Enter in the task editor: in the title it moves on to the notes (it never saves,
    /// so a task isn't created, in Jira too, half typed); in the tag or subtask field it
    /// adds what's typed. Returns whether it did something (otherwise Enter saves).
    pub(super) fn task_editor_enter(&mut self, fx: &mut Vec<Effect>) -> bool {
        let focus = self.focus;
        if !matches!(self.dialog, Some(Dialog::Task { .. })) {
            return false;
        }
        match focus {
            Some(Field::TaskTitle) => {
                self.focus = Some(Field::TaskNotes);
                return true;
            }
            // The Jira side: Enter searches, posts, logs, or leaves a field.
            Some(Field::JiraSearch) => {
                self.update_issue(crate::msg::IssueMsg::Search, fx);
                return true;
            }
            Some(Field::JiraWork) => {
                self.update_issue(crate::msg::IssueMsg::LogWork, fx);
                return true;
            }
            Some(Field::JiraText(_)) => {
                self.focus = None;
                return true;
            }
            _ => {}
        }
        let Some(Dialog::Task {
            subtasks, labels, ..
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
                subtasks.drafts.push(title);
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
            issue,
            ..
        }) = &self.dialog
        else {
            return;
        };
        if title.text().trim().is_empty() {
            return;
        }
        if task.is_none() {
            // Jira refuses an issue without its required fields: say which first.
            let missing = Self::missing_fields(issue);
            if !missing.is_empty() {
                self.error = Some(format!("Fill in {} first.", missing.join(", ")));
                return;
            }
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
                jira_fields: issue.edits(),
            });
            fx.push(request(body, Followup::CloseDialog));
            return;
        }
        let changes = self.editor_changes();
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

    /// The requests that save an existing task's editor: its draft subtasks (and one
    /// still typed in its field), a new column, and its changed fields (the last one
    /// carries `last`, so the editor closes when it is done). Nothing for a new task's
    /// editor, or a task deleted meanwhile.
    pub(super) fn task_changes(&self, dialog: &Dialog, last: Followup) -> Vec<Effect> {
        let Dialog::Task {
            column,
            task: Some(task),
            title,
            notes,
            subtasks,
            labels,
            issue,
            ..
        } = dialog
        else {
            return Vec::new();
        };
        let Some(found) = self.workspace.find_task(task) else {
            return Vec::new();
        };
        let current = found.task();
        let mut out = Vec::new();
        let typed = Some(subtasks.new.text().trim().to_owned()).filter(|t| !t.is_empty());
        for title in subtasks.drafts.iter().cloned().chain(typed) {
            let body = Body::CreateSubtask(v1::CreateSubtaskRequest {
                task_id: task.clone(),
                title,
            });
            out.push(request(body, Followup::None));
        }
        // A Jira issue's other fields (assignee, priority…): one change, made in Jira.
        if let Some(body) = Self::issue_edits(task, issue) {
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
pub(super) fn editor_for(found: &FoundTask<'_>) -> Dialog {
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
        nav: Box::default(),
        issue: Box::default(),
    }
}
