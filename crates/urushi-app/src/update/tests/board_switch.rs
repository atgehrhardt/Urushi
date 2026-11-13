//! The filter belongs to the board on screen (UR-71): whatever changes which board that
//! is (another host, a new or imported board, the shown one deleted, a reorder) starts
//! it afresh, and a reconnect asks the daemon again for what the board needs.

use urushi_client::v1::{self, request::Body, response::Outcome};

use super::bodies;
use super::jira::{jira_board, signed_in, with_jira};
use crate::filter::Dim;
use crate::msg::{FilterMsg, Followup, Msg};
use crate::state::{AppState, Field};

fn changed(s: &mut AppState, boards: Vec<v1::Board>) -> Vec<crate::msg::Effect> {
    let mut workspace = super::workspace();
    workspace.boards = boards;
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)))
}

/// The Jira board on screen on a 1.13 daemon, filtered by epic, "Only my issues" and a
/// quick filter Jira answered, with the search focused.
fn filtered() -> AppState {
    let mut s = with_jira();
    s.daemon_minor = 13;
    s.jira_accounts = vec![signed_in()];
    s.update(Msg::SelectBoard("jb".into()));
    s.update(Msg::Filter(FilterMsg::Toggle(Dim::Epic, "ACME-9".into())));
    s.update(Msg::Filter(FilterMsg::Mine));
    s.update(Msg::Filter(FilterMsg::Quick(4)));
    let query = s.board.quick_query;
    s.update(Msg::Response {
        followup: Followup::QuickFilter("jb".into(), query),
        result: Ok(Outcome::JiraIssueIds(v1::JiraIssueIds {
            ids: vec!["nope".into()],
        })),
    });
    s.update(Msg::FocusField(Field::BoardSearch));
    assert!(s.board.filter.active() && s.board.filter.quick_matches.is_some());
    s
}

fn assert_fresh(s: &AppState) {
    assert!(!s.board.filter.active(), "{:?}", s.board.filter);
    assert!(s.board.filter.quick_matches.is_none());
    assert!(!s.board.quick_asking && !s.board.quick_stale);
    assert_ne!(s.focus, Some(Field::BoardSearch));
}

#[test]
fn another_host_starts_with_no_filter() {
    let mut s = filtered();
    s.update(Msg::ConnectTo("box.tail.ts.net".into()));
    assert!(s.jira_accounts.is_empty(), "the other host's sign-ins");
    let mut workspace = super::workspace();
    let mut other = jira_board();
    other.id = "other".into();
    workspace.boards = vec![other];
    s.update(Msg::Connected {
        welcome: v1::Welcome {
            protocol_minor: 13,
            ..Default::default()
        },
        workspace,
    });
    assert_fresh(&s);
    assert_eq!(s.board.selected, None);
}

#[test]
fn a_new_or_imported_board_starts_with_no_filter() {
    let mut s = filtered();
    let mut new = super::board::board();
    new.id = "new".into();
    let mut boards = s.workspace.raw().boards.clone();
    boards.push(new.clone());
    changed(&mut s, boards);
    s.update(Msg::Response {
        followup: Followup::BoardCreated,
        result: Ok(Outcome::Board(new)),
    });
    assert_eq!(
        s.board.board(&s.workspace).map(|b| b.id.as_str()),
        Some("new")
    );
    assert_fresh(&s);
}

#[test]
fn deleting_the_shown_board_starts_the_next_with_no_filter() {
    let mut s = filtered();
    changed(&mut s, vec![super::board::board()]);
    assert_fresh(&s);
}

#[test]
fn reordering_which_board_shows_first_starts_it_with_no_filter() {
    let mut s = with_jira();
    s.update(Msg::Filter(FilterMsg::Toggle(Dim::Epic, "ACME-9".into())));
    assert_eq!(s.board.selected, None, "the first board shows");
    changed(&mut s, vec![super::board::board(), jira_board()]);
    assert!(!s.board.filter.active());
}

#[test]
fn the_same_board_keeps_its_filter_through_changes_and_reconnects() {
    let mut s = filtered();
    let boards = s.workspace.raw().boards.clone();
    changed(&mut s, boards);
    assert!(s.board.filter.active());
    s.update(Msg::Disconnected("lost".into()));
    let mut workspace = super::workspace();
    workspace.boards = s.workspace.raw().boards.clone();
    let fx = s.update(Msg::Connected {
        welcome: v1::Welcome {
            protocol_minor: 13,
            ..Default::default()
        },
        workspace,
    });
    assert!(s.board.filter.active());
    // The daemon is told again what is on screen, Jira is asked again about the quick
    // filter, and "me" stays known meanwhile.
    let sent = bodies(&fx);
    assert!(
        sent.iter()
            .any(|b| matches!(b, Body::WatchBoard(r) if r.board_id == "jb")),
        "{sent:?}"
    );
    assert!(
        sent.iter()
            .any(|b| matches!(b, Body::QueryJiraFilter(r) if r.board_id == "jb")),
        "{sent:?}"
    );
    assert_eq!(s.filter_context().me, Some("acc-ada"));
}

#[test]
fn a_restarted_daemon_is_told_again_which_board_is_on_screen() {
    let mut s = filtered();
    s.update(Msg::DaemonStopped(Ok(())));
    let mut workspace = super::workspace();
    workspace.boards = s.workspace.raw().boards.clone();
    let fx = s.update(Msg::Connected {
        welcome: v1::Welcome {
            protocol_minor: 13,
            ..Default::default()
        },
        workspace,
    });
    assert!(
        bodies(&fx)
            .iter()
            .any(|b| matches!(b, Body::WatchBoard(r) if r.board_id == "jb"))
    );
}
