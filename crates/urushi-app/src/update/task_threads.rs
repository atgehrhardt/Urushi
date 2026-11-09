//! Starting a thread for a task or a subtask (at any depth), or going to the one
//! already started. The daemon links the thread to the task when it creates it
//! (ADR 0012); from then on the same button goes to that thread.

use urushi_client::v1;

use crate::msg::Effect;
use crate::state::{AppState, Dialog, TaskLink};

impl AppState {
    /// The thread started for a task or subtask, if it still exists.
    pub(crate) fn linked_thread(&self, link: &TaskLink) -> Option<&v1::Thread> {
        let found = self.workspace.find_task(link.target())?;
        self.workspace.thread(found.task().thread_id())
    }

    /// Goes to the thread already started for a task or subtask, or opens the new-thread
    /// dialog for it: titled after it, in the repo (and project) of its card's thread or
    /// else the thread on screen.
    pub(super) fn start_thread(&mut self, link: TaskLink, fx: &mut Vec<Effect>) {
        self.menu = None;
        if let Some(thread) = self.linked_thread(&link).map(|t| t.id.clone()) {
            self.dialog = None;
            self.focus = None;
            self.board.open = false;
            self.select_thread(thread, fx);
            return;
        }
        let Some(found) = self.workspace.find_task(link.target()) else {
            return;
        };
        let title = found.task().title().to_owned();
        let near = self.workspace.thread(&found.card.thread_id).or_else(|| {
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
