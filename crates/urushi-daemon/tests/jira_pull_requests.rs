//! Pull requests on tasks (ADR 0036), end to end: a Jira issue linked to a thread gets
//! its pull requests from Jira's development info (and only such an issue is asked
//! about), loses them when the thread goes, and a failing read never marks the board
//! as failing; a local task links one by hand, which a Jira board refuses.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

mod fake_jira;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Daemon, repo, temp_home};
use fake_jira::{FakeJira, TOKEN};
use urushi_client::{Client, v1};
use urushi_core::board::{PrStatus, PrUrl, PullRequest};
use urushi_daemon::DaemonOptions;
use urushi_daemon::jira::JiraError;
use urushi_daemon::tailscale::StaticTailscale;

const PR: &str = "https://github.com/acme/app/pull/7";

async fn signed_in(home: &std::path::Path, jira: Arc<FakeJira>) -> (Daemon, Client, v1::Board) {
    let mut options = DaemonOptions::new(home.join("home"));
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.jira = jira;
    let daemon = Daemon::start_options(options).await;
    let (client, _events) = daemon.connect().await;
    client.connect_jira("acme", "a@b.c", TOKEN).await.unwrap();
    let imported = client.import_jira_board("", 7).await.unwrap();
    client.sync_board(&imported.id).await.unwrap();
    let b = jira_board(&client).await;
    (daemon, client, b)
}

async fn jira_board(client: &Client) -> v1::Board {
    let w = client.list_workspace(false).await.unwrap();
    w.boards.into_iter().find(|b| b.jira.is_some()).unwrap()
}

fn issue<'a>(board: &'a v1::Board, key: &str) -> &'a v1::JiraIssue {
    board
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .find_map(|t| t.issue.as_deref().filter(|i| i.key == key))
        .unwrap()
}

/// Waits (up to 20 s: the daemon looks every few seconds) until `f` holds of the board.
async fn until(client: &Client, what: &str, f: impl Fn(&v1::Board) -> bool) -> v1::Board {
    let start = Instant::now();
    loop {
        let b = jira_board(client).await;
        if f(&b) {
            return b;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "never: {what}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn start_thread(client: &Client, repo_id: &str, task: &v1::Task) -> v1::Thread {
    client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: repo_id.into(),
            title: task.title.clone(),
            task_id: task.id.clone(),
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_linked_issue_gets_its_pull_requests_until_its_thread_goes() {
    let home = temp_home();
    let fixture = repo(home.path());
    let jira = FakeJira::new();
    let pr = PullRequest::new(PrUrl::parse(PR).unwrap(), "ACME-2 fix", PrStatus::Open);
    jira.pull_requests.lock().unwrap().insert(2, Ok(vec![pr]));
    jira.pull_requests.lock().unwrap().insert(
        1,
        Ok(vec![PullRequest::new(
            PrUrl::parse("https://github.com/acme/app/pull/1").unwrap(),
            "",
            PrStatus::Open,
        )]),
    );
    let (daemon, client, b) = signed_in(home.path(), jira.clone()).await;
    assert!(issue(&b, "ACME-2").pull_requests.is_empty());
    let repo_id = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap()
        .id;
    let invoices = b
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .find(|t| t.issue.as_ref().is_some_and(|i| i.key == "ACME-2"))
        .unwrap()
        .clone();
    let thread = start_thread(&client, &repo_id, &invoices).await;

    let b = until(&client, "ACME-2's pull request shows", |b| {
        !issue(b, "ACME-2").pull_requests.is_empty()
    })
    .await;
    let prs = &issue(&b, "ACME-2").pull_requests;
    assert_eq!(prs[0].url, PR);
    assert_eq!(prs[0].status, i32::from(v1::PullRequestStatus::Open));
    assert!(issue(&b, "ACME-1").pull_requests.is_empty());
    let asked: Vec<String> = jira
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("pull_requests"))
        .collect();
    assert!(
        asked.iter().all(|c| c == "pull_requests ACME-2"),
        "{asked:?}"
    );

    // A board sync keeps them.
    client.sync_board(&b.id).await.unwrap();
    assert_eq!(
        issue(&jira_board(&client).await, "ACME-2")
            .pull_requests
            .len(),
        1
    );

    // The thread goes, and so do the issue's pull requests.
    let plan = client.plan_thread_deletion(&thread.id).await.unwrap();
    client.delete_thread(&thread.id, &plan.token).await.unwrap();
    until(&client, "ACME-2's pull request is cleared", |b| {
        issue(b, "ACME-2").pull_requests.is_empty()
    })
    .await;
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_read_leaves_the_board_alone() {
    let home = temp_home();
    let fixture = repo(home.path());
    let jira = FakeJira::new();
    jira.pull_requests
        .lock()
        .unwrap()
        .insert(2, Err(JiraError::Unavailable("down".into())));
    let (daemon, client, b) = signed_in(home.path(), jira.clone()).await;
    let repo_id = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap()
        .id;
    let invoices = b
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .find(|t| t.issue.as_ref().is_some_and(|i| i.key == "ACME-2"))
        .unwrap()
        .clone();
    start_thread(&client, &repo_id, &invoices).await;
    let start = Instant::now();
    while !jira.calls().iter().any(|c| c == "pull_requests ACME-2") {
        assert!(start.elapsed() < Duration::from_secs(20), "never asked");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    client.sync_board(&b.id).await.unwrap();
    let b = jira_board(&client).await;
    assert!(b.jira.as_ref().unwrap().error.is_empty(), "{:?}", b.jira);
    assert!(issue(&b, "ACME-2").pull_requests.is_empty());
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_local_task_links_a_pull_request_and_a_jira_issue_cannot() {
    let home = temp_home();
    let (daemon, client, jb) = signed_in(home.path(), FakeJira::new()).await;
    let board = client.create_board("Local").await.unwrap();
    let task = client
        .create_task(&board.id, &board.columns[0].id, "Ship", "")
        .await
        .unwrap();
    let update = |task_id: &str, url: &str| v1::UpdateTaskRequest {
        task_id: task_id.into(),
        pr_url: Some(url.into()),
        ..Default::default()
    };
    client.update_task_with(update(&task.id, PR)).await.unwrap();
    let linked = |w: &v1::Workspace| {
        w.boards.iter().find(|b| b.id == board.id).unwrap().columns[0].tasks[0]
            .pr_url
            .clone()
    };
    assert_eq!(linked(&client.list_workspace(false).await.unwrap()), PR);

    let bad = client
        .update_task_with(update(&task.id, "http://insecure/pull/1"))
        .await
        .unwrap_err();
    assert_eq!(bad.code(), Some(v1::ErrorCode::InvalidArgument));
    client.update_task_with(update(&task.id, "")).await.unwrap();
    assert_eq!(linked(&client.list_workspace(false).await.unwrap()), "");

    let jira_task = &jb.columns[0].tasks[0];
    let refused = client
        .update_task_with(update(&jira_task.id, PR))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Some(v1::ErrorCode::FailedPrecondition));
    daemon.stop().await;
}
