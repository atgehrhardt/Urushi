//! Jira boards end to end, against an in-memory Jira: signing in, importing, syncing
//! changes made in Jira, and writing changes made here through to Jira.
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
use fake_jira::{DOING, DONE, FakeJira, TODO, TOKEN};
use urushi_client::{Client, ClientError, v1};
use urushi_core::TaskKind;
use urushi_daemon::DaemonOptions;
use urushi_daemon::tailscale::StaticTailscale;

async fn start(home: &std::path::Path, jira: Arc<FakeJira>) -> Daemon {
    let mut options = DaemonOptions::new(home.join("home"));
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.jira = jira;
    Daemon::start_options(options).await
}

async fn board(client: &Client) -> v1::Board {
    let w = client.list_workspace(false).await.unwrap();
    w.boards.into_iter().find(|b| b.jira.is_some()).unwrap()
}

fn keys(board: &v1::Board, column: usize) -> Vec<String> {
    board.columns[column]
        .tasks
        .iter()
        .map(|t| t.issue.as_ref().unwrap().key.clone())
        .collect()
}

fn task<'a>(board: &'a v1::Board, key: &str) -> &'a v1::Task {
    board
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .find(|t| t.issue.as_ref().is_some_and(|i| i.key == key))
        .unwrap()
}

fn code(error: &ClientError) -> Option<v1::ErrorCode> {
    error.code()
}

