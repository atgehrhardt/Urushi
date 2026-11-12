//! The writer task: oversized frames and failed writes.

use tokio::io::AsyncWriteExt;
use urushi_proto::codec::{MAX_FRAME_LEN, read_envelope};
use urushi_proto::v1::{self, envelope::Kind};

use super::*;

/// A response far over the frame limit.
fn huge_response(id: u64) -> Envelope {
    envelope::response(
        id,
        v1::response::Outcome::ScreenText(v1::ScreenText {
            lines: vec!["x".repeat(MAX_FRAME_LEN + 1)],
        }),
    )
}

#[tokio::test]
async fn an_oversized_response_becomes_an_error_and_the_connection_keeps_working() {
    let (daemon, mut client) = tokio::io::duplex(64 * 1024);
    let (out, outbound) = mpsc::channel(8);
    let writer = tokio::spawn(write_frames(daemon, outbound, "test".into()));

    out.send(huge_response(7)).await.unwrap();
    out.send(envelope::response(
        8,
        v1::response::Outcome::Ok(v1::Empty {}),
    ))
    .await
    .unwrap();

    let first = read_envelope(&mut client).await.unwrap().unwrap();
    let Some(Kind::Response(v1::Response {
        id: 7,
        outcome: Some(v1::response::Outcome::Error(error)),
    })) = first.kind
    else {
        panic!("expected an error for request 7, got {first:?}");
    };
    assert_eq!(error.code, v1::ErrorCode::Internal as i32);
    let second = read_envelope(&mut client).await.unwrap().unwrap();
    assert!(matches!(
        second.kind,
        Some(Kind::Response(v1::Response { id: 8, .. }))
    ));
    assert!(!writer.is_finished());
}

#[tokio::test]
async fn an_oversized_event_is_dropped_and_later_frames_still_go_out() {
    let (daemon, mut client) = tokio::io::duplex(64 * 1024);
    let (out, outbound) = mpsc::channel(8);
    let _writer = tokio::spawn(write_frames(daemon, outbound, "test".into()));

    let mut workspace = v1::Workspace::default();
    workspace.boards.push(v1::Board {
        name: "x".repeat(MAX_FRAME_LEN + 1),
        ..Default::default()
    });
    out.send(envelope::event(v1::event::Body::WorkspaceChanged(
        workspace,
    )))
    .await
    .unwrap();
    out.send(envelope::response(
        1,
        v1::response::Outcome::Ok(v1::Empty {}),
    ))
    .await
    .unwrap();
    let next = read_envelope(&mut client).await.unwrap().unwrap();
    assert!(matches!(
        next.kind,
        Some(Kind::Response(v1::Response { id: 1, .. }))
    ));
}

#[tokio::test]
async fn a_failed_write_ends_the_writer() {
    let (daemon, mut client) = tokio::io::duplex(64);
    let (out, outbound) = mpsc::channel(8);
    let writer = tokio::spawn(write_frames(daemon, outbound, "test".into()));
    client.shutdown().await.unwrap();
    drop(client);
    out.send(envelope::response(
        1,
        v1::response::Outcome::Ok(v1::Empty {}),
    ))
    .await
    .unwrap();
    // The writer ends although `out` is still open: `serve` sees it and hangs up.
    tokio::time::timeout(std::time::Duration::from_secs(5), writer)
        .await
        .expect("the writer should stop after a failed write")
        .unwrap();
}
