//! A dialog waiting on the daemon to save it (ADR 0055). Many dialogs close only once
//! the daemon answered (`Followup::CloseDialog`): a rename, notes, a task. Meanwhile
//! their main button says "Saving…" and submitting again does nothing, so nothing is
//! sent twice (a dialog changed since, or another one opened, can be submitted). The
//! answer closes the open dialog only if it is of the kind that was saved, never some
//! other dialog opened meanwhile; its contents may have changed since (a Jira issue's
//! details arriving). An error leaves it open, ready to try again.

use std::mem::discriminant;

use crate::msg::{Effect, Followup, Msg};
use crate::state::AppState;

impl AppState {
    /// Whether the open dialog waits for the daemon to save it.
    pub fn dialog_saving(&self) -> bool {
        self.submitting.is_some() && self.submitting.as_deref() == self.dialog.as_ref()
    }

    /// Before a message: a submit while the dialog is saving does nothing.
    pub(super) fn ignores_while_saving(&self, msg: &Msg) -> bool {
        matches!(msg, Msg::Submit) && self.dialog_saving()
    }

    /// After a message: if it sent something that closes the open dialog when
    /// answered, the dialog now waits for that answer.
    pub(super) fn note_saving(&mut self, fx: &[Effect]) {
        let closes = fx.iter().any(|e| {
            matches!(
                e,
                Effect::Request {
                    followup: Followup::CloseDialog,
                    ..
                }
            )
        });
        if closes && let Some(dialog) = &self.dialog {
            self.submitting = Some(Box::new(dialog.clone()));
        }
    }

    /// The daemon saved what the dialog asked for: close it, if it is still the one
    /// that asked.
    pub(super) fn saved(&mut self) {
        let same_kind = match (&self.submitting, &self.dialog) {
            (Some(saved), Some(open)) => discriminant(&**saved) == discriminant(open),
            _ => false,
        };
        if same_kind {
            self.dialog = None;
            self.focus = None;
        }
        self.submitting = None;
    }

    /// The daemon refused: the dialog stays open and can be submitted again.
    pub(super) fn not_saved(&mut self) {
        self.submitting = None;
    }
}
