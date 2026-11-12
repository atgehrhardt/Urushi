//! A thread's unread mark and notes, and a project's color (UR-62, ADR 0048). The daemon
//! keeps all three; this sends the requests. Opening an unread thread marks it read.

use urushi_client::v1::{self, request::Body};
use urushi_ui::TextField;

use super::request;
use crate::msg::{Effect, Followup, ThreadMsg};
use crate::state::{AppState, Dialog, Field};

impl AppState {
    /// Handles a [`ThreadMsg`].
    pub(super) fn thread_msg(&mut self, msg: ThreadMsg, fx: &mut Vec<Effect>) {
        self.menu = None;
        if !self.daemon_supports_thread_extras() {
            return;
        }
        match msg {
            ThreadMsg::SetUnread { thread, unread } => {
                self.update_thread(thread, |r| r.unread = Some(unread), fx);
            }
            ThreadMsg::OpenNotes(thread) => {
                let Some(current) = self.workspace.thread(&thread).map(|t| t.notes.clone()) else {
                    return;
                };
                self.dialog = Some(Dialog::ThreadNotes {
                    thread,
                    notes: TextField::multiline(current),
                });
                self.focus = Some(Field::ThreadNotes);
            }
            ThreadMsg::SetProjectColor { project, color } => fx.push(request(
                Body::SetFolderColor(v1::SetFolderColorRequest {
                    folder_id: project,
                    color,
                }),
                Followup::None,
            )),
        }
    }

    /// Opening `thread`: if it is marked unread, it is read now.
    pub(super) fn mark_read(&mut self, thread: &str, fx: &mut Vec<Effect>) {
        let unread = self.workspace.thread(thread).is_some_and(|t| t.unread);
        if unread && self.daemon_supports_thread_extras() {
            fx.push(request(
                Body::UpdateThread(v1::UpdateThreadRequest {
                    thread_id: thread.to_owned(),
                    unread: Some(false),
                    ..Default::default()
                }),
                Followup::None,
            ));
        }
    }

    /// Save in the notes editor: replace the thread's notes (blank clears them). The
    /// dialog closes once the daemon has them.
    pub(super) fn submit_thread_notes(&mut self, fx: &mut Vec<Effect>) {
        let Some(Dialog::ThreadNotes { thread, notes }) = &self.dialog else {
            return;
        };
        fx.push(request(
            Body::UpdateThread(v1::UpdateThreadRequest {
                thread_id: thread.clone(),
                notes: Some(notes.text().trim_end().to_owned()),
                ..Default::default()
            }),
            Followup::CloseDialog,
        ));
    }
}
