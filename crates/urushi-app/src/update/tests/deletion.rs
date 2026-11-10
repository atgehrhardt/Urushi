//! Deleting threads in the background (UR-8): the thread disappears when the warning is
//! confirmed, nothing waits for the daemon, and its answer never moves focus.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{TabId, ThreadId};

use super::{attached, bodies, workspace};
use crate::msg::{Effect, Followup, Msg};
use crate::state::{AppState, Dialog};

fn thread_id(n: u128) -> String {
    ThreadId::from_u128(n).to_string()
}

/// The sample workspace plus thread 11 (tab 21, same layout), attached to thread 10.
fn two_threads() -> (AppState, v1::Workspace) {
    let mut ws = workspace();
    let mut other = ws.threads[0].clone();
    other.id = thread_id(11);
    other.title = "Other".into();
    other.tab_ids = vec![TabId::from_u128(21).to_string()];
    ws.threads.push(other);
    let mut tab = ws.tabs[0].clone();
    tab.id = TabId::from_u128(21).to_string();
    tab.thread_id = thread_id(11);
    ws.tabs.push(tab);
    let mut s = attached();
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(ws.clone())));
    (s, ws)
}

/// Opens the warning for `thread` and confirms it.
fn confirm(s: &mut AppState, thread: &str) -> Vec<Effect> {
    s.update(Msg::RequestDeleteThread(thread.to_owned()));
    s.update(Msg::Response {
        followup: Followup::DeletionPlan(None),
        result: Ok(Outcome::DeletionPlan(v1::DeletionPlan {
            thread_id: thread.to_owned(),
            token: "tok".into(),
            ..Default::default()
        })),
    });
    s.update(Msg::ConfirmDelete)
}

fn answer(s: &mut AppState, thread: &str, result: Result<Outcome, String>) -> Vec<Effect> {
    s.update(Msg::Response {
        followup: Followup::ThreadDeleted(thread.to_owned()),
        result,
    })
}

#[test]
fn deleting_another_thread_leaves_the_current_one_alone() {
    let (mut s, _) = two_threads();
    let (selected, focus) = (s.selected_thread.clone(), s.focused_pane());
    let fx = confirm(&mut s, &thread_id(11));
    assert!(s.workspace.thread(&thread_id(11)).is_none());
    assert_eq!(s.selected_thread, selected);
    assert_eq!(s.focused_pane(), focus);
    assert!(
        !bodies(&fx).iter().any(|b| matches!(b, Body::DetachPane(_))),
        "the shown terminals stay attached: {fx:?}"
    );
}

#[test]
fn deleting_the_current_thread_shows_another() {
    let (mut s, _) = two_threads();
    assert_eq!(s.selected_thread, Some(thread_id(10)));
    confirm(&mut s, &thread_id(10));
    assert_eq!(s.selected_thread, Some(thread_id(11)));
    assert_eq!(
        s.current_tab().map(|t| t.thread_id.clone()),
        Some(thread_id(11))
    );
}

#[test]
fn the_answer_never_closes_a_dialog_or_moves_focus() {
    let (mut s, ws) = two_threads();
    confirm(&mut s, &thread_id(11));
    // The user moves on and opens Settings before the daemon is done.
    s.update(Msg::OpenSettings);
    let (dialog, focus) = (s.dialog.clone(), s.focus);
    assert!(matches!(dialog, Some(Dialog::Settings { .. })));

    // Done: the answer can beat the workspace that drops the thread.
    answer(&mut s, &thread_id(11), Ok(Outcome::Ok(v1::Empty {})));
    assert_eq!((&s.dialog, s.focus), (&dialog, focus));
    assert_eq!(s.error, None);
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(ws.clone())));
    assert!(s.workspace.thread(&thread_id(11)).is_none(), "no flicker");

    let mut gone = ws;
    gone.threads.retain(|t| t.id != thread_id(11));
    gone.tabs.retain(|t| t.thread_id != thread_id(11));
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(gone)));
    assert!(!s.deletions.is_pending(&thread_id(11)));
    assert_eq!((&s.dialog, s.focus), (&dialog, focus));
}

#[test]
fn a_refusal_brings_the_thread_back_and_says_why() {
    let (mut s, _) = two_threads();
    confirm(&mut s, &thread_id(11));
    s.update(Msg::OpenSettings);
    let dialog = s.dialog.clone();
    answer(
        &mut s,
        &thread_id(11),
        Err("the thread changed since the deletion warning was shown".into()),
    );
    assert!(s.workspace.thread(&thread_id(11)).is_some());
    assert_eq!(
        s.error.as_deref(),
        Some("Couldn't delete \"Other\": the thread changed since the deletion warning was shown")
    );
    assert_eq!(s.dialog, dialog, "the open dialog stays");
}

#[test]
fn threads_deleting_elsewhere_are_hidden_and_failed_ones_shown() {
    let (mut s, mut ws) = two_threads();
    ws.threads[1].state = v1::ThreadState::Deleting.into();
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(ws.clone())));
    assert!(s.workspace.thread(&thread_id(11)).is_none());
    assert!(s.workspace.tab(&TabId::from_u128(21).to_string()).is_none());

    ws.threads[1].state = v1::ThreadState::DeletionFailed.into();
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(ws)));
    assert!(s.workspace.thread(&thread_id(11)).is_some());
}
