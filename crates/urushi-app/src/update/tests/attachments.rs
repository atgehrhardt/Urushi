//! Attachments: choosing and dropping files onto a task, opening and deleting them, and
//! thumbnails asked for once as they come into view.

use urushi_client::v1::{self, request::Body, response::Outcome};

use super::board::board;
use super::{attached, bodies};
use crate::msg::{BoardMsg, Effect, Followup, Msg};
use crate::state::AppState;

fn attachment(id: &str, name: &str, image: bool) -> v1::Attachment {
    v1::Attachment {
        id: id.into(),
        name: name.into(),
        media_type: if image { "image/png" } else { "text/plain" }.into(),
        size: 10,
        width: if image { 64 } else { 0 },
        height: if image { 32 } else { 0 },
        ..Default::default()
    }
}

/// A 1.8 daemon; card "a" has an image and a text file, card "b" an image.
fn with_attachments() -> AppState {
    let mut s = attached();
    s.daemon_minor = 8;
    let mut w = super::workspace();
    let mut b = board();
    b.columns[0].tasks[0].attachments = vec![
        attachment("f1", "notes.txt", false),
        attachment("i1", "a.png", true),
    ];
    b.columns[0].tasks[1].attachments = vec![attachment("i2", "b.png", true)];
    w.boards = vec![b];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    s
}

fn thumbnails_asked(fx: &[Effect]) -> Vec<String> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Request {
                body:
                    Body::GetAttachment(v1::GetAttachmentRequest {
                        attachment_id,
                        thumbnail: true,
                    }),
                followup: Followup::Thumbnail(id),
            } if attachment_id == id => Some(id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn thumbnails_are_asked_for_once_when_the_board_shows_them() {
    let mut s = with_attachments();
    let fx = s.update(Msg::ToggleBoard);
    assert_eq!(
        thumbnails_asked(&fx),
        ["i1", "i2"],
        "each card's first image"
    );
    assert!(
        thumbnails_asked(&s.update(Msg::BoardReveal(1.0))).is_empty(),
        "not twice"
    );
    s.update(Msg::Response {
        followup: Followup::Thumbnail("i1".into()),
        result: Ok(Outcome::AttachmentData(v1::AttachmentData {
            attachment_id: "i1".into(),
            media_type: "image/x-rgba".into(),
            data: vec![255; 8],
            width: 2,
            height: 1,
        })),
    });
    let image = s.board.thumbnails.get("i1").expect("kept");
    assert_eq!((image.width, image.height), (2, 1));
    // A failed one isn't asked for again, and shows no error.
    s.update(Msg::Response {
        followup: Followup::Thumbnail("i2".into()),
        result: Err("gone".into()),
    });
    assert!(s.error.is_none());
    assert!(thumbnails_asked(&s.update(Msg::BoardReveal(1.0))).is_empty());
}

#[test]
fn files_are_chosen_or_dropped_onto_an_open_task() {
    let mut s = with_attachments();
    s.update(Msg::ToggleBoard);
    s.update(Msg::PressTask("a".into()));
    s.update(Msg::PressEnded);
    let fx = s.update(Msg::Board(BoardMsg::AddFiles("a".into())));
    assert!(matches!(&fx[..], [Effect::PickFiles { task }] if task == "a"));
    let fx = s.update(Msg::FileDropped {
        path: "/tmp/shot.png".into(),
        is_dir: false,
        over_terminal: true,
    });
    assert!(
        matches!(
            &fx[..],
            [Effect::ReadFiles { task, paths }] if task == "a" && paths == &["/tmp/shot.png"]
        ),
        "dropped on the editor, not pasted into the terminal: {fx:?}"
    );
    let fx = s.update(Msg::Board(BoardMsg::Attach {
        task: "a".into(),
        name: "shot.png".into(),
        bytes: vec![1, 2, 3],
    }));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::AddAttachment(v1::AddAttachmentRequest { task_id, name, data })]
            if task_id == "a" && name == "shot.png" && data == &[1, 2, 3]
    ));
    s.update(Msg::Board(BoardMsg::AttachFailed(
        "big.bin is too big".into(),
    )));
    assert_eq!(s.error.as_deref(), Some("big.bin is too big"));
}

#[test]
fn attachments_open_with_the_systems_app_and_are_deleted() {
    let mut s = with_attachments();
    let fx = s.update(Msg::Board(BoardMsg::OpenAttachment("f1".into())));
    let [
        Effect::Request {
            body: Body::GetAttachment(r),
            followup,
        },
    ] = &fx[..]
    else {
        panic!("{fx:?}")
    };
    assert!(!r.thumbnail);
    let fx = s.update(Msg::Response {
        followup: followup.clone(),
        result: Ok(Outcome::AttachmentData(v1::AttachmentData {
            attachment_id: "f1".into(),
            media_type: "text/plain".into(),
            data: b"hi".to_vec(),
            ..Default::default()
        })),
    });
    assert!(
        matches!(
            &fx[..],
            [Effect::OpenFile { id, name, bytes }] if id == "f1" && name == "notes.txt" && bytes == b"hi"
        ),
        "{fx:?}"
    );
    let fx = s.update(Msg::Board(BoardMsg::DeleteAttachment("f1".into())));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::DeleteAttachment(v1::DeleteAttachmentRequest { attachment_id })] if attachment_id == "f1"
    ));
}
