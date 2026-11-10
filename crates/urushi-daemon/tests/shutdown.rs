//! End-to-end: a daemon that stops kills its panes' processes, but those kills are not
//! panes exiting. The stored layout keeps every pane for the next daemon to restore, and
//! nothing the stopped daemon still holds can change the workspace any more.
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
use urushi_client::{ClientError, v1};

/// Whether process `pid` still exists.
fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_keeps_the_layout_and_refuses_changes() {
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
    let pane = panes_of(&tab)[0].clone();
    // Printed as `P=<pid>=D`, which the typed command line itself doesn't contain.
    type_line(&client, &pane, r#"printf 'P%sD\n' "=$$=""#).await;
    let lines = wait_for_text(&client, &pane, "=D").await;
    let pid = lines
        .iter()
        .find_map(|l| {
            let pid = l.split("P=").nth(1)?.split('=').next()?;
            (!pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit())).then_some(pid)
        })
        .expect("the shell printed its pid")
        .to_owned();

    // The client stays connected, so the stopped daemon's connection is still open.
    daemon.stop().await;

    // The daemon killed the shell on its way out; by now its exit has been noticed.
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while alive(&pid) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "shell {pid} still running"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    let stored = std::fs::read_to_string(daemon_home.join("workspace.json")).unwrap();
    assert!(
        stored.contains(&pane),
        "the stopped daemon dropped pane {pane} from the stored layout"
    );

    // What the stopped daemon still serves can't change the workspace.
    let refused = client.create_board("late").await.unwrap_err();
    assert!(
        matches!(
            refused,
            ClientError::Remote {
                code: v1::ErrorCode::Unavailable,
                ..
            }
        ),
        "{refused:?}"
    );
    let after = std::fs::read_to_string(daemon_home.join("workspace.json")).unwrap();
    assert_eq!(stored, after, "the stopped daemon wrote the workspace");
}
