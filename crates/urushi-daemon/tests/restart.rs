//! Restarting the local daemon, as after installing a new version: the real `urushid`
//! binary in a throwaway home, started and stopped through `urushi-client`.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use common::*;
use urushi_client::{Endpoint, StopMethod, ensure_local_daemon, stop_local_daemon};
use urushi_proto::endpoint::local_endpoint;

/// Starts the built `urushid` as a separate process, as the launcher would.
async fn start_daemon_process(home: &std::path::Path) -> std::process::Child {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_urushid"))
        .arg("--home")
        .arg(home)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let endpoint = Endpoint::Local(local_endpoint(home));
    for _ in 0..200 {
        if urushi_client::Client::connect(&endpoint, "probe")
            .await
            .is_ok()
        {
            return child;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("urushid did not start");
}

/// A running daemon process with one repo and one thread; returns the thread id.
async fn workspace_with_a_thread(home: &std::path::Path) -> (std::process::Child, String) {
    let fixture = repo(home);
    let child = start_daemon_process(&home.join("home")).await;
    let (client, _events) = ensure_local_daemon(Some(&home.join("home")), "test")
        .await
        .unwrap();
    client
        .update_settings(urushi_client::v1::SettingsPatch {
            default_shell: Some("/bin/sh".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client
        .create_thread(&repo.id, "keep me", "")
        .await
        .unwrap()
        .id;
    (child, thread)
}

#[tokio::test(flavor = "multi_thread")]
async fn restarting_replaces_the_daemon_and_restores_the_workspace() {
    let tmp = temp_home();
    let home = tmp.path().join("home");
    let (mut old, thread) = workspace_with_a_thread(tmp.path()).await;

    // Stop (by request, as for current daemons), then start the next one.
    let stopped = stop_local_daemon(Some(&home), StopMethod::Auto)
        .await
        .unwrap();
    assert_eq!(
        stopped.unwrap().protocol_minor,
        urushi_proto::PROTOCOL_MINOR
    );
    assert!(
        old.wait().unwrap().success(),
        "the old daemon exited cleanly"
    );
    let _new = start_daemon_process(&home).await;
    let (client, _events) = ensure_local_daemon(Some(&home), "test").await.unwrap();

    // The new daemon has the same threads, with their tabs back on fresh shells.
    let w = client.list_workspace(false).await.unwrap();
    let tab = w
        .tabs
        .iter()
        .find(|t| t.thread_id == thread)
        .expect("tab restored");
    let pane = panes_of(tab)[0].clone();
    type_line(&client, &pane, "echo fresh-shell").await;
    wait_for_text(&client, &pane, "fresh-shell").await;

    // Stopping leaves nothing listening; stopping again finds nothing.
    drop(client);
    assert!(
        stop_local_daemon(Some(&home), StopMethod::Auto)
            .await
            .unwrap()
            .is_some()
    );
    let endpoint = Endpoint::Local(local_endpoint(&home));
    assert!(
        urushi_client::Client::connect(&endpoint, "probe")
            .await
            .is_err()
    );
    assert!(
        stop_local_daemon(Some(&home), StopMethod::Auto)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn daemons_too_old_for_shutdown_requests_are_stopped_with_a_signal() {
    let tmp = temp_home();
    let home = tmp.path().join("home");
    let (mut old, _) = workspace_with_a_thread(tmp.path()).await;
    let stopped = stop_local_daemon(Some(&home), StopMethod::Signal)
        .await
        .unwrap();
    assert!(stopped.is_some());
    assert!(old.wait().unwrap().success(), "SIGTERM is a clean shutdown");
    let endpoint = Endpoint::Local(local_endpoint(&home));
    assert!(
        urushi_client::Client::connect(&endpoint, "probe")
            .await
            .is_err()
    );
    assert!(!home.join("urushid.pid").exists(), "it shut down cleanly");
}
