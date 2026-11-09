//! End-to-end: files attached to tasks (ADR 0016): added, fetched whole and as
//! thumbnails, refused when too big or a broken image, deleted with their task, and
//! swept when the daemon starts (protocol 1.8).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::io::Cursor;

use common::*;
use urushi_client::{ClientError, WorkspaceMirror, v1};

fn code(err: &ClientError) -> Option<v1::ErrorCode> {
    match err {
        ClientError::Remote { code, .. } => Some(*code),
        _ => None,
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 120, 200, 255]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn files(home: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(home.join("attachments"))
        .map(|d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread")]
async fn attachments_are_stored_fetched_and_deleted_with_their_task() {
    let home = temp_home();
    let daemon_home = home.path().join("home");
    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;
    let board = client.create_board("Sprint").await.unwrap();
    let task = client
        .create_task(&board.id, &board.columns[0].id, "Design", "")
        .await
        .unwrap();
    let step = client.create_subtask(&task.id, "Mockups").await.unwrap();

    let image = client
        .add_attachment(&step.id, "/Users/me/Desktop/mock up.png", png(800, 400))
        .await
        .unwrap();
    assert_eq!(image.name, "mock up.png", "directories are dropped");
    assert_eq!(image.media_type, "image/png");
    assert_eq!((image.width, image.height), (800, 400));
    let text = client
        .add_attachment(&task.id, "notes.txt", b"hello".to_vec())
        .await
        .unwrap();
    assert_eq!((text.media_type.as_str(), text.size), ("text/plain", 5));

    // The whole file, and a thumbnail as raw RGBA that fits 320 pixels.
    let file = client.get_attachment(&image.id, false).await.unwrap();
    assert_eq!(file.data, png(800, 400));
    let thumb = client.get_attachment(&image.id, true).await.unwrap();
    assert_eq!((thumb.width, thumb.height), (320, 160));
    assert_eq!(thumb.data.len(), 320 * 160 * 4);
    assert_eq!(&thumb.data[..4], &[10, 120, 200, 255]);
    let err = client.get_attachment(&text.id, true).await.unwrap_err();
    assert_eq!(
        code(&err),
        Some(v1::ErrorCode::FailedPrecondition),
        "{err:?}"
    );

    // Refused: too big, a broken image, an unknown task.
    let big = vec![b'x'; urushi_core::board::MAX_ATTACHMENT_BYTES + 1];
    let err = client
        .add_attachment(&task.id, "big.txt", big)
        .await
        .unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::InvalidArgument), "{err:?}");
    let err = client
        .add_attachment(&task.id, "bad.png", b"\x89PNG\r\n\x1a\nbroken".to_vec())
        .await
        .unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::InvalidArgument), "{err:?}");
    let err = client
        .add_attachment(
            "00000000-0000-0000-0000-000000000009",
            "a.txt",
            b"a".to_vec(),
        )
        .await
        .unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::NotFound), "{err:?}");
    assert_eq!(
        files(&daemon_home),
        [
            text.id.clone(),
            image.id.clone(),
            format!("{}.thumb", image.id)
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>(),
        "refused files leave nothing behind"
    );

    // The board lists them; deleting one, then the task, removes the files.
    let w = WorkspaceMirror::new(client.list_workspace(false).await.unwrap());
    let found = w.find_task(&task.id).unwrap();
    assert_eq!(found.card.attachments[0].name, "notes.txt");
    assert_eq!(found.card.subtasks[0].attachments[0].id, image.id);
    client.delete_attachment(&text.id).await.unwrap();
    assert_eq!(files(&daemon_home).len(), 2);
    daemon.stop().await;

    // A file nobody names (e.g. left by a crash) is swept at start; named ones stay.
    std::fs::write(
        daemon_home.join("attachments/00000000-0000-0000-0000-0000000000aa"),
        "stray",
    )
    .unwrap();
    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;
    assert_eq!(files(&daemon_home).len(), 2);
    assert_eq!(
        client
            .get_attachment(&image.id, false)
            .await
            .unwrap()
            .data
            .len(),
        png(800, 400).len()
    );
    client.delete_task(&task.id).await.unwrap();
    assert!(files(&daemon_home).is_empty(), "{:?}", files(&daemon_home));
    daemon.stop().await;
}
