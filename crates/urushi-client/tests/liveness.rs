//! A connection either works both ways or ends (ADR 0052): a daemon that goes silent,
//! a client that is dropped, and an older daemon that doesn't know `Ping`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

use std::time::Duration;

use tokio::io::{DuplexStream, ReadHalf, WriteHalf};
use urushi_client::{Client, ClientError, LIVENESS_TIMEOUT, PING_INTERVAL, v1};
use urushi_proto::codec::{read_envelope, write_envelope};
use urushi_proto::envelope;
use urushi_proto::v1::envelope::Kind;

/// The daemon's end of a connection, after it welcomed the client.
struct FakeDaemon {
    reader: ReadHalf<DuplexStream>,
    writer: WriteHalf<DuplexStream>,
}

/// A client connected to a fake daemon that has answered `Hello`.
async fn connected() -> (Client, urushi_client::Events, FakeDaemon) {
    let (client_end, daemon_end) = tokio::io::duplex(64 * 1024);
    let (mut reader, mut writer) = tokio::io::split(daemon_end);
    let daemon = tokio::spawn(async move {
        let hello = read_envelope(&mut reader).await.unwrap().unwrap();
        let Some(Kind::Request(v1::Request { id, .. })) = hello.kind else {
            panic!("expected Hello");
        };
        let welcome = v1::Welcome {
            protocol_major: urushi_proto::PROTOCOL_MAJOR,
            protocol_minor: urushi_proto::PROTOCOL_MINOR,
            daemon_version: "test".into(),
            host_name: "test".into(),
        };
        write_envelope(
            &mut writer,
            &envelope::response(id, v1::response::Outcome::Welcome(welcome)),
        )
        .await
        .unwrap();
        FakeDaemon { reader, writer }
    });
    let (client, events) = Client::over(client_end, "test").await.unwrap();
    (client, events, daemon.await.unwrap())
}

#[tokio::test(start_paused = true)]
async fn a_daemon_that_goes_silent_ends_the_connection() {
    let (client, mut events, mut daemon) = connected().await;
    // The daemon reads (so pings arrive) but never answers anything.
    tokio::spawn(async move { while let Ok(Some(_)) = read_envelope(&mut daemon.reader).await {} });
    let started = tokio::time::Instant::now();

    let request = tokio::spawn({
        let client = client.clone();
        async move { client.list_workspace(false).await }
    });
    assert!(events.recv().await.is_none(), "the event stream ends");
    let waited = started.elapsed();
    assert!(waited >= LIVENESS_TIMEOUT, "ended too early: {waited:?}");
    assert!(
        waited <= LIVENESS_TIMEOUT + PING_INTERVAL * 2,
        "ended too late: {waited:?}"
    );
    assert!(!client.is_connected());
    assert!(matches!(
        request.await.unwrap(),
        Err(ClientError::Disconnected)
    ));
}

#[tokio::test(start_paused = true)]
async fn an_older_daemon_answering_pings_with_errors_stays_connected() {
    let (client, mut events, mut daemon) = connected().await;
    let pings = tokio::spawn(async move {
        let mut pings = 0;
        while let Ok(Some(frame)) = read_envelope(&mut daemon.reader).await {
            if let Some(Kind::Request(v1::Request { id, body })) = frame.kind {
                // What a daemon before 1.30 does with a body it doesn't know.
                assert!(matches!(body, Some(v1::request::Body::Ping(_))));
                pings += 1;
                let error = envelope::error(id, v1::ErrorCode::InvalidArgument, "empty request");
                write_envelope(&mut daemon.writer, &error).await.unwrap();
            }
        }
        pings
    });
    let quiet = tokio::time::timeout(LIVENESS_TIMEOUT * 4, events.recv()).await;
    assert!(quiet.is_err(), "the connection must stay up: {quiet:?}");
    assert!(client.is_connected());
    drop((client, events));
    assert!(pings.await.unwrap() >= 4, "the client pinged while quiet");
}

#[tokio::test]
async fn dropping_every_client_closes_the_socket() {
    let (client, events, mut daemon) = connected().await;
    let copy = client.clone();
    drop(client);
    drop(copy);
    // `events` is still held: the socket closes all the same, so a reconnecting
    // application never leaves its old connection open on the daemon.
    let end = tokio::time::timeout(Duration::from_secs(5), read_envelope(&mut daemon.reader))
        .await
        .expect("the daemon should see the connection close");
    assert!(matches!(end, Ok(None)));
    drop(events);
}

#[tokio::test(start_paused = true)]
async fn a_daemon_that_never_says_welcome_times_out() {
    let (client_end, _daemon_end) = tokio::io::duplex(1024);
    let result = Client::over(client_end, "test").await;
    assert!(
        matches!(result, Err(ClientError::TimedOut(_))),
        "{:?}",
        result.err()
    );
}
