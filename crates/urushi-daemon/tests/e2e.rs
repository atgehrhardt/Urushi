//! End-to-end: a real daemon, real git, real shells, driven through the client library.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use urushi_client::{Client, ClientError, Endpoint, v1};
use urushi_daemon::tailscale::{Identity, StaticTailscale, Status};

#[tokio::test(flavor = "multi_thread")]
async fn thread_lifecycle_with_worktree_and_safe_deletion() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;

    // Projects are deduplicated by repository root.
    let project = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    assert_eq!(project.name, "repo");
    let again = client
        .add_repo(fixture.repo.join(".").to_str().unwrap(), "")
        .await
        .unwrap();
    assert_eq!(again.id, project.id);

    // A thread always gets a worktree on its own branch.
    let thread = client
        .create_thread(&project.id, "Fix the bug", "")
        .await
        .unwrap();
    assert_eq!(thread.state, v1::ThreadState::Ready as i32);
    assert!(thread.primary_branch.starts_with("urushi/fix-the-bug-"));
    assert!(std::path::Path::new(&thread.worktree_path).is_dir());
    assert!(branches(&fixture.repo).contains(&thread.primary_branch));
    assert!(git(&fixture.repo, &["worktree", "list"]).contains(&thread.worktree_path));

    // Its first tab runs a shell in the worktree, with Urushi's environment.
    let workspace = client.list_workspace(false).await.unwrap();
    let tab = workspace
        .tabs
        .iter()
        .find(|t| t.thread_id == thread.id)
        .unwrap()
        .clone();
    let pane = panes_of(&tab)[0].clone();
    type_line(&client, &pane, "echo \"id=$URUSHI_THREAD_ID\"; pwd -P").await;
    wait_for_text(&client, &pane, &format!("id={}", thread.id)).await;
    wait_for_text(&client, &pane, &thread.worktree_path).await;

    // Splits and closes.
    let second = client
        .split_pane(&tab.id, &pane, v1::Direction::Right)
        .await
        .unwrap();
    let ws = client.list_workspace(false).await.unwrap();
    assert_eq!(
        panes_of(ws.tabs.iter().find(|t| t.id == tab.id).unwrap()),
        vec![pane.clone(), second.clone()]
    );
    client.close_pane(&tab.id, &second).await.unwrap();

    // Do real work in the worktree: push the branch, create another one, leave a dirty file.
    let wt = std::path::PathBuf::from(&thread.worktree_path);
    git(&wt, &["push", "-q", "-u", "origin", &thread.primary_branch]);
    git(&wt, &["checkout", "-q", "-b", "feature/extra"]);
    std::fs::write(wt.join("README"), "dirty").unwrap();

    let plan = client.plan_thread_deletion(&thread.id).await.unwrap();
    assert!(
        plan.delete_remote_branches,
        "remote deletion is on by default"
    );
    assert_eq!(
        plan.local_branches,
        vec![thread.primary_branch.clone(), "feature/extra".to_owned()]
    );
    assert_eq!(
        plan.remote_branches,
        vec![v1::RemoteBranch {
            remote: "origin".into(),
            branch: thread.primary_branch.clone()
        }]
    );
    assert_eq!(plan.modified_files, 1);
    assert_eq!(plan.running_panes, 1);
    assert!(!plan.local_branches.contains(&"main".to_owned()));

    // A stale or missing token is refused.
    let err = client.delete_thread(&thread.id, "0000").await.unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::Conflict));
    assert_eq!(
        client
            .delete_thread(&thread.id, "")
            .await
            .unwrap_err()
            .code(),
        Some(v1::ErrorCode::InvalidArgument)
    );

    client.delete_thread(&thread.id, &plan.token).await.unwrap();
    assert!(!wt.exists(), "worktree removed");
    assert_eq!(
        branches(&fixture.repo),
        vec!["main"],
        "owned local branches removed, main kept"
    );
    assert_eq!(
        branches(&fixture.remote),
        vec!["main"],
        "remote branch removed, main kept"
    );
    let ws = client.list_workspace(false).await.unwrap();
    assert!(ws.threads.is_empty() && ws.tabs.is_empty());

    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_deletion_can_be_disabled() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    client
        .update_settings(v1::SettingsPatch {
            delete_remote_branches: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();

    let project = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client
        .create_thread(&project.id, "keep remote", "main")
        .await
        .unwrap();
    git(
        std::path::Path::new(&thread.worktree_path),
        &["push", "-q", "origin", &thread.primary_branch],
    );

    let plan = client.plan_thread_deletion(&thread.id).await.unwrap();
    assert!(!plan.delete_remote_branches);
    assert!(plan.remote_branches.is_empty());
    client.delete_thread(&thread.id, &plan.token).await.unwrap();
    assert!(
        branches(&fixture.remote).contains(&thread.primary_branch),
        "remote branch untouched"
    );
    assert_eq!(branches(&fixture.repo), vec!["main"]);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_survive_client_disconnect_and_stream_with_acks() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let project = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&project.id, "t", "").await.unwrap();
    let tab = client
        .list_workspace(false)
        .await
        .unwrap()
        .tabs
        .into_iter()
        .find(|t| t.thread_id == thread.id)
        .unwrap();
    let pane = panes_of(&tab)[0].clone();

    type_line(&client, &pane, "echo persist-me").await;
    wait_for_text(&client, &pane, "persist-me").await;
    drop(client);

    // A new client sees the same screen.
    let (client, mut events) = Client::connect(&daemon.endpoint(), "second").await.unwrap();
    let first = client.attach_pane(&pane, 80, 24).await.unwrap();
    assert!(first.full && first.seq == 1);
    let text: String = first
        .lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
        .collect();
    assert!(text.contains("persist-me"));

    // Without an ack, no further updates are sent.
    type_line(&client, &pane, "echo one").await;
    let early = tokio::time::timeout(Duration::from_millis(400), events.recv()).await;
    assert!(early.is_err(), "update sent before ack: {early:?}");

    // After the ack, changes arrive as an incremental update.
    client.ack_screen(&pane, 1).await.unwrap();
    let update = next_event(&mut events, |e| match e {
        v1::event::Body::ScreenUpdate(u) => Some(u),
        _ => None,
    })
    .await;
    assert_eq!(update.seq, 2);
    assert!(!update.full);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn exited_processes_close_their_pane() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, mut events) = daemon.connect().await;
    let project = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&project.id, "t", "").await.unwrap();
    let ws = client.list_workspace(true).await.unwrap();
    let tab = ws
        .tabs
        .iter()
        .find(|t| t.thread_id == thread.id)
        .unwrap()
        .clone();
    let first = panes_of(&tab)[0].clone();
    let second = client
        .split_pane(&tab.id, &first, v1::Direction::Down)
        .await
        .unwrap();
    client.attach_pane(&second, 80, 24).await.unwrap();
    client.ack_screen(&second, 1).await.unwrap();

    type_line(&client, &second, "exit 7").await;
    // Both happen; their relative order isn't guaranteed.
    let (mut exit_code, mut remaining) = (None, None);
    while exit_code.is_none() || remaining.is_none() {
        next_event(&mut events, |e| {
            match e {
                v1::event::Body::PaneExited(p) if p.pane_id == second => {
                    exit_code = Some(p.exit_code);
                }
                v1::event::Body::WorkspaceChanged(ws) => {
                    if let Some(tab) = ws.tabs.into_iter().find(|t| t.id == tab.id)
                        && panes_of(&tab).len() == 1
                    {
                        remaining = Some(panes_of(&tab));
                    }
                }
                _ => return None,
            }
            Some(())
        })
        .await;
    }
    assert_eq!(exit_code, Some(7));
    let remaining = remaining.unwrap();
    assert_eq!(remaining, vec![first]);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn restart_restores_layout_and_respawns_shells() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon_home = home.path().join("home");
    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;
    let project = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&project.id, "t", "").await.unwrap();
    let tab = client
        .list_workspace(false)
        .await
        .unwrap()
        .tabs
        .into_iter()
        .find(|t| t.thread_id == thread.id)
        .unwrap();
    let first = panes_of(&tab)[0].clone();
    let second = client
        .split_pane(&tab.id, &first, v1::Direction::Right)
        .await
        .unwrap();
    drop(client);
    daemon.stop().await;

    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;
    let ws = client.list_workspace(false).await.unwrap();
    let restored = ws
        .tabs
        .iter()
        .find(|t| t.id == tab.id)
        .expect("tab restored");
    assert_eq!(panes_of(restored), vec![first.clone(), second]);
    type_line(&client, &first, "echo back-again; pwd -P").await;
    wait_for_text(&client, &first, "back-again").await;
    wait_for_text(&client, &first, &thread.worktree_path).await;
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn guard_rails() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon_home = home.path().join("home");
    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;

    // Only one daemon per home.
    let second = urushi_daemon::run(
        urushi_daemon::DaemonOptions::new(&daemon_home),
        std::future::pending(),
    )
    .await;
    assert!(matches!(
        second,
        Err(urushi_daemon::DaemonError::AlreadyRunning(_))
    ));

    // Not a repository.
    let err = client
        .add_repo(home.path().to_str().unwrap(), "")
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::GitFailed));

    // A project with threads can't be removed.
    let project = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    client.create_thread(&project.id, "t", "").await.unwrap();
    let err = client.remove_repo(&project.id).await.unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::FailedPrecondition));

    // Unknown base refs fail cleanly and leave nothing behind.
    let before = branches(&fixture.repo);
    assert!(
        client
            .create_thread(&project.id, "bad", "no-such-branch")
            .await
            .is_err()
    );
    assert_eq!(branches(&fixture.repo), before);

    // Malformed ids.
    let err = client.close_tab("not-an-id").await.unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::InvalidArgument));
    daemon.stop().await;
}

