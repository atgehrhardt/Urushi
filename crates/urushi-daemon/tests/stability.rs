//! Stability under load (UR-69): a lost screen ack. (Many panes: `many_panes.rs`.)
//! Unix only, like `e2e.rs`: panes run `/bin/sh`.
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
use urushi_client::v1;

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_ack_only_delays_the_next_update() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, mut events) = daemon.connect().await;
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
    let first = client.attach_pane(&pane, 80, 24).await.unwrap();
    assert_eq!(first.seq, 1);

    // The client never acks update 1 (as if the ack was dropped). The pane's output
    // still arrives, after the ack timeout instead of never.
    type_line(&client, &pane, "echo after-$((6 * 7))").await;
    let started = tokio::time::Instant::now();
    let deadline = started + TIMEOUT + Duration::from_secs(2);
    let mut seen = false;
    while !seen {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let event = tokio::time::timeout(remaining, events.recv())
            .await
            .expect("an update should arrive without the ack")
            .expect("connection open");
        if let v1::event::Body::ScreenUpdate(update) = event
            && update.pane_id == pane
        {
            assert!(update.seq >= 2);
            seen = update
                .lines
                .iter()
                .flat_map(|l| l.spans.iter())
                .any(|s| s.text.contains("after-42"));
        }
    }
    daemon.stop().await;
}

/// A client that sends requests without reading the replies can't make the daemon hold
/// an unbounded number of them: past the cap, requests are refused at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_that_never_reads_replies_is_refused_past_the_cap() {
    use urushi_proto::codec::{read_envelope, write_envelope};
    use urushi_proto::endpoint::LocalEndpoint;
    use urushi_proto::{envelope, v1::envelope::Kind, v1::request::Body, v1::response::Outcome};

    const REQUESTS: u64 = 20_000;
    let home = temp_home();
    let daemon = Daemon::start(&home.path().join("home")).await;
    let urushi_client::Endpoint::Local(LocalEndpoint::UnixSocket(path)) = daemon.endpoint() else {
        panic!("expected a Unix socket");
    };
    let stream = tokio::net::UnixStream::connect(path).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();
    write_envelope(&mut writer, &envelope::hello(0, "flood", "0"))
        .await
        .unwrap();
    read_envelope(&mut reader).await.unwrap().unwrap();

    let flood = tokio::spawn(async move {
        for id in 1..=REQUESTS {
            let body = Body::GetSettings(v1::GetSettingsRequest {});
            write_envelope(&mut writer, &envelope::request(id, body))
                .await
                .unwrap();
        }
        writer
    });
    // Let the replies back up while nothing reads them.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (mut answered, mut refused) = (0, 0);
    while answered + refused < REQUESTS {
        let frame = tokio::time::timeout(Duration::from_secs(30), read_envelope(&mut reader))
            .await
            .expect("a reply in time")
            .unwrap()
            .unwrap();
        match frame.kind {
            Some(Kind::Response(v1::Response {
                outcome: Some(Outcome::Settings(_)),
                ..
            })) => answered += 1,
            Some(Kind::Response(v1::Response {
                outcome: Some(Outcome::Error(error)),
                ..
            })) => {
                assert_eq!(error.code, v1::ErrorCode::Unavailable as i32, "{error:?}");
                refused += 1;
            }
            _ => {}
        }
    }
    let _writer = flood.await.unwrap();
    assert!(refused > 0, "every one of {answered} requests was held");
    assert!(answered > 0);
    daemon.stop().await;
}
