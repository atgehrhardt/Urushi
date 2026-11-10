//! Deleting a thread from the GUI, after the warning (`Dialog::DeleteThread`).
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
    /// The red Delete button: send `DeleteThread` with the plan's token, close the
    /// warning and hide the thread.
    pub(super) fn confirm_delete(&mut self, fx: &mut Vec<Effect>) {
        let Some(Dialog::DeleteThread { plan, .. }) = self
            .dialog
            .take_if(|d| matches!(d, Dialog::DeleteThread { .. }))
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
        fx.push(request(body, Followup::ThreadDeleted(plan.thread_id)));
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
