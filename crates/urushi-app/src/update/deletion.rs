//! Deleting a thread from the GUI, after the warning (`Dialog::DeleteThread`).
//!
//! The warning opens at once, saying it is checking what deleting removes, while the
//! daemon works out the plan (several git commands); its Delete button works once the
//! plan is there (ADR 0055).
//!
//! Confirming closes the warning and hides the thread at once. The daemon then removes
//! its worktree and branches in the background (see [`crate::deletions`]), while the
//! user keeps working: nothing waits for it, and its answer never closes a dialog or
//! moves focus. Only a refusal shows: the thread comes back, with the reason in the
//! error banner.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{Effect, Followup};
use crate::state::{AppState, Dialog};

impl AppState {
    /// A thread's delete button: opens the warning now, and asks the daemon what
    /// deleting would remove.
    pub(super) fn request_delete(&mut self, thread_id: String, fx: &mut Vec<Effect>) {
        self.menu = None;
        self.open_deletion(thread_id.clone(), None);
        fx.push(request(
            Body::PlanThreadDeletion(v1::PlanThreadDeletionRequest { thread_id }),
            Followup::DeletionPlan(None),
        ));
    }

    /// Opens the warning for `thread`, still checking what deleting removes.
    pub(super) fn open_deletion(
        &mut self,
        thread: String,
        then: Option<Box<crate::project_settings::PendingSettings>>,
    ) {
        let title = self
            .workspace
            .thread(&thread)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        self.open(
            Dialog::DeleteThread {
                title,
                thread,
                plan: None,
                then,
            },
            None,
        );
    }

    /// The daemon's plan: shown in the warning that waits for it. If the user closed
    /// that warning meanwhile, it stays closed.
    pub(super) fn show_deletion_plan(
        &mut self,
        plan: v1::DeletionPlan,
        then: Option<Box<crate::project_settings::PendingSettings>>,
    ) {
        if let Some(Dialog::DeleteThread {
            thread,
            plan: shown,
            ..
        }) = &mut self.dialog
            && *thread == plan.thread_id
        {
            *shown = Some(plan);
        } else if then.is_some() {
            // From the project's settings, which stay open until the plan is there.
            self.open_deletion(plan.thread_id.clone(), then);
            self.show_deletion_plan(plan, None);
        }
    }

    /// The daemon couldn't work out the plan: the warning that waits for it closes, and
    /// the reason shows.
    pub(super) fn deletion_unplanned(&mut self, message: &str) {
        if matches!(self.dialog, Some(Dialog::DeleteThread { plan: None, .. })) {
            self.dialog = None;
            self.focus = None;
        }
        self.error = Some(message.to_owned());
    }

    /// The red Delete button: send `DeleteThread` with the plan's token, close the
    /// warning and hide the thread. Closing a parent-branch thread to change its
    /// project's settings saves them once the daemon has closed it.
    pub(super) fn confirm_delete(&mut self, fx: &mut Vec<Effect>) {
        let Some(Dialog::DeleteThread {
            plan: Some(plan),
            then,
            ..
        }) = self
            .dialog
            .take_if(|d| matches!(d, Dialog::DeleteThread { plan: Some(_), .. }))
        else {
            return;
        };
        self.focus = None;
        self.deletions.start(plan.thread_id.clone());
        self.apply_workspace(fx);
        let body = Body::DeleteThread(v1::DeleteThreadRequest {
            thread_id: plan.thread_id.clone(),
            token: plan.token,
        });
        let followup = match then {
            Some(pending) => Followup::ProjectBaseClosed(plan.thread_id, pending),
            None => Followup::ThreadDeleted(plan.thread_id),
        };
        fx.push(request(body, followup));
    }

    /// The daemon answered `DeleteThread`. Success needs nothing: the thread stays
    /// hidden until the daemon's workspace no longer lists it (or lists it as
    /// `DeletionFailed`). A refusal shows the thread again and says why.
    pub(super) fn thread_deleted(
        &mut self,
        thread: &str,
        error: Option<String>,
        fx: &mut Vec<Effect>,
    ) {
        let Some(message) = error else {
            return;
        };
        let title = self
            .deletions
            .reported_thread(thread)
            .map_or_else(|| "the thread".to_owned(), |t| format!("\"{}\"", t.title));
        self.deletions.failed(thread);
        self.apply_workspace(fx);
        self.error = Some(format!("Couldn't delete {title}: {message}"));
    }
}
