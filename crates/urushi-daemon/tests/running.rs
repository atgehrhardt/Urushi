//! End-to-end: an agent's hooks say it works, clients see the pane running on its tab,
//! and finishing, ending its session, an interrupt key or closing the pane stop it
//! (ADR 0033).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use common::*;
use urushi_client::v1;
use urushi_core::AttentionKind;
use urushi_core::agent::hooks::{idle_command, running_command, signal_command};

/// Whether `pane` runs in the next workspace where that differs from `previous`.
async fn running_change(events: &mut urushi_client::Events, pane: &str, previous: bool) -> bool {
    next_event(events, |event| {
        let v1::event::Body::WorkspaceChanged(w) = event else {
            return None;
        };
        let now = w
            .tabs
            .iter()
            .flat_map(|t| &t.running)
            .any(|r| r.pane_id == pane);
        (now != previous).then_some(now)
    })
    .await
}

/// Whether `pane` runs, as the daemon lists it now.
async fn running_now(client: &urushi_client::Client, pane: &str) -> bool {
    client
        .list_workspace(false)
        .await
        .unwrap()
        .tabs
        .iter()
        .flat_map(|t| &t.running)
        .any(|r| r.pane_id == pane)
}

fn escape() -> v1::input::Kind {
    v1::input::Kind::Key(v1::KeyInput {
        key: v1::Key::Escape.into(),
        ..Default::default()
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn hooks_say_when_an_agent_runs_and_its_end_or_an_interrupt_stops_it() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, mut events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&repo.id, "t", "").await.unwrap();
    let w = client.list_workspace(true).await.unwrap();
    let tab = w.tabs.iter().find(|t| t.thread_id == thread.id).unwrap();
    assert!(tab.running.is_empty());
    let pane = panes_of(tab)[0].clone();
    let revision = client.list_workspace(false).await.unwrap().revision;

    // What Claude's prompt and tool hooks run. Running isn't stored.
    type_line(&client, &pane, &running_command()).await;
    assert!(running_change(&mut events, &pane, false).await);
    assert_eq!(
        client.list_workspace(false).await.unwrap().revision,
        revision
    );

    // Typing (a queued message) doesn't stop it.
    client
        .send_input(&pane, v1::input::Kind::Text("x".into()))
        .await
        .unwrap();
    assert!(running_now(&client, &pane).await);
    client
        .send_input(
            &pane,
            v1::input::Kind::Key(v1::KeyInput {
                key: v1::Key::Backspace.into(),
                ..Default::default()
            }),
        )
        .await
        .unwrap();

    // Its Stop hook: finished, so no longer running.
    type_line(&client, &pane, &signal_command(AttentionKind::Done)).await;
    assert!(!running_change(&mut events, &pane, true).await);

    // Running again, then the session ends.
    type_line(&client, &pane, &running_command()).await;
    assert!(running_change(&mut events, &pane, false).await);
    type_line(&client, &pane, &idle_command()).await;
    assert!(!running_change(&mut events, &pane, true).await);

    // Running again, then the user presses Esc: Claude runs no hook when interrupted.
    type_line(&client, &pane, &running_command()).await;
    assert!(running_change(&mut events, &pane, false).await);
    client.send_input(&pane, escape()).await.unwrap();
    assert!(!running_change(&mut events, &pane, true).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_pane_no_longer_runs() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, mut events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&repo.id, "t", "").await.unwrap();
    let w = client.list_workspace(true).await.unwrap();
    let tab = w.tabs.iter().find(|t| t.thread_id == thread.id).unwrap();
    let pane = panes_of(tab)[0].clone();
    type_line(&client, &pane, &running_command()).await;
    assert!(running_change(&mut events, &pane, false).await);

    // The pane's shell exits: the pane goes (with its tab), and with it its running mark.
    type_line(&client, &pane, "exit").await;
    assert!(!running_change(&mut events, &pane, true).await);
    assert!(!running_now(&client, &pane).await);
}
