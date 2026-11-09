//! End-to-end: scrolling and mouse input reaching terminal panes.
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

/// A connected daemon with one thread; returns its first pane.
async fn pane(
    home: &std::path::Path,
) -> (Daemon, urushi_client::Client, urushi_client::Events, String) {
    let fixture = repo(home);
    let daemon = Daemon::start(&home.join("home")).await;
    let (client, events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&repo.id, "t", "").await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let tab = w.tabs.iter().find(|t| t.thread_id == thread.id).unwrap();
    let pane = panes_of(tab)[0].clone();
    (daemon, client, events, pane)
}

fn text(update: &v1::ScreenUpdate) -> String {
    update
        .lines
        .iter()
        .flat_map(|l| l.spans.iter().map(|s| s.text.clone()))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn scrolling_an_attached_pane_sends_history() {
    let home = temp_home();
    let (daemon, client, mut events, pane) = pane(home.path()).await;
    type_line(
        &client,
        &pane,
        "i=1; while [ $i -le 60 ]; do echo row$i; i=$((i+1)); done",
    )
    .await;
    wait_for_text(&client, &pane, "row60").await;

    let first = client.attach_pane(&pane, 40, 10).await.unwrap();
    client.ack_screen(&pane, first.seq).await.unwrap();
    client.scroll_pane(&pane, 20, false).await.unwrap();
    let update = next_event(&mut events, |e| match e {
        v1::event::Body::ScreenUpdate(u) if u.display_offset > 0 => Some(u),
        _ => None,
    })
    .await;
    assert_eq!(update.display_offset, 20);
    assert!(text(&update).contains("row4"), "{}", text(&update));
    daemon.stop().await;
}

/// Makes the shell turn on `modes`, then print the next `bytes` bytes it reads with ESC
/// shown as `E`.
async fn echo_raw_input(client: &urushi_client::Client, pane: &str, modes: &str, bytes: usize) {
    type_line(
        client,
        pane,
        &format!(
            "printf '{modes}'; stty raw -echo; head -c {bytes} | tr '\\033' E; stty sane; echo; echo done"
        ),
    )
    .await;
    // Let the shell reach `head` before sending.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
}

async fn send(client: &urushi_client::Client, pane: &str, kind: v1::input::Kind) {
    client.send_input(pane, kind).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn clicks_and_the_wheel_reach_apps_that_ask_for_the_mouse() {
    let home = temp_home();
    let (daemon, client, _events, pane) = pane(home.path()).await;
    // Button-event tracking with SGR encoding, as Claude Code turns on.
    echo_raw_input(&client, &pane, "\\033[?1002h\\033[?1006h", 20).await;
    let press = v1::MouseInput {
        button: v1::MouseButton::Left.into(),
        action: v1::MouseAction::Press.into(),
        col: 9,
        row: 4,
        modifiers: 0,
    };
    send(&client, &pane, v1::input::Kind::Mouse(press)).await;
    send(
        &client,
        &pane,
        v1::input::Kind::Wheel(v1::WheelInput {
            up: true,
            lines: 1,
            col: 0,
            row: 0,
            modifiers: 0,
        }),
    )
    .await;
    wait_for_text(&client, &pane, "E[<0;10;5ME[<64;1;1M").await;
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_wheel_sends_arrows_on_the_alternate_screen() {
    let home = temp_home();
    let (daemon, client, _events, pane) = pane(home.path()).await;
    echo_raw_input(&client, &pane, "\\033[?1049h", 6).await;
    send(
        &client,
        &pane,
        v1::input::Kind::Wheel(v1::WheelInput {
            up: false,
            lines: 2,
            col: 0,
            row: 0,
            modifiers: 0,
        }),
    )
    .await;
    // A click without mouse reporting sends nothing.
    let click = v1::MouseInput {
        button: v1::MouseButton::Left.into(),
        action: v1::MouseAction::Press.into(),
        col: 1,
        row: 1,
        modifiers: 0,
    };
    send(&client, &pane, v1::input::Kind::Mouse(click)).await;
    type_line(&client, &pane, "\u{1b}[?1049l").await;
    wait_for_text(&client, &pane, "E[BE[B").await;
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_wheel_scrolls_history_in_a_plain_shell() {
    let home = temp_home();
    let (daemon, client, mut events, pane) = pane(home.path()).await;
    type_line(
        &client,
        &pane,
        "i=1; while [ $i -le 60 ]; do echo row$i; i=$((i+1)); done",
    )
    .await;
    wait_for_text(&client, &pane, "row60").await;
    let first = client.attach_pane(&pane, 40, 10).await.unwrap();
    client.ack_screen(&pane, first.seq).await.unwrap();
    send(
        &client,
        &pane,
        v1::input::Kind::Wheel(v1::WheelInput {
            up: true,
            lines: 15,
            col: 0,
            row: 0,
            modifiers: 0,
        }),
    )
    .await;
    let update = next_event(&mut events, |e| match e {
        v1::event::Body::ScreenUpdate(u) if u.display_offset > 0 => Some(u),
        _ => None,
    })
    .await;
    assert_eq!(update.display_offset, 15);
    daemon.stop().await;
}

/// The app queues input with `try_notify` straight from the UI thread; a burst of
/// presses and releases must reach the application in the order it happened.
#[tokio::test(flavor = "multi_thread")]
async fn queued_input_arrives_in_order() {
    let home = temp_home();
    let (daemon, client, _events, pane) = pane(home.path()).await;
    let one = "E[<0;2;1ME[<0;2;1m".len();
    echo_raw_input(&client, &pane, "\\033[?1000h\\033[?1006h", one * 20).await;
    for i in 0..40u32 {
        let action = if i % 2 == 0 {
            v1::MouseAction::Press
        } else {
            v1::MouseAction::Release
        };
        client
            .try_notify(v1::notification::Body::Input(v1::Input {
                pane_id: pane.clone(),
                kind: Some(v1::input::Kind::Mouse(v1::MouseInput {
                    button: v1::MouseButton::Left.into(),
                    action: action.into(),
                    col: 1,
                    row: 0,
                    modifiers: 0,
                })),
            }))
            .unwrap();
    }
    wait_for_text(&client, &pane, &"E[<0;2;1ME[<0;2;1m".repeat(20)).await;
    daemon.stop().await;
}
