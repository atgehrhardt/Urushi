//! End-to-end: subtasks as nested tasks (ADR 0015): nesting cards, splitting subtasks
//! back onto the board, refusals, deep subtasks edited like tasks, and threads linked
//! and prompted at any depth (protocol 1.8).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use common::*;
use urushi_client::{ClientError, WorkspaceMirror, v1};

fn code(err: &ClientError) -> Option<v1::ErrorCode> {
    match err {
        ClientError::Remote { code, .. } => Some(*code),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn cards_and_subtasks_move_into_and_out_of_each_other() {
    let home = temp_home();
    let daemon_home = home.path().join("home");
    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;

    let board = client.create_board("Sprint").await.unwrap();
    let [todo, done] = [0, 2].map(|i| board.columns[i].id.clone());
    let parent = client
        .create_task(&board.id, &todo, "Parent", "")
        .await
        .unwrap();
    let finished = client
        .create_task(&board.id, &done, "Finished", "")
        .await
        .unwrap();
    let step = client.create_subtask(&parent.id, "Step").await.unwrap();
    // A subtask gets subtasks of its own, and is edited like any task.
    let inner = client.create_subtask(&step.id, "Inner").await.unwrap();
    client
        .update_task_with(v1::UpdateTaskRequest {
            task_id: inner.id.clone(),
            notes: Some("deep notes".into()),
            tags: Some(v1::TagList {
                tags: vec!["deep".into()],
            }),
            ..Default::default()
        })
        .await
        .unwrap();

    // A card from the last column nests checked off.
    client.nest_task(&finished.id, &parent.id, 0).await.unwrap();
    let w = WorkspaceMirror::new(client.list_workspace(false).await.unwrap());
    let found = w.find_task(&finished.id).unwrap();
    assert!(found.task().done());
    assert_eq!(
        found.parent().map(urushi_client::TaskRef::id),
        Some(parent.id.as_str())
    );
    assert!(w.board(&board.id).unwrap().columns[2].tasks.is_empty());
    let inner_now = w.find_task(&inner.id).unwrap();
    assert_eq!(inner_now.path.len(), 3);
    assert_eq!(inner_now.task().notes(), "deep notes");
    assert_eq!(inner_now.task().tags(), ["deep"]);

    // Splitting "Step" out: a card again, with "Inner" still under it.
    client.move_task(&step.id, &todo, 0).await.unwrap();
    let w = WorkspaceMirror::new(client.list_workspace(false).await.unwrap());
    let card = w.task(&step.id).unwrap().2;
    assert_eq!(card.subtasks[0].id, inner.id);
    assert_eq!(w.find_task(&parent.id).unwrap().task().subtasks().len(), 1);

    // Refusals change nothing: a task under its own subtask, an unknown parent.
    let err = client.nest_task(&step.id, &inner.id, 0).await.unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::InvalidArgument), "{err:?}");
    let err = client
        .nest_task(&step.id, "00000000-0000-0000-0000-000000000009", 0)
        .await
        .unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::NotFound), "{err:?}");

    // Deleting a card deletes its subtasks.
    client.delete_task(&step.id).await.unwrap();
    let w = WorkspaceMirror::new(client.list_workspace(false).await.unwrap());
    assert!(w.find_task(&inner.id).is_none());
    daemon.stop().await;

    // Nesting survives a restart.
    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;
    let w = WorkspaceMirror::new(client.list_workspace(false).await.unwrap());
    assert!(w.find_task(&finished.id).unwrap().task().done());
    daemon.stop().await;
}
