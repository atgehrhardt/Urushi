//! End-to-end: a board's columns (added, recolored, moved, removed with their tasks
//! moved next door) and task colors and tags, all kept across a restart (protocol 1.8).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use common::*;
use urushi_client::{ClientError, v1};

fn names(board: &v1::Board) -> Vec<&str> {
    board.columns.iter().map(|c| c.name.as_str()).collect()
}

fn code(err: &ClientError) -> Option<v1::ErrorCode> {
    match err {
        ClientError::Remote { code, .. } => Some(*code),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn columns_colors_and_tags_are_kept_and_survive_a_restart() {
    let home = temp_home();
    let daemon_home = home.path().join("home");
    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;

    let board = client.create_board("Sprint").await.unwrap();
    let [todo, doing, done] = [0, 1, 2].map(|i| board.columns[i].id.clone());
    let review = client.create_column(&board.id, "Review", 2).await.unwrap();
    assert_eq!(review.name, "Review");
    client
        .update_column(&review.id, Some("In review"), Some(v1::LabelColor::Purple))
        .await
        .unwrap();
    client.move_column(&done, 0).await.unwrap();

    let task = client
        .create_task_with(v1::CreateTaskRequest {
            board_id: board.id.clone(),
            column_id: doing.clone(),
            title: "Ship".into(),
            color: v1::LabelColor::Red.into(),
            tags: vec!["ui".into(), " UI ".into(), "backend".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(task.tags, ["ui", "backend"], "tags are cleaned up");
    assert_eq!(task.color, i32::from(v1::LabelColor::Red));
    client
        .update_task_with(v1::UpdateTaskRequest {
            task_id: task.id.clone(),
            color: Some(v1::LabelColor::Unspecified.into()),
            tags: Some(v1::TagList {
                tags: vec!["docs".into()],
            }),
            ..Default::default()
        })
        .await
        .unwrap();

    // Removing "In progress" moves its task left, to "To do".
    client.delete_column(&doing).await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let b = &w.boards[0];
    assert_eq!(names(b), ["Done", "To do", "In review"]);
    assert_eq!(b.columns[1].tasks[0].title, "Ship");
    assert_eq!(b.columns[1].tasks[0].tags, ["docs"]);
    assert_eq!(b.columns[1].tasks[0].color, 0);
    assert_eq!(b.columns[2].color, i32::from(v1::LabelColor::Purple));

    // Refusals: a blank name, an unknown column, the last column.
    let err = client.create_column(&board.id, " ", 0).await.unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::InvalidArgument), "{err:?}");
    let err = client.move_column(&task.id, 0).await.unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::NotFound), "{err:?}");
    client.delete_column(&done).await.unwrap();
    client.delete_column(&review.id).await.unwrap();
    let err = client.delete_column(&todo).await.unwrap_err();
    assert_eq!(
        code(&err),
        Some(v1::ErrorCode::FailedPrecondition),
        "{err:?}"
    );
    daemon.stop().await;

    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;
    let w = client.list_workspace(false).await.unwrap();
    let b = &w.boards[0];
    assert_eq!(names(b), ["To do"]);
    assert_eq!(b.columns[0].tasks[0].tags, ["docs"]);
    daemon.stop().await;
}
