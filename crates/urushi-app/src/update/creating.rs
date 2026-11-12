//! Creating a thread without waiting on screen (ADR 0056): the New Thread dialog closes
//! as soon as it is submitted, a placeholder shows until the daemon answers
//! (`creations.rs`), and while the dialog is open the daemon is asked to fetch the base
//! ahead of time (ADR 0055).

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{Effect, Followup};
use crate::state::{AppState, Dialog, Field};

/// Title shown for a thread created without one (the daemon's default too).
const DEFAULT_TITLE: &str = "New thread";

impl AppState {
    /// Whether the daemon can fetch a new thread's base ahead of time (protocol 1.31).
    pub fn daemon_supports_prepare_base(&self) -> bool {
        self.daemon_minor >= urushi_client::PREPARE_THREAD_BASE_SINCE_MINOR
    }

    /// While the New Thread dialog is open: asks the daemon to start fetching the base
    /// it would start from, so creating the thread can reuse the fetch. Sent when the
    /// dialog opens and whenever its repo, base or project changes.
    pub(super) fn prepare_base(&self, fx: &mut Vec<Effect>) {
        let Some(Dialog::NewThread {
            repo,
            project,
            base,
            ..
        }) = &self.dialog
        else {
            return;
        };
        if !self.daemon_supports_prepare_base() {
            return;
        }
        fx.push(request(
            Body::PrepareThreadBase(v1::PrepareThreadBaseRequest {
                project_id: repo.clone(),
                base_ref: base.clone().unwrap_or_default(),
                folder_id: project.clone().unwrap_or_default(),
            }),
            Followup::Ignore,
        ));
    }

    /// Submitting the New Thread dialog for a new thread: closes it, shows a placeholder
    /// in its place, and asks the daemon to create the thread.
    pub(super) fn submit_new_thread(
        &mut self,
        body: v1::CreateThreadRequest,
        fx: &mut Vec<Effect>,
    ) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        self.focus = None;
        let title = match body.title.as_str() {
            "" => DEFAULT_TITLE.to_owned(),
            t => t.to_owned(),
        };
        let project = Some(body.folder_id.clone()).filter(|p| !p.is_empty());
        let key = self
            .creations
            .start(body.project_id.clone(), project, title, dialog);
        // Started from a task: the board makes way for the new thread.
        if self.creations.get(key).is_some_and(|p| p.for_task) {
            self.board.open = false;
        }
        fx.push(request(
            Body::CreateThread(body),
            Followup::ThreadCreated(key),
        ));
        self.show_pending(key, fx);
    }

    /// Shows the placeholder of a thread being created.
    pub(super) fn show_pending(&mut self, key: u64, fx: &mut Vec<Effect>) {
        if self.creations.get(key).is_none() {
            return;
        }
        self.selected_thread = None;
        self.selected_pending = Some(key);
        self.sidebar_open = false;
        self.selection = None;
        self.sync_panes(fx);
    }

    /// Whether the main area shows the placeholder of the create with `key`.
    pub(crate) fn shows_pending(&self, key: u64) -> bool {
        self.selected_thread.is_none() && self.selected_pending == Some(key)
    }

    /// The pending create the main area shows, if any.
    pub(crate) fn shown_pending(&self) -> Option<&crate::creations::PendingThread> {
        let key = self.selected_pending?;
        self.shows_pending(key)
            .then(|| self.creations.get(key))
            .flatten()
    }

    /// The daemon created the thread for the create with `key`. It is shown if its
    /// placeholder still was; otherwise the user has moved on, and it just appears in the
    /// sidebar. Never touches whatever dialog is open now.
    pub(super) fn thread_created(&mut self, key: u64, thread: v1::Thread, fx: &mut Vec<Effect>) {
        let shown = self.shows_pending(key);
        self.creations.finish(key);
        if self.selected_pending == Some(key) {
            self.selected_pending = None;
        }
        if shown {
            self.selected_thread = Some(thread.id);
            self.sync_panes(fx);
        }
        // The new thread shows the launcher; make sure it lists what's installed.
        self.request_agents(fx);
    }

    /// The daemon couldn't create the thread for the create with `key`: says why, and
    /// brings the dialog back as it was submitted, unless another dialog is open.
    pub(super) fn thread_not_created(&mut self, key: u64, message: String) {
        let Some(pending) = self.creations.finish(key) else {
            return;
        };
        if self.selected_pending == Some(key) {
            self.selected_pending = None;
        }
        self.error = Some(message);
        if self.dialog.is_none() {
            self.dialog = Some(pending.dialog);
            self.focus = Some(Field::ThreadTitle);
        }
    }
}
