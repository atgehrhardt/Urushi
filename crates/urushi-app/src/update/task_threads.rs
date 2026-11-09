//! Starting a thread for a task or a subtask (at any depth), starting it in a thread that
//! already exists, or going to the one already started. The daemon links the thread to
//! the task when it creates the thread or the task's tab in it (ADR 0012, ADR 0018);
//! from then on the same button goes to that thread.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{Effect, Followup};
use crate::state::{AppState, Dialog, Field, TaskLink, ThreadTarget};

impl AppState {
    /// The thread started for a task or subtask, if it still exists.
    pub(crate) fn linked_thread(&self, link: &TaskLink) -> Option<&v1::Thread> {
        let found = self.workspace.find_task(link.target())?;
        self.workspace.thread(found.task().thread_id())
    }

    /// The thread a task most likely belongs near: its card's thread, or else the thread
    /// on screen.
    fn thread_near(&self, link: &TaskLink) -> Option<&v1::Thread> {
        let found = self.workspace.find_task(link.target())?;
        self.workspace.thread(&found.card.thread_id).or_else(|| {
            self.selected_thread
                .as_ref()
                .and_then(|t| self.workspace.thread(t))
        })
    }

    /// The threads a task can be started in: every ready thread, by repo, labeled
    /// "repo › title".
    pub(crate) fn existing_thread_choices(&self) -> Vec<(String, String)> {
        self.workspace
            .repos()
            .iter()
            .flat_map(|repo| {
                self.workspace
                    .threads_of_repo(&repo.id)
                    .filter(|t| t.state() == v1::ThreadState::Ready)
                    .map(|t| (t.id.clone(), format!("{} › {}", repo.name, t.title)))
            })
            .collect()
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
        let near = self.thread_near(&link);
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

    /// The switch in the new-thread dialog for a task. Existing preselects the thread
    /// near the task, if it can take the task.
    pub(super) fn use_existing_thread(&mut self, existing: bool) {
        let Some(Dialog::NewThread {
            task: Some(link), ..
        }) = &self.dialog
        else {
            return;
        };
        let target = if existing && self.daemon_supports_task_in_thread() {
            let near = self.thread_near(link).map(|t| t.id.clone());
            ThreadTarget::Existing(
                near.filter(|id| self.existing_thread_choices().iter().any(|(c, _)| c == id)),
            )
        } else {
            ThreadTarget::New
        };
        self.focus = (target == ThreadTarget::New).then_some(Field::ThreadTitle);
        if let Some(Dialog::NewThread {
            target: t, open, ..
        }) = &mut self.dialog
        {
            *t = target;
            *open = None;
        }
    }

    /// Picks the existing thread to start the task in.
    pub(super) fn pick_existing_thread(&mut self, thread: String) {
        if let Some(Dialog::NewThread {
            target: ThreadTarget::Existing(picked),
            ..
        }) = &mut self.dialog
        {
            *picked = Some(thread);
        }
    }

    /// Submitting the dialog in Existing mode: a new tab in the picked thread, with the
    /// task linked and (when one is chosen) the task agent running it. Nothing without a
    /// picked thread.
    pub(super) fn submit_task_in_thread(&self, fx: &mut Vec<Effect>) {
        let Some(Dialog::NewThread {
            task: Some(link),
            target: ThreadTarget::Existing(Some(thread)),
            ..
        }) = &self.dialog
        else {
            return;
        };
        let body = Body::CreateTab(v1::CreateTabRequest {
            thread_id: thread.clone(),
            task_id: link.task.clone(),
            subtask_id: link.subtask.clone().unwrap_or_default(),
            start_task_agent: self.task_agent().is_some(),
            ..Default::default()
        });
        fx.push(request(body, Followup::TaskTabCreated));
    }

    /// The task's tab exists: the board and dialog make way for it.
    pub(super) fn task_tab_created(&mut self, tab: v1::Tab, fx: &mut Vec<Effect>) {
        self.dialog = None;
        self.focus = None;
        self.board.open = false;
        self.selected_tab.insert(tab.thread_id.clone(), tab.id);
        self.select_thread(tab.thread_id, fx);
    }
}