fn tailnet(addr: std::net::IpAddr, login: &str) -> Arc<StaticTailscale> {
    let mut identities = HashMap::new();
    identities.insert(
        addr,
        Identity {
            login: login.into(),
            node: "phone".into(),
            tags: vec![],
        },
    );
    Arc::new(StaticTailscale {
        status: Ok(Status {
            dns_name: "box.tail.ts.net".into(),
            addresses: vec![addr],
            login: "me@example.com".into(),
            peers: vec![],
        }),
        identities,
    })
}

async fn serve_on_tailnet(client: &Client, port: u16) -> v1::TailnetStatus {
    client
        .update_settings(v1::SettingsPatch {
            tailnet_port: Some(port.into()),
            tailnet_serve: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    client.tailnet_status().await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn tailnet_serving_admits_the_owner() {
    let home = temp_home();
    let loopback = std::net::IpAddr::from([127, 0, 0, 1]);
    let daemon = Daemon::start_with(
        &home.path().join("home"),
        tailnet(loopback, "me@example.com"),
    )
    .await;
    let (client, _events) = daemon.connect().await;
    let status = serve_on_tailnet(&client, 47_391).await;
    assert!(status.running && status.serving, "{status:?}");
    assert_eq!(status.dns_name, "box.tail.ts.net");

    let remote = Endpoint::Tcp {
        host: "127.0.0.1".into(),
        port: 47_391,
    };
    let (remote_client, _events) = Client::connect(&remote, "phone").await.unwrap();
    assert!(remote_client.list_workspace(false).await.is_ok());
    // Even the owner can't stop the daemon over the tailnet.
    let err = remote_client.shutdown_daemon().await.unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::PermissionDenied));
    assert!(client.list_workspace(false).await.is_ok(), "still running");

    let off = client.set_tailnet_serve(false).await.unwrap();
    assert!(!off.serving);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn tailnet_serving_refuses_other_users() {
    let home = temp_home();
    let loopback = std::net::IpAddr::from([127, 0, 0, 1]);
    let daemon = Daemon::start_with(
        &home.path().join("home"),
        tailnet(loopback, "stranger@example.com"),
    )
    .await;
    let (client, _events) = daemon.connect().await;
    assert!(serve_on_tailnet(&client, 47_392).await.serving);

    let remote = Endpoint::Tcp {
        host: "127.0.0.1".into(),
        port: 47_392,
    };
    let result = Client::connect(&remote, "intruder").await;
    assert!(
        matches!(
            result,
            Err(ClientError::Protocol(_) | ClientError::Frame(_))
        ),
        "{:?}",
        result.map(|_| ())
    );
    daemon.stop().await;
}
