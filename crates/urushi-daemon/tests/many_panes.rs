//! Many panes at once (UR-69): a daemon started with the 256 open files macOS gives GUI
//! apps must still serve a hundred panes. Its own test binary, because it lowers the
//! process's open-file limit, which every test in the process shares.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use common::*;
use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
use urushi_client::v1;

/// More panes than a 256 open-file limit allows (each holds about three descriptors).
const PANES: usize = 100;

#[tokio::test(flavor = "multi_thread")]
async fn a_hundred_panes_dont_exhaust_the_daemon() {
    // What a daemon launched from the macOS app inherits.
    let hard = getrlimit(Resource::Nofile).maximum;
    setrlimit(
        Resource::Nofile,
        Rlimit {
            current: Some(256),
            maximum: hard,
        },
    )
    .unwrap();
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let project = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&project.id, "busy", "").await.unwrap();
    let mut panes = Vec::new();
    while panes.len() < PANES {
        let tab = client
            .create_tab(&thread.id, "t", v1::Agent::Unspecified)
            .await
            .unwrap_or_else(|e| panic!("tab {} failed: {e}", panes.len()));
        panes.extend(panes_of(&tab));
    }

    // Every pane works, the first and the last.
    for pane in [&panes[0], &panes[PANES - 1]] {
        type_line(&client, pane, "echo still-$((40 + 2))").await;
        wait_for_text(&client, pane, "still-42").await;
    }
    // New connections are still accepted, and answered.
    let (second, _events) = daemon.connect().await;
    let workspace = second.list_workspace(false).await.unwrap();
    let tabs = workspace
        .tabs
        .iter()
        .filter(|t| t.thread_id == thread.id)
        .count();
    assert!(tabs >= PANES, "{tabs} tabs");
    let first = second.attach_pane(&panes[PANES - 1], 80, 24).await.unwrap();
    assert!(first.full);
    daemon.stop().await;
}
