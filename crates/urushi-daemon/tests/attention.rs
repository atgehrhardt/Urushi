//! End-to-end: a pane's program asks for the user (agent hooks, bells), clients see it
//! on the tab, and typing or looking clears it (ADR 0017).
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
use urushi_core::agent::hooks::signal_command;

/// The attention of `pane` in the next workspace where it differs from `previous`.
async fn attention_change(
    events: &mut urushi_client::Events,
    pane: &str,
    previous: Option<v1::AttentionKind>,
) -> Option<v1::AttentionKind> {
    next_event(events, |event| {
        let v1::event::Body::WorkspaceChanged(w) = event else {
            return None;
        };
        let now = w
            .tabs
            .iter()
            .flat_map(|t| &t.attention)
            .find(|a| a.pane_id == pane)
            .map(v1::PaneAttention::kind);
        (now != previous).then_some(now)
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn hooks_and_bells_raise_attention_and_typing_or_looking_clears_it() {
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
    assert!(tab.attention.is_empty());
    let pane = panes_of(tab)[0].clone();

    // What an agent's Stop hook runs: the shell writes Urushi's signal to $URUSHI_TTY.
    type_line(&client, &pane, &signal_command(AttentionKind::Done)).await;
    let done = attention_change(&mut events, &pane, None).await;
    assert_eq!(done, Some(v1::AttentionKind::Done));

    // Typing clears it; a bell is a program waiting for the user.
    type_line(&client, &pane, "printf '\\a'").await;
    let mut seen = attention_change(&mut events, &pane, done).await;
    if seen.is_none() {
        seen = attention_change(&mut events, &pane, None).await;
    }
    assert_eq!(seen, Some(v1::AttentionKind::Waiting));

    // A client looking at the pane clears it, without changing the stored revision.
    let before = client.list_workspace(false).await.unwrap().revision;
    client
        .clear_attention(std::slice::from_ref(&pane))
        .await
        .unwrap();
    assert_eq!(attention_change(&mut events, &pane, seen).await, None);
    assert_eq!(client.list_workspace(false).await.unwrap().revision, before);

    // So does typing into it.
    type_line(&client, &pane, &signal_command(AttentionKind::Waiting)).await;
    let waiting = attention_change(&mut events, &pane, None).await;
    assert_eq!(waiting, Some(v1::AttentionKind::Waiting));
    client
        .send_input(&pane, v1::input::Kind::Text("x".into()))
        .await
        .unwrap();
    assert_eq!(attention_change(&mut events, &pane, waiting).await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn agents_typed_by_the_daemon_load_urushis_hooks() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&repo.id, "t", "").await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let tab = w.tabs.iter().find(|t| t.thread_id == thread.id).unwrap();
    let pane = panes_of(tab)[0].clone();
    // Every pane has the hook settings and its terminal's path.
    type_line(
        &client,
        &pane,
        "[ -w \"$URUSHI_TTY\" ] && echo \"tty-ok stop-hooks=$(echo \"$URUSHI_CLAUDE_SETTINGS\" | grep -c Stop)\"",
    )
    .await;
    wait_for_text(&client, &pane, "tty-ok stop-hooks=1").await;
}
