//! End-to-end: threads are reordered with `MoveThread` (protocol 1.24), reordering keeps
//! each thread's project, pin and status, and the order survives a daemon restart.
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

async fn titles(client: &Client) -> Vec<String> {
    let w = client.list_workspace(false).await.unwrap();
    w.threads.into_iter().map(|t| t.title).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn threads_are_reordered_and_keep_their_order() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let mut ids = Vec::new();
    for title in ["A", "B", "C"] {
        let thread = client
            .create_thread_with(v1::CreateThreadRequest {
                project_id: repo.id.clone(),
                title: title.into(),
                skip_initial_tab: true,
                ..Default::default()
            })
            .await
            .unwrap();
        ids.push(thread.id);
    }
    assert_eq!(titles(&client).await, ["A", "B", "C"]);

    client.move_thread(&ids[2], 0).await.unwrap();
    assert_eq!(titles(&client).await, ["C", "A", "B"]);
    // Past the end is the end.
    client.move_thread(&ids[2], 99).await.unwrap();
    assert_eq!(titles(&client).await, ["A", "B", "C"]);
    client.move_thread(&ids[0], 1).await.unwrap();
    assert_eq!(titles(&client).await, ["B", "A", "C"]);

    // Reordering leaves the thread in its project, pin and status.
    let project = client.create_project("Billing").await.unwrap();
    client
        .update_thread(v1::UpdateThreadRequest {
            thread_id: ids[0].clone(),
            pinned: Some(true),
            status: Some("Review".into()),
            folder_id: Some(project.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    client.move_thread(&ids[0], 0).await.unwrap();
    assert_eq!(titles(&client).await, ["A", "B", "C"]);
    let w = client.list_workspace(false).await.unwrap();
    let a = &w.threads[0];
    assert!(
        a.pinned && a.status == "Review" && a.folder_id == project.id,
        "{a:?}"
    );
    client.move_thread(&ids[0], 1).await.unwrap();

    // An unknown thread is refused.
    let unknown = urushi_core::ThreadId::from_u128(9).to_string();
    let err = client.move_thread(&unknown, 0).await.unwrap_err();
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
    assert_eq!(titles(&client).await, ["B", "A", "C"]);
    daemon.stop().await;
}
