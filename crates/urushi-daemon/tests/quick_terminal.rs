//! End-to-end: the quick terminal (ADR 0031), split panes that belong to no thread. It
//! opens once per daemon, starts in the home directory, takes the tab layout requests by
//! its id, keeps running without clients, ends with its last pane, and comes back after
//! the daemon restarts.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::time::Duration;

use common::*;
use urushi_client::{Client, v1};

fn panes(quick: &v1::QuickTerminal) -> Vec<String> {
    let layout = urushi_proto::convert::layout_from_proto(quick.layout.as_ref().unwrap()).unwrap();
    layout.panes().iter().map(ToString::to_string).collect()
}

async fn quick(client: &Client) -> Option<v1::QuickTerminal> {
    client.list_workspace(false).await.unwrap().quick_terminal
}

/// Waits until the workspace's quick terminal matches `done`.
async fn wait_for_quick(client: &Client, done: impl Fn(Option<&v1::QuickTerminal>) -> bool) {
    for _ in 0..200 {
        if done(quick(client).await.as_ref()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "quick terminal never changed as expected: {:?}",
        quick(client).await
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_quick_terminal_opens_once_in_home_and_splits_like_a_tab() {
    let home = temp_home();
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    assert!(quick(&client).await.is_none());

    let opened = client.open_quick_terminal().await.unwrap();
    assert_eq!(client.open_quick_terminal().await.unwrap().id, opened.id);
    assert_eq!(quick(&client).await.unwrap().id, opened.id);
    let pane = panes(&opened)[0].clone();

    // No thread: it starts in the home directory, without the thread's variables.
    type_line(
        &client,
        &pane,
        "[ \"$(pwd -P)\" = \"$(cd \"$HOME\" && pwd -P)\" ] && echo \"AT-HOME thread=[$URUSHI_THREAD_ID] pane=$URUSHI_PANE_ID\"",
    )
    .await;
    wait_for_text(&client, &pane, &format!("AT-HOME thread=[] pane={pane}")).await;

    // Layout requests take its id as the tab id.
    let second = client
        .split_pane(&opened.id, &pane, v1::Direction::Down)
        .await
        .unwrap();
    assert_eq!(
        panes(&quick(&client).await.unwrap()),
        [pane.clone(), second.clone()]
    );
    client.focus_pane(&opened.id, &pane).await.unwrap();
    assert_eq!(quick(&client).await.unwrap().focused_pane_id, pane);
    client.equalize(&opened.id).await.unwrap();
    assert!(client.rename_tab(&opened.id, "nope").await.is_err());

    // Closing a pane keeps it; exiting the last one ends it, and the next open is new.
    client.close_pane(&opened.id, &second).await.unwrap();
    assert_eq!(
        panes(&quick(&client).await.unwrap()),
        std::slice::from_ref(&pane)
    );
    type_line(&client, &pane, "exit").await;
    wait_for_quick(&client, |q| q.is_none()).await;
    let fresh = client.open_quick_terminal().await.unwrap();
    assert_ne!(fresh.id, opened.id);

    // Closing it as a tab kills all its panes.
    client
        .split_pane(&fresh.id, &panes(&fresh)[0], v1::Direction::Right)
        .await
        .unwrap();
    client.close_tab(&fresh.id).await.unwrap();
    assert!(quick(&client).await.is_none());
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_quick_terminal_keeps_running_without_clients_and_wants_attention() {
    let home = temp_home();
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let pane = panes(&client.open_quick_terminal().await.unwrap())[0].clone();
    type_line(&client, &pane, "sleep 1; echo still-running-$((40+2))").await;
    drop(client);

    let (client, _events) = daemon.connect().await;
    wait_for_text(&client, &pane, "still-running-42").await;
    type_line(&client, &pane, "printf '\\a'").await;
    wait_for_quick(&client, |q| q.is_some_and(|q| !q.attention.is_empty())).await;
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_quick_terminal_survives_thread_deletion_and_daemon_restarts() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon_home = home.path().join("home");
    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;
    let opened = client.open_quick_terminal().await.unwrap();
    let pane = panes(&opened)[0].clone();
    type_line(&client, &pane, "printf 'quick-%s\\n' marker").await;
    wait_for_text(&client, &pane, "quick-marker").await;

    // Deleting a thread leaves it alone.
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&repo.id, "t", "").await.unwrap();
    let plan = client.plan_thread_deletion(&thread.id).await.unwrap();
    client.delete_thread(&thread.id, &plan.token).await.unwrap();
    assert_eq!(quick(&client).await.unwrap().id, opened.id);
    drop(client);
    daemon.stop().await;

    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;
    assert_eq!(quick(&client).await.unwrap().id, opened.id);
    let screen = wait_for_text(&client, &pane, "restored after the daemon restarted").await;
    assert!(
        screen.iter().any(|l| l == "quick-marker"),
        "old output replayed: {screen:?}"
    );
    daemon.stop().await;
}
