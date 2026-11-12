//! Saving the workspace: changes are announced at once and saved together, but a reply
//! still means the change is on disk (ADR 0054).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use common::*;
use urushi_client::{Client, v1};
use urushi_daemon::store::{JsonFileStore, Store};

/// A daemon with one repo, and a thread with one tab of two side-by-side panes.
async fn split_thread(home: &std::path::Path) -> (Daemon, Client, v1::Thread, v1::Tab) {
    let fixture = repo(home);
    let daemon = Daemon::start(&home.join("home")).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&repo.id, "t", "").await.unwrap();
    let tab = client
        .list_workspace(false)
        .await
        .unwrap()
        .tabs
        .into_iter()
        .find(|t| t.thread_id == thread.id)
        .unwrap();
    let first = panes_of(&tab)[0].clone();
    client
        .split_pane(&tab.id, &first, v1::Direction::Right)
        .await
        .unwrap();
    (daemon, client, thread, tab)
}

fn stored(home: &std::path::Path) -> urushi_daemon::store::Document {
    JsonFileStore::new(home.join("home").join("workspace.json"))
        .load()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reply_means_the_change_is_on_disk() {
    let home = temp_home();
    let (daemon, client, thread, _tab) = split_thread(home.path()).await;
    client.rename_thread(&thread.id, "renamed").await.unwrap();
    let document = stored(home.path());
    let saved = document
        .threads
        .iter()
        .find(|r| r.thread.id.to_string() == thread.id)
        .unwrap();
    assert_eq!(saved.thread.title, "renamed");
    drop(client);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_divider_moves_all_succeed_and_are_saved() {
    let home = temp_home();
    let (daemon, client, _thread, tab) = split_thread(home.path()).await;
    let first = panes_of(&tab)[0].clone();
    // A drag: many moves at once, as fast as the client can send them.
    let mut moves = tokio::task::JoinSet::new();
    for i in 1..=50 {
        let client = client.clone();
        let (tab, first) = (tab.id.clone(), first.clone());
        moves.spawn(async move {
            let position = 0.2 + f64::from(i) * 0.01;
            client
                .move_divider(&tab, &first, v1::Axis::Row, position)
                .await
        });
    }
    while let Some(result) = moves.join_next().await {
        result.unwrap().unwrap();
    }
    // The last reply came after the newest state was saved: what's on disk is what the
    // daemon shows.
    let shown = client.list_workspace(false).await.unwrap();
    let shown = shown.tabs.iter().find(|t| t.id == tab.id).unwrap();
    let document = stored(home.path());
    let saved = document
        .tabs
        .iter()
        .find(|t| t.id().to_string() == tab.id)
        .unwrap();
    assert_eq!(
        Some(&urushi_proto::convert::layout_to_proto(saved.layout())),
        shown.layout.as_ref()
    );
    drop(client);
    daemon.stop().await;

    // And a restarted daemon brings back the same layout.
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let restored = client.list_workspace(false).await.unwrap();
    let restored = restored.tabs.iter().find(|t| t.id == tab.id).unwrap();
    assert_eq!(restored.layout, shown.layout);
    drop(client);
    daemon.stop().await;
}
