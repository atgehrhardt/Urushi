//! How long an error stays (UR-44): one shown in a dialog goes with the dialog; one
//! that arrives after it closed stays until dismissed.

use urushi_client::v1;

use super::board::board;
use super::bodies;
use crate::msg::{BoardMsg, Effect, Followup, Msg};
use crate::state::{AppState, Dialog};

/// The new-task editor open on a 1.5 daemon's board, with a title typed.
fn new_task_typed() -> AppState {
    let mut s = super::attached();
    s.daemon_minor = 5;
    let mut workspace = super::workspace();
    workspace.boards = vec![board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s.update(Msg::NewTask {
        board: "b".into(),
        column: "todo".into(),
    });
    s.update(Msg::Text("Ship it".into()));
    s
}

/// Closes the editor, discarding what was typed.
fn discard(s: &mut AppState) {
    s.update(Msg::CloseDialog);
    s.update(Msg::Board(BoardMsg::DiscardEdits));
    assert!(s.dialog.is_none());
}

/// Saves the editor and returns the `CreateTask` request's followup.
fn save(s: &mut AppState) -> Followup {
    let fx = s.update(Msg::Submit);
    assert!(matches!(
        bodies(&fx)[..],
        [v1::request::Body::CreateTask(_)]
    ));
    fx.into_iter()
        .find_map(|e| match e {
            Effect::Request { followup, .. } => Some(followup),
            _ => None,
        })
        .expect("a request")
}

#[test]
fn a_refused_new_task_keeps_its_editor_open_with_the_reason() {
    let mut s = new_task_typed();
    let followup = save(&mut s);
    s.update(Msg::Response {
        followup,
        result: Err("Jira refused the issue".into()),
    });
    assert!(matches!(s.dialog, Some(Dialog::Task { .. })), "still open");
    assert_eq!(s.error.as_deref(), Some("Jira refused the issue"));
    assert_eq!(
        crate::errors::error_place(&s),
        crate::errors::ErrorPlace::Dialog
    );
    // Typing on, or being asked whether to discard, keeps it; closing the editor
    // takes it away.
    s.update(Msg::Text("!".into()));
    s.update(Msg::CloseDialog);
    assert!(s.error.is_some());
    s.update(Msg::Board(BoardMsg::DiscardEdits));
    assert_eq!(s.error, None);
}

#[test]
fn an_error_arriving_after_its_dialog_closed_stays_until_dismissed() {
    let mut s = new_task_typed();
    discard(&mut s);
    s.update(Msg::Response {
        followup: Followup::None,
        result: Err("the board is gone".into()),
    });
    assert_eq!(
        crate::errors::error_place(&s),
        crate::errors::ErrorPlace::Board
    );
    s.update(Msg::ToggleBoard);
    assert_eq!(s.error.as_deref(), Some("the board is gone"));
    s.update(Msg::DismissError);
    assert_eq!(s.error, None);
}

#[test]
fn a_dialog_opened_over_an_error_shows_it_and_takes_it_when_it_closes() {
    let mut s = new_task_typed();
    discard(&mut s);
    s.error = Some("no shell".into());
    s.update(Msg::NewTask {
        board: "b".into(),
        column: "todo".into(),
    });
    assert_eq!(
        crate::errors::error_place(&s),
        crate::errors::ErrorPlace::Dialog
    );
    assert!(s.error.is_some());
    s.update(Msg::CloseDialog);
    assert_eq!(s.error, None);
}
