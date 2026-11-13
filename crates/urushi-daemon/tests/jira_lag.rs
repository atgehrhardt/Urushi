//! Jira's search lags behind its writes (UR-71, ADR 0057): a sync right after a change
//! must never put the board back as it was before it, whether it looks at what changed
//! or reads everything, and issues just written are asked for as they are now.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

mod fake_jira;

use std::sync::Arc;
use std::time::Duration;

use common::{Daemon, temp_home};
use fake_jira::{DOING, FakeJira, TOKEN};
use urushi_client::{Client, v1};
use urushi_daemon::DaemonOptions;
use urushi_daemon::tailscale::StaticTailscale;

async fn signed_in(home: &std::path::Path) -> (Daemon, Client, Arc<FakeJira>, v1::Board) {
    let jira = FakeJira::new();
    let mut options = DaemonOptions::new(home.join("home"));
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.jira = jira.clone();
    let daemon = Daemon::start_options(options).await;
    let (client, _events) = daemon.connect().await;
    client.connect_jira("acme", "a@b.c", TOKEN).await.unwrap();
    let imported = client.import_jira_board("", 7).await.unwrap();
    client.sync_board(&imported.id).await.unwrap();
    let b = board(&client).await;
    (daemon, client, jira, b)
}

async fn board(client: &Client) -> v1::Board {
    let w = client.list_workspace(false).await.unwrap();
    w.boards.into_iter().find(|b| b.jira.is_some()).unwrap()
}

/// The column (by index) the issue's card is in.
fn column_of(board: &v1::Board, key: &str) -> Option<usize> {
    board.columns.iter().position(|c| {
        c.tasks
            .iter()
            .any(|t| t.issue.as_ref().is_some_and(|i| i.key == key))
    })
}

fn task_id(board: &v1::Board, key: &str) -> String {
    board
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .find(|t| t.issue.as_ref().is_some_and(|i| i.key == key))
        .unwrap()
        .id
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lagging_search_never_moves_a_card_back() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(home.path()).await;
    // Watched, so it is looked at every few seconds.
    client.watch_board(Some(&b.id)).await.unwrap();
    // The index won't see the move, and doesn't reconcile.
    jira.lag_search(false);
    let invoices = task_id(&b, "ACME-2");
    client
        .move_task(&invoices, &b.columns[2].id, 0)
        .await
        .unwrap();
    assert_eq!(column_of(&board(&client).await, "ACME-2"), Some(2));
    // The sync the write pokes, then the watched board's looks: all see it as it was.
    tokio::time::sleep(urushi_daemon::service::WATCHED_INTERVAL + Duration::from_secs(2)).await;
    assert!(jira.calls().iter().any(|c| c == "changed"));
    assert_eq!(column_of(&board(&client).await, "ACME-2"), Some(2));
    // A full read keeps it too.
    client.sync_board(&b.id).await.unwrap();
    assert_eq!(column_of(&board(&client).await, "ACME-2"), Some(2));
    // A change made in Jira afterwards is newer: it shows.
    jira.catch_up();
    jira.with(2, |r| r.status = DOING);
    client.sync_board(&b.id).await.unwrap();
    assert_eq!(column_of(&board(&client).await, "ACME-2"), Some(1));
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn issues_just_written_are_asked_for_as_they_are_now() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(home.path()).await;
    jira.lag_search(true);
    let checkout = task_id(&b, "ACME-1");
    client
        .move_task(&checkout, &b.columns[1].id, 0)
        .await
        .unwrap();
    // Changed in Jira meanwhile, so the next read is of the issue itself.
    jira.with(1, |r| r.summary = "Checkout, renamed".into());
    client.sync_board(&b.id).await.unwrap();
    let calls = jira.calls();
    assert!(calls.contains(&"reconcile 1001".to_owned()), "{calls:?}");
    let now = board(&client).await;
    assert_eq!(column_of(&now, "ACME-1"), Some(1));
    let title = now
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .find(|t| t.id == checkout)
        .map(|t| t.title.clone());
    assert_eq!(title.as_deref(), Some("Checkout, renamed"));
    daemon.stop().await;
}
