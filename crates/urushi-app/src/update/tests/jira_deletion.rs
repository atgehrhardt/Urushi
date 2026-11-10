//! Deleting Jira issues in the app (ADR 0033): Delete always asks first, in the editor
//! or in a dialog from the card's menu; only "Delete from Jira" sends anything, and a
//! daemon older than 1.21 isn't offered it.

use urushi_client::v1::{self, request::Body};

use super::{attached, bodies};
use crate::msg::{BoardMsg, Effect, Followup, Msg};
use crate::state::{AppState, Dialog};

/// The Jira board ("ACME-1" with subtask "ACME-2") and an ordinary one, on a daemon
/// at `minor`, the panel open.
fn with_jira(minor: u32) -> AppState {
    let mut s = attached();
    s.daemon_minor = minor;
    let mut workspace = super::workspace();
    workspace.boards = vec![super::jira::jira_board(), super::board::board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s
}

fn editing(task: &str) -> AppState {
    let mut s = with_jira(21);
    s.update(Msg::Board(BoardMsg::OpenTask(task.into())));
    assert!(matches!(s.dialog, Some(Dialog::Task { .. })));
    s
}

fn asked(s: &AppState) -> Option<(String, String, usize)> {
    match &s.dialog {
        Some(Dialog::Task { nav, .. }) => nav.confirm_delete.as_ref(),
        Some(Dialog::DeleteIssue(about)) => Some(about),
        _ => None,
    }
    .map(|a| (a.task.clone(), a.key.clone(), a.subtasks))
}

fn deleted(fx: &[Effect]) -> Vec<(String, Followup)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Request {
                body: Body::DeleteTask(r),
                followup,
            } => Some((r.task_id.clone(), followup.clone())),
            Effect::Request {
                body: Body::DeleteSubtask(r),
                followup,
            } => Some((r.subtask_id.clone(), followup.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn deleting_an_issue_in_its_editor_asks_then_deletes_and_closes() {
    let mut s = editing("j1");
    let fx = s.update(Msg::DeleteTask);
    assert!(
        bodies(&fx).is_empty(),
        "nothing is sent before the answer: {fx:?}"
    );
    assert_eq!(asked(&s), Some(("j1".into(), "ACME-1".into(), 1)));

    let fx = s.update(Msg::Board(BoardMsg::ConfirmDeleteIssue));
    assert!(
        matches!(&bodies(&fx)[..], [Body::DeleteTask(r)] if r.task_id == "j1"),
        "{fx:?}"
    );
    assert_eq!(deleted(&fx), [("j1".into(), Followup::CloseDialog)]);
    assert_eq!(asked(&s), None);
}

#[test]
fn deleting_a_jira_subtask_from_its_parents_editor_keeps_the_editor() {
    let mut s = editing("j1");
    assert!(s.update(Msg::DeleteSubtask("j2".into())).is_empty());
    assert_eq!(asked(&s), Some(("j2".into(), "ACME-2".into(), 0)));
    let fx = s.update(Msg::Board(BoardMsg::ConfirmDeleteIssue));
    assert_eq!(deleted(&fx), [("j2".into(), Followup::None)]);
    assert!(matches!(&bodies(&fx)[..], [Body::DeleteSubtask(_)]));
}

#[test]
fn keep_or_escape_backs_out_and_sends_nothing() {
    let mut s = editing("j1");
    s.update(Msg::DeleteTask);
    assert!(s.update(Msg::Board(BoardMsg::KeepEditing)).is_empty());
    assert_eq!(asked(&s), None);
    assert!(
        matches!(s.dialog, Some(Dialog::Task { .. })),
        "still editing"
    );

    s.update(Msg::DeleteSubtask("j2".into()));
    assert!(s.update(Msg::CloseDialog).is_empty());
    assert_eq!(asked(&s), None, "closing answers no");
    assert!(
        matches!(s.dialog, Some(Dialog::Task { .. })),
        "still editing"
    );
    // Answering after the question is gone does nothing.
    assert!(deleted(&s.update(Msg::Board(BoardMsg::ConfirmDeleteIssue))).is_empty());
}

#[test]
fn a_cards_menu_asks_in_a_dialog_that_enter_doesnt_confirm() {
    let mut s = with_jira(21);
    assert!(
        s.update(Msg::Board(BoardMsg::DeleteTask("j1".into())))
            .is_empty()
    );
    assert_eq!(asked(&s), Some(("j1".into(), "ACME-1".into(), 1)));
    assert!(
        deleted(&s.update(Msg::Submit)).is_empty(),
        "Enter doesn't delete"
    );
    assert!(matches!(s.dialog, Some(Dialog::DeleteIssue(_))));

    s.update(Msg::CloseDialog);
    assert!(s.dialog.is_none());

    s.update(Msg::Board(BoardMsg::DeleteTask("j1".into())));
    let fx = s.update(Msg::Board(BoardMsg::ConfirmDeleteIssue));
    assert_eq!(deleted(&fx), [("j1".into(), Followup::CloseDialog)]);
}

#[test]
fn tasks_of_urushis_own_are_still_deleted_at_once() {
    let mut s = with_jira(21);
    let own = super::board::board().columns[0].tasks[0].id.clone();
    let fx = s.update(Msg::Board(BoardMsg::DeleteTask(own.clone())));
    assert_eq!(deleted(&fx), [(own, Followup::None)]);
    assert!(s.dialog.is_none());
}

#[test]
fn only_a_daemon_that_deletes_jira_issues_offers_it() {
    let s = with_jira(20);
    let found = s.workspace.find_task("j1").unwrap();
    assert!(!s.can_delete(found.task()));
    let own = super::board::board().columns[0].tasks[0].id.clone();
    assert!(s.can_delete(s.workspace.find_task(&own).unwrap().task()));
    let s = with_jira(21);
    assert!(s.can_delete(s.workspace.find_task("j1").unwrap().task()));
}