/// A daemon signed in to the fake Jira, with its board imported and synced.
async fn signed_in(home: &std::path::Path) -> (Daemon, Client, Arc<FakeJira>, v1::Board) {
    let jira = FakeJira::new();
    let daemon = start(home, jira.clone()).await;
    let (client, _events) = daemon.connect().await;
    client.connect_jira("acme", "a@b.c", TOKEN).await.unwrap();
    let imported = client.import_jira_board(7).await.unwrap();
    client.sync_board(&imported.id).await.unwrap();
    let b = board(&client).await;
    (daemon, client, jira, b)
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_in_checks_the_token_with_jira() {
    let home = temp_home();
    let daemon = start(home.path(), FakeJira::new()).await;
    let (client, _events) = daemon.connect().await;
    let bad = client
        .connect_jira("acme", "a@b.c", "wrong")
        .await
        .unwrap_err();
    assert_eq!(code(&bad), Some(v1::ErrorCode::PermissionDenied));
    assert!(!client.jira_account().await.unwrap().connected);
    let insecure = client
        .connect_jira("http://acme.atlassian.net", "a@b.c", TOKEN)
        .await
        .unwrap_err();
    assert_eq!(code(&insecure), Some(v1::ErrorCode::InvalidArgument));
    let account = client.connect_jira("acme", "a@b.c", TOKEN).await.unwrap();
    assert!(account.connected);
    assert_eq!(account.site, "https://acme.atlassian.net");
    assert_eq!(account.display_name, "Ada Lovelace");
    assert_eq!(account.account_id, "acc-ada");
    assert_eq!(client.jira_account().await.unwrap(), account);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn imported_boards_show_jiras_columns_issues_and_changes() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(home.path()).await;
    assert_eq!(b.name, "ACME board");
    assert_eq!(
        b.columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["To Do", "In Progress", "Done"]
    );
    assert_eq!(keys(&b, 0), ["ACME-1", "ACME-4"]);
    assert_eq!(keys(&b, 1), ["ACME-2"]);
    let invoices = task(&b, "ACME-2");
    assert_eq!(invoices.kind, i32::from(v1::TaskKind::Bug));
    assert_eq!(invoices.subtasks[0].title, "Write tests");
    assert_eq!(b.epics[0].key, "ACME-9");
    let checkout = task(&b, "ACME-1").issue.as_ref().unwrap();
    assert_eq!(checkout.epic_key, "ACME-9");
    assert_eq!(checkout.url, "https://acme.atlassian.net/browse/ACME-1");

    // Listing says it is imported, and importing again answers the same board.
    let listed = client.list_jira_boards("").await.unwrap();
    assert_eq!(listed[0].imported_board_id, b.id);
    assert_eq!(client.import_jira_board(7).await.unwrap().id, b.id);

    // Changes made in Jira arrive; issues gone from Jira go.
    jira.with(1, |r| {
        r.summary = "Checkout v2".into();
        r.status = DOING;
    });
    jira.issues.lock().unwrap().retain(|r| r.id != 4);
    client.sync_board(&b.id).await.unwrap();
    let b = board(&client).await;
    assert_eq!(keys(&b, 0), Vec::<String>::new());
    assert_eq!(task(&b, "ACME-1").title, "Checkout v2");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn moves_and_new_cards_take_jiras_transitions() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(home.path()).await;

    let invoices = task(&b, "ACME-2").id.clone();
    client
        .move_task(&invoices, &b.columns[2].id, 0)
        .await
        .unwrap();
    assert!(jira.calls().contains(&format!("transition ACME-2 t{DONE}")));
    assert_eq!(keys(&board(&client).await, 2), ["ACME-2"]);

    // A new card is an issue of its kind, with its labels.
    let fresh = client
        .create_task_with(v1::CreateTaskRequest {
            board_id: b.id.clone(),
            column_id: b.columns[0].id.clone(),
            title: "Refunds".into(),
            kind: v1::TaskKind::Story.into(),
            tags: vec!["payments".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(fresh.issue.as_ref().unwrap().key, "ACME-10");
    assert_eq!(jira.remote("ACME-10").kind, TaskKind::Story);
    assert_eq!(jira.remote("ACME-10").labels, ["payments"]);

    // A move the workflow doesn't allow changes nothing.
    let refused = client
        .move_task(&fresh.id, &b.columns[2].id, 0)
        .await
        .unwrap_err();
    assert_eq!(code(&refused), Some(v1::ErrorCode::FailedPrecondition));
    assert_eq!(jira.remote("ACME-10").status, TODO);
    assert_eq!(
        keys(&board(&client).await, 0),
        ["ACME-1", "ACME-4", "ACME-10"]
    );

    // A card added to another column is moved there in Jira, with its subtasks.
    let started = client
        .create_task_with(v1::CreateTaskRequest {
            board_id: b.id.clone(),
            column_id: b.columns[1].id.clone(),
            title: "Taxes".into(),
            subtasks: vec!["Rates".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(jira.remote("ACME-11").status, DOING);
    assert_eq!(jira.remote("ACME-12").parent, Some(11));
    assert_eq!(started.subtasks[0].title, "Rates");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_and_check_marks_are_written_to_jira() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(home.path()).await;
    let invoices = task(&b, "ACME-2").clone();

    client
        .update_task_with(v1::UpdateTaskRequest {
            task_id: invoices.id.clone(),
            title: Some("Sales invoices".into()),
            tags: Some(v1::TagList {
                tags: vec!["finance".into()],
            }),
            color: Some(v1::LabelColor::Red.into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let remote = jira.remote("ACME-2");
    assert_eq!(remote.summary, "Sales invoices");
    assert_eq!(remote.labels, ["finance"]);
    let b = board(&client).await;
    assert_eq!(task(&b, "ACME-2").title, "Sales invoices");
    assert_eq!(
        task(&b, "ACME-2").color,
        i32::from(v1::LabelColor::Red),
        "the color stays here"
    );

    // Jira refuses labels with spaces; the daemon says so first.
    let spaced = client
        .update_task_with(v1::UpdateTaskRequest {
            task_id: invoices.id.clone(),
            tags: Some(v1::TagList {
                tags: vec!["has space".into()],
            }),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(code(&spaced), Some(v1::ErrorCode::InvalidArgument));

    // A subtask is checked off by a transition to a done status.
    jira.with(3, |r| r.status = DOING);
    let tests = &invoices.subtasks[0];
    client
        .update_subtask(&tests.id, None, Some(true))
        .await
        .unwrap();
    assert_eq!(jira.remote("ACME-3").status, DONE);
    assert!(task(&board(&client).await, "ACME-2").subtasks[0].done);

    // A Jira subtask is created under its card.
    let step = client.create_subtask(&invoices.id, "Docs").await.unwrap();
    assert_eq!(step.title, "Docs");
    assert_eq!(jira.remote("ACME-10").parent, Some(2));
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn what_jira_owns_is_refused() {
    let home = temp_home();
    let (daemon, client, _jira, b) = signed_in(home.path()).await;
    let card = task(&b, "ACME-1").id.clone();
    let subtask = task(&b, "ACME-2").subtasks[0].id.clone();
    for refused in [
        client.delete_task(&card).await.unwrap_err(),
        client.rename_board(&b.id, "Mine").await.unwrap_err(),
        client.delete_column(&b.columns[0].id).await.unwrap_err(),
        client.nest_task(&card, &subtask, 0).await.unwrap_err(),
        client
            .move_task(&subtask, &b.columns[0].id, 0)
            .await
            .unwrap_err(),
    ] {
        assert_eq!(
            code(&refused),
            Some(v1::ErrorCode::FailedPrecondition),
            "{refused}"
        );
    }
    // (The import's own first sync may still touch the sync time.)
    assert_eq!(board(&client).await.columns, b.columns, "nothing changed");
    // Removing the board only removes it here.
    client.delete_board(&b.id).await.unwrap();
    assert!(
        client
            .list_workspace(false)
            .await
            .unwrap()
            .boards
            .is_empty()
    );
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sign_in_survives_restarts_and_never_leaves_the_daemon() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(home.path()).await;
    let settings = format!("{:?}", client.settings().await.unwrap());
    let workspace = format!("{:?}", client.list_workspace(false).await.unwrap());
    assert!(!settings.contains(TOKEN) && !workspace.contains(TOKEN));
    drop(client);
    daemon.stop().await;
    let document = std::fs::read_to_string(home.path().join("home/workspace.json")).unwrap();
    assert!(document.contains("ACME-2") && !document.contains(TOKEN));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = home.path().join("home/jira-credentials.json");
        let mode = std::fs::metadata(file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let daemon = start(home.path(), jira).await;
    let (client, _events) = daemon.connect().await;
    assert!(client.jira_account().await.unwrap().connected);
    assert_eq!(keys(&board(&client).await, 1), keys(&b, 1));

    // Signing out keeps the board, marked as not syncing.
    client.disconnect_jira().await.unwrap();
    assert!(!client.jira_account().await.unwrap().connected);
    let b = board(&client).await;
    assert_eq!(b.jira.as_ref().unwrap().error, "Not signed in to Jira");
    assert!(client.sync_board(&b.id).await.is_err());
    assert!(!home.path().join("home/jira-credentials.json").exists());
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn boards_follow_jira_on_their_own() {
    let home = temp_home();
    let jira = FakeJira::new();
    let daemon = start(home.path(), jira.clone()).await;
    let (client, mut events) = daemon.connect().await;
    client.list_workspace(true).await.unwrap();
    client.connect_jira("acme", "a@b.c", TOKEN).await.unwrap();
    client.import_jira_board(7).await.unwrap();
    // The import's first sync runs in the background and reaches subscribers.
    common::next_event(&mut events, |e| match e {
        v1::event::Body::WorkspaceChanged(w) => w
            .boards
            .iter()
            .any(|b| b.columns.iter().any(|c| !c.tasks.is_empty()))
            .then_some(()),
        _ => None,
    })
    .await;
    assert!(jira.calls().contains(&"issues full=true".to_owned()));
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn quick_filters_are_listed_and_evaluated_by_jira() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(home.path()).await;
    let source = b.jira.as_ref().unwrap();
    assert_eq!(source.quick_filters.len(), 1);
    assert_eq!(source.quick_filters[0].name, "Bugs");
    let ids = client.query_jira_filter(&b.id, &[1]).await.unwrap();
    let bug = task(&b, "ACME-2").issue.as_ref().unwrap();
    assert_eq!(ids, std::slice::from_ref(&bug.id));
    assert!(
        jira.calls()
            .contains(&format!("issue_ids {:?}", [fake_jira::BUGS]))
    );
    let unknown = client.query_jira_filter(&b.id, &[99]).await.unwrap_err();
    assert_eq!(code(&unknown), Some(v1::ErrorCode::NotFound));
    // The issue's ids travel too, for filtering by person and priority.
    assert_eq!(bug.assignee_id, "acc-ada");
    assert_eq!(bug.priority_id, "2");
    daemon.stop().await;
}
