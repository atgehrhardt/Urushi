//! Deleting Jira issues end to end, against an in-memory Jira: a card goes with its
//! sub-tasks, a subtask leaves its parent, a refusal changes nothing, and an issue
//! already gone from Jira is taken off the board too (ADR 0034).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

mod fake_jira;

use std::sync::Arc;

use common::{Daemon, temp_home};
use fake_jira::{FakeJira, TOKEN};
use urushi_client::{Client, v1};
use urushi_daemon::DaemonOptions;
use urushi_daemon::tailscale::StaticTailscale;

async fn board(client: &Client) -> v1::Board {
    let w = client.list_workspace(false).await.unwrap();
    w.boards.into_iter().find(|b| b.jira.is_some()).unwrap()
}

/// The keys on the board, subtasks included, sorted.
fn keys_on(board: &v1::Board) -> Vec<String> {
    fn walk(subtasks: &[v1::Subtask], out: &mut Vec<String>) {
        for s in subtasks {
            out.extend(s.issue.as_ref().map(|i| i.key.clone()));
            walk(&s.subtasks, out);
        }
    }
    let mut out = Vec::new();
    for t in board.columns.iter().flat_map(|c| &c.tasks) {
        out.extend(t.issue.as_ref().map(|i| i.key.clone()));
        walk(&t.subtasks, &mut out);
    }
    out.sort();
    out
}

fn card<'a>(board: &'a v1::Board, key: &str) -> &'a v1::Task {
    board
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .find(|t| t.issue.as_ref().is_some_and(|i| i.key == key))
        .unwrap()
}

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

#[tokio::test(flavor = "multi_thread")]
async fn issues_and_subtasks_are_deleted_in_jira() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(home.path()).await;
    assert_eq!(
        keys_on(&b),
        ["ACME-1", "ACME-2", "ACME-3", "ACME-4", "ACME-9"]
    );

    // A delete Jira refuses (no permission) changes nothing, here or there.
    *jira.deletes_forbidden.lock().unwrap() = true;
    let refused = client
        .delete_task(&card(&b, "ACME-4").id)
        .await
        .unwrap_err();
    assert_eq!(
        refused.code(),
        Some(v1::ErrorCode::PermissionDenied),
        "{refused}"
    );
    assert!(refused.to_string().contains("permission"), "{refused}");
    assert!(jira.has("ACME-4"));
    assert_eq!(keys_on(&board(&client).await), keys_on(&b));
    *jira.deletes_forbidden.lock().unwrap() = false;

    // A card goes with its sub-tasks, in Jira and here.
    client.delete_task(&card(&b, "ACME-2").id).await.unwrap();
    assert!(jira.calls().contains(&"delete ACME-2".to_owned()));
    assert!(!jira.has("ACME-2") && !jira.has("ACME-3"));
    assert_eq!(
        keys_on(&board(&client).await),
        ["ACME-1", "ACME-4", "ACME-9"]
    );

    // A subtask leaves its parent, which stays.
    let new = client
        .create_subtask(&card(&b, "ACME-4").id, "Spike")
        .await
        .unwrap();
    client.delete_subtask(&new.id).await.unwrap();
    assert!(jira.has("ACME-4"));
    let now = board(&client).await;
    assert!(card(&now, "ACME-4").subtasks.is_empty());

    // An issue someone already deleted in Jira is taken off the board too.
    jira.issues.lock().unwrap().retain(|i| i.id != 1);
    client.delete_task(&card(&now, "ACME-1").id).await.unwrap();
    assert_eq!(keys_on(&board(&client).await), ["ACME-4", "ACME-9"]);

    // Reading Jira in full brings none of them back.
    client.sync_board(&b.id).await.unwrap();
    assert_eq!(keys_on(&board(&client).await), ["ACME-4", "ACME-9"]);
    daemon.stop().await;
}
