//! Running agents (ADR 0033): threads, tabs, cards and subtask rows show the most
//! important of their panes' states, waiting > running > finished, and the pulse runs
//! only while something is running.

use urushi_client::v1;
use urushi_core::{TabId, ThreadId};

use super::attention::{showing, with_attention};
use super::id;
use crate::msg::Msg;
use crate::running::Indicator;
use crate::state::AppState;

const DONE: v1::AttentionKind = v1::AttentionKind::Done;
const WAITING: v1::AttentionKind = v1::AttentionKind::Waiting;

/// The workspace with `raised` attention (as in `attention.rs`) and `running` panes.
fn with_running(raised: &[(u128, v1::AttentionKind, i64)], running: &[u128]) -> v1::Workspace {
    let mut w = with_attention(raised);
    for pane in running {
        let tab = w
            .tabs
            .iter_mut()
            .find(|t| t.focused_pane_id == id(*pane))
            .unwrap();
        tab.running.push(v1::PaneRunning {
            pane_id: id(*pane),
            since_ms: 1,
        });
    }
    w
}

fn changed(s: &mut AppState, raised: &[(u128, v1::AttentionKind, i64)], running: &[u128]) {
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(
        with_running(raised, running),
    )));
}

fn thread(n: u128) -> String {
    ThreadId::from_u128(n).to_string()
}

#[test]
fn threads_and_tabs_show_running_below_waiting_and_above_finished() {
    let mut s = showing(&[], 21);
    assert_eq!(s.thread_indicator(&thread(11)), None);
    assert!(!s.any_running());

    changed(&mut s, &[], &[101]);
    assert_eq!(s.thread_indicator(&thread(11)), Some(Indicator::Running));
    assert_eq!(
        s.tab_indicator(&TabId::from_u128(21).to_string()),
        Some(Indicator::Running)
    );
    assert_eq!(s.thread_indicator(&thread(10)), None);
    assert!(s.any_running());

    // An older "finished" on the same thread: the agent at work wins.
    changed(&mut s, &[(101, DONE, 5)], &[101]);
    assert_eq!(s.thread_indicator(&thread(11)), Some(Indicator::Running));
    // Something waiting for the user wins over that.
    changed(&mut s, &[(101, WAITING, 6)], &[101]);
    assert_eq!(s.thread_indicator(&thread(11)), Some(Indicator::Waiting));
    // Stopped: back to plain attention.
    changed(&mut s, &[(101, DONE, 7)], &[]);
    assert_eq!(s.thread_indicator(&thread(11)), Some(Indicator::Done));
    assert!(!s.any_running());
}

#[test]
fn cards_show_agents_running_on_their_threads_and_nested_subtasks() {
    let mut s = showing(&[], 21);
    changed(&mut s, &[(100, DONE, 5)], &[101]);
    let subtask = |thread: &str, subtasks| v1::Subtask {
        thread_id: thread.into(),
        subtasks,
        ..Default::default()
    };
    let mut task = v1::Task::default();
    assert_eq!(s.task_indicator(&task), None);
    // Its only running thread is two subtasks down.
    task.subtasks = vec![subtask("", vec![subtask(&thread(11), Vec::new())])];
    assert_eq!(s.task_indicator(&task), Some(Indicator::Running));
    assert_eq!(
        s.subtask_indicator(&task.subtasks[0]),
        Some(Indicator::Running)
    );
    // Its own thread finished; the subtask's agent still works.
    task.thread_id = thread(10);
    assert_eq!(s.task_indicator(&task), Some(Indicator::Running));
    // Running isn't attention: the card's border, announcements and clearing ignore it.
    assert_eq!(s.task_attention(&task), Some(DONE));
}

#[test]
fn older_daemons_show_nothing_running() {
    // A daemon before 1.21 sends no running panes.
    let s = showing(&[(101, DONE, 5)], 20);
    assert!(!s.any_running());
    assert_eq!(s.thread_indicator(&thread(11)), Some(Indicator::Done));
}

#[test]
fn the_pulse_only_changes_brightness() {
    let mut s = showing(&[], 21);
    assert!((s.pulse - 1.0).abs() < f32::EPSILON, "full until it pulses");
    let fx = s.update(Msg::Pulse(0.25));
    assert!(fx.is_empty());
    assert!((s.pulse - 0.25).abs() < f32::EPSILON);
    s.update(Msg::Pulse(7.0));
    assert!((s.pulse - 1.0).abs() < f32::EPSILON, "clamped");
}
