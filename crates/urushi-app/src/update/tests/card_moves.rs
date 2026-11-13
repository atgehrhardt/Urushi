//! A dropped card shows where it was dropped at once, and stays there until the daemon
//! answers, through workspaces that still have it where it was (ADR 0056, UR-71).

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_ui::Point;

use super::board::{board, with_board};
use super::bodies;
use crate::msg::{Effect, Followup, Msg};
use crate::state::AppState;

fn ids(s: &AppState, column: usize) -> Vec<String> {
    let b = s.board.board(&s.workspace).unwrap();
    b.columns[column]
        .tasks
        .iter()
        .map(|t| t.id.clone())
        .collect()
}

/// Drops task "a" into Done; returns the move's followup.
fn drop_a_in_done(s: &mut AppState) -> Followup {
    s.update(Msg::Pointer(Point::new(10.0, 10.0)));
    s.update(Msg::PressTask("a".into()));
    s.update(Msg::DragTask("a".into()));
    s.update(Msg::DragMove(Point::new(50.0, 60.0)));
    let fx = s.update(Msg::DropTask {
        column: "done".into(),
        before: None,
    });
    assert!(matches!(bodies(&fx)[..], [Body::MoveTask(_)]));
    fx.into_iter()
        .find_map(|e| match e {
            Effect::Request { followup, .. } => Some(followup),
            _ => None,
        })
        .unwrap()
}

/// The daemon's workspace, with the board as `board()` has it: "a" still in To do.
fn unmoved(s: &mut AppState) {
    let mut workspace = super::workspace();
    workspace.boards = vec![board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
}

#[test]
fn a_dropped_card_stays_where_it_was_dropped_until_the_daemon_answers() {
    let mut s = with_board();
    let followup = drop_a_in_done(&mut s);
    assert_eq!(ids(&s, 0), ["b"]);
    assert_eq!(ids(&s, 1), ["a"]);
    // A sync of the old picture arrives meanwhile: it stays put.
    unmoved(&mut s);
    assert_eq!(ids(&s, 1), ["a"]);
    // The daemon answers; from then on its workspace is what shows.
    s.update(Msg::Response {
        followup,
        result: Ok(Outcome::Ok(v1::Empty {})),
    });
    unmoved(&mut s);
    assert_eq!(ids(&s, 0), ["a", "b"]);
}

#[test]
fn a_refused_move_puts_the_card_back_and_says_why() {
    let mut s = with_board();
    let followup = drop_a_in_done(&mut s);
    assert_eq!(ids(&s, 1), ["a"]);
    s.update(Msg::Response {
        followup,
        result: Err("Jira's workflow doesn't allow that".into()),
    });
    assert_eq!(ids(&s, 0), ["a", "b"]);
    assert_eq!(
        s.error.as_deref(),
        Some("Jira's workflow doesn't allow that")
    );
}
