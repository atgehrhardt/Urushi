//! Attention (ADR 0017): what the user is looking at is cleared on the daemon, new
//! raises elsewhere become a system notification while the window is in the background,
//! and threads show the most urgent of their panes.

use urushi_client::v1::{self, request::Body};
use urushi_core::{Layout, PaneId, TabId, ThreadId};
use urushi_proto::convert;
use urushi_ui::Rect;

use super::{CELL, bodies, id, workspace};
use crate::config::ConnectTarget;
use crate::msg::{Effect, Msg};
use crate::state::AppState;

/// The usual workspace plus a second thread ("Other", tab 21, pane 101). `raised` lists
/// (pane, kind, since) attention.
fn with_attention(raised: &[(u128, v1::AttentionKind, i64)]) -> v1::Workspace {
    let mut w = workspace();
    w.threads.push(v1::Thread {
        id: ThreadId::from_u128(11).to_string(),
        title: "Other".into(),
        tab_ids: vec![TabId::from_u128(21).to_string()],
        ..w.threads[0].clone()
    });
    w.tabs.push(v1::Tab {
        id: TabId::from_u128(21).to_string(),
        thread_id: ThreadId::from_u128(11).to_string(),
        title: "Claude".into(),
        layout: Some(convert::layout_to_proto(&Layout::single(
            PaneId::from_u128(101),
        ))),
        focused_pane_id: id(101),
        attention: Vec::new(),
    });
    for (pane, kind, since) in raised {
        let tab = w
            .tabs
            .iter_mut()
            .find(|t| t.focused_pane_id == id(*pane))
            .unwrap();
        tab.attention.push(v1::PaneAttention {
            pane_id: id(*pane),
            kind: (*kind).into(),
            since_ms: *since,
        });
    }
    w
}

/// Connected to a 1.9 daemon, showing the first thread's terminal.
fn showing(raised: &[(u128, v1::AttentionKind, i64)], minor: u32) -> AppState {
    let mut s = AppState::new(ConnectTarget::Local(None), CELL);
    s.update(Msg::Connected {
        welcome: v1::Welcome {
            protocol_major: 1,
            protocol_minor: minor,
            ..Default::default()
        },
        workspace: with_attention(raised),
    });
    s.update(Msg::AreaChanged(Rect::new(0.0, 0.0, 816.0, 400.0)));
    s
}

fn changed(s: &mut AppState, raised: &[(u128, v1::AttentionKind, i64)]) -> Vec<Effect> {
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(
        with_attention(raised),
    )))
}

fn cleared(fx: &[Effect]) -> Vec<String> {
    bodies(fx)
        .iter()
        .flat_map(|b| match b {
            Body::ClearAttention(r) => r.pane_ids.clone(),
            _ => Vec::new(),
        })
        .collect()
}

fn notifications(fx: &[Effect]) -> Vec<(String, String)> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::SystemNotification { title, body } => Some((title.clone(), body.clone())),
            _ => None,
        })
        .collect()
}

const DONE: v1::AttentionKind = v1::AttentionKind::Done;
const WAITING: v1::AttentionKind = v1::AttentionKind::Waiting;

#[test]
fn threads_show_their_most_urgent_pane() {
    let s = showing(&[(101, DONE, 5)], 9);
    let other = ThreadId::from_u128(11).to_string();
    assert_eq!(s.thread_attention(&other), Some(DONE));
    assert_eq!(
        s.tab_attention(&TabId::from_u128(21).to_string()),
        Some(DONE)
    );
    assert_eq!(
        s.thread_attention(&ThreadId::from_u128(10).to_string()),
        None
    );
}

#[test]
fn what_the_user_is_looking_at_is_cleared_once() {
    let mut s = showing(&[], 9);
    let fx = changed(&mut s, &[(100, DONE, 5)]);
    assert_eq!(cleared(&fx), vec![id(100)]);
    assert!(notifications(&fx).is_empty(), "they're looking at it");
    // Until the daemon reports it cleared, it isn't asked again.
    assert!(cleared(&changed(&mut s, &[(100, DONE, 5)])).is_empty());
    // Another thread's pane isn't on screen: it stays, shown in the sidebar.
    assert!(cleared(&changed(&mut s, &[(101, WAITING, 6)])).is_empty());
}

#[test]
fn in_the_background_new_raises_are_announced_once_and_cleared_on_return() {
    let mut s = showing(&[], 9);
    s.update(Msg::WindowFocused(false));
    let fx = changed(&mut s, &[(101, WAITING, 6)]);
    assert_eq!(
        notifications(&fx),
        vec![("Other".to_owned(), "Claude needs your input".to_owned())]
    );
    // The visible pane isn't seen while the window is in the background.
    let fx = changed(&mut s, &[(101, WAITING, 6), (100, DONE, 7)]);
    assert_eq!(notifications(&fx).len(), 1, "only the new raise: {fx:?}");
    assert!(cleared(&fx).is_empty());
    // Back in front: the visible pane is cleared, nothing is announced again.
    let fx = s.update(Msg::WindowFocused(true));
    assert_eq!(cleared(&fx), vec![id(100)]);
    assert!(notifications(&fx).is_empty());
}

#[test]
fn raises_from_before_connecting_are_not_announced() {
    let mut s = showing(&[(101, DONE, 5)], 9);
    s.update(Msg::WindowFocused(false));
    assert!(notifications(&changed(&mut s, &[(101, DONE, 5)])).is_empty());
    // A new raise on the same pane is.
    let fx = changed(&mut s, &[(101, WAITING, 9)]);
    assert_eq!(notifications(&fx).len(), 1);
}

#[test]
fn older_daemons_are_never_asked_to_clear() {
    let mut s = showing(&[], 8);
    assert!(cleared(&changed(&mut s, &[(100, DONE, 5)])).is_empty());
}

#[test]
fn cards_show_the_most_urgent_agent_on_their_threads_and_nested_subtasks() {
    let s = showing(&[(101, DONE, 5), (100, WAITING, 6)], 9);
    let first = ThreadId::from_u128(10).to_string();
    let other = ThreadId::from_u128(11).to_string();
    let subtask = |thread: &str, subtasks| v1::Subtask {
        thread_id: thread.into(),
        subtasks,
        ..Default::default()
    };
    // A card without threads, and one whose only thread is two subtasks down.
    let mut task = v1::Task::default();
    assert_eq!(s.task_attention(&task), None);
    task.subtasks = vec![subtask("", vec![subtask(&other, Vec::new())])];
    assert_eq!(s.task_attention(&task), Some(DONE));
    assert_eq!(s.subtask_attention(&task.subtasks[0]), Some(DONE));
    // Its own thread waits: waiting outranks done.
    task.thread_id = first;
    assert_eq!(s.task_attention(&task), Some(WAITING));
}
