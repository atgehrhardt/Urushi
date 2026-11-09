//! Starting a board task in an existing thread from the new-thread dialog (ADR 0018).

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{RepoId, ThreadId};

use super::board::board;
use super::bodies;
use super::setup::{on, settings};
use crate::msg::{Followup, Msg};
use crate::state::{AppState, Dialog, Field, TaskLink, ThreadTarget};

fn other_thread() -> String {
    ThreadId::from_u128(11).to_string()
}

/// Connected to a daemon of protocol `minor`, with a board, a second thread, and the
/// task agent set to Claude in plan mode; the dialog is open for task "a".
fn dialog_for_task(minor: u32, task_agent: v1::Agent) -> AppState {
    let mut s = on(minor);
    s.update(settings(task_agent, v1::AgentMode::Plan));
    let mut workspace = super::workspace();
    workspace.threads.push(v1::Thread {
        id: other_thread(),
        project_id: RepoId::from_u128(1).to_string(),
        title: "Other".into(),
        state: v1::ThreadState::Ready.into(),
        ..Default::default()
    });
    workspace.boards = vec![board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s.update(Msg::StartThread(TaskLink {
        task: "a".into(),
        subtask: None,
    }));
    s
}

fn target(s: &AppState) -> ThreadTarget {
    match &s.dialog {
        Some(Dialog::NewThread { target, .. }) => target.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_task_starts_in_the_picked_thread_in_a_new_tab() {
    let mut s = dialog_for_task(10, v1::Agent::Claude);
    let near = ThreadId::from_u128(10).to_string();
    s.update(Msg::UseExistingThread(true));
    assert_eq!(
        target(&s),
        ThreadTarget::Existing(Some(near)),
        "the thread on screen is picked first"
    );
    assert_eq!(s.focus, None, "no text fields to type in");

    s.update(Msg::PickExistingThread(other_thread()));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::CreateTab(v1::CreateTabRequest {
                thread_id,
                task_id,
                subtask_id,
                start_task_agent: true,
                ..
            })] if *thread_id == other_thread() && task_id == "a" && subtask_id.is_empty()
        ),
        "{fx:?}"
    );

    s.update(Msg::Response {
        followup: Followup::TaskTabCreated,
        result: Ok(Outcome::Tab(v1::Tab {
            id: "new tab".into(),
            thread_id: other_thread(),
            ..Default::default()
        })),
    });
    assert!(s.dialog.is_none());
    assert!(!s.board.open, "the board makes way for the thread");
    assert_eq!(s.selected_thread, Some(other_thread()));
    assert_eq!(
        s.selected_tab.get(&other_thread()).map(String::as_str),
        Some("new tab")
    );
}

#[test]
fn without_a_task_agent_the_task_gets_a_plain_tab() {
    let mut s = dialog_for_task(10, v1::Agent::Unspecified);
    s.update(Msg::UseExistingThread(true));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::CreateTab(v1::CreateTabRequest {
                task_id,
                start_task_agent: false,
                ..
            })] if task_id == "a"
        ),
        "{fx:?}"
    );
}

#[test]
fn switching_back_creates_a_new_thread_and_nothing_is_sent_unpicked() {
    let mut s = dialog_for_task(10, v1::Agent::Claude);
    s.update(Msg::UseExistingThread(true));
    s.update(Msg::UseExistingThread(false));
    assert_eq!(target(&s), ThreadTarget::New);
    assert_eq!(s.focus, Some(Field::ThreadTitle));
    let fx = s.update(Msg::Submit);
    assert!(matches!(bodies(&fx)[..], [Body::CreateThread(_)]), "{fx:?}");

    let mut s = dialog_for_task(10, v1::Agent::Claude);
    if let Some(Dialog::NewThread { target, .. }) = &mut s.dialog {
        *target = ThreadTarget::Existing(None);
    }
    assert!(bodies(&s.update(Msg::Submit)).is_empty());
}

#[test]
fn only_a_task_on_a_new_enough_daemon_can_use_an_existing_thread() {
    // A 1.9 daemon would ignore the task in CreateTab.
    let mut s = dialog_for_task(9, v1::Agent::Claude);
    s.update(Msg::UseExistingThread(true));
    assert_eq!(target(&s), ThreadTarget::New);

    // A plain new thread has no task to send.
    let mut s = on(10);
    s.update(Msg::OpenNewThread {
        repo: None,
        project: None,
    });
    s.update(Msg::UseExistingThread(true));
    assert_eq!(target(&s), ThreadTarget::New);
}
