//! End-to-end: projects are reordered with `MoveFolder` (protocol 1.11), and the order
//! survives a daemon restart.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use common::*;
use urushi_client::{Client, ClientError, v1};

async fn names(client: &Client) -> Vec<String> {
    let w = client.list_workspace(false).await.unwrap();
    w.folders.into_iter().map(|f| f.name).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn projects_are_reordered_and_keep_their_order() {
    let home = temp_home();
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let a = client.create_project("A").await.unwrap();
    client.create_project("B").await.unwrap();
    let c = client.create_project("C").await.unwrap();
    assert_eq!(names(&client).await, ["A", "B", "C"]);

    client.move_project(&c.id, 0).await.unwrap();
    assert_eq!(names(&client).await, ["C", "A", "B"]);
    // Past the end is the end.
    client.move_project(&c.id, 99).await.unwrap();
    assert_eq!(names(&client).await, ["A", "B", "C"]);
    client.move_project(&a.id, 1).await.unwrap();
    assert_eq!(names(&client).await, ["B", "A", "C"]);

    // An unknown project is refused.
    let unknown = urushi_core::ProjectId::from_u128(9).to_string();
    let err = client.move_project(&unknown, 0).await.unwrap_err();
    assert!(
        matches!(
            err,
            ClientError::Remote {
                code: v1::ErrorCode::NotFound,
                ..
            }
        ),
        "{err:?}"
    );

    drop(client);
    let home_dir = daemon.home.clone();
    daemon.stop().await;
    let daemon = Daemon::start(&home_dir).await;
    let (client, _events) = daemon.connect().await;
    assert_eq!(names(&client).await, ["B", "A", "C"]);
    daemon.stop().await;
}
