//! Creating a thread without waiting on screen (UR-68, ADR 0055): the dialog closes on
//! submit, a placeholder shows until the daemon answers, and the base is fetched ahead.

use std::collections::HashMap;

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::RepoId;
use urushi_ui::{Frame, Point, Rect, TextSystem, Theme, build};

use super::{attached, bodies, sent};
use crate::msg::{Effect, Followup, Msg};
use crate::state::{AppState, Dialog};

fn repo() -> String {
    RepoId::from_u128(1).to_string()
}

fn render(s: &AppState) -> Frame<Msg> {
    let mut text = TextSystem::bundled_only();
    build(
        &crate::view::view(s, &Theme::dark(), &HashMap::new()),
        Rect::new(0.0, 0.0, 1200.0, 800.0),
        &mut text,
        None::<Point>,
    )
}

/// Opens New Thread, types `title`, and submits. Returns the create's key.
fn submit(s: &mut AppState, title: &str) -> u64 {
    s.update(Msg::OpenNewThread {
        repo: Some(repo()),
        project: None,
    });
    s.update(Msg::Paste(title.into()));
    let fx = s.update(Msg::Submit);
    let keys: Vec<u64> = fx
        .iter()
        .filter_map(|e| match e {
            Effect::Request {
                body: Body::CreateThread(_),
                followup: Followup::ThreadCreated(key),
            } => Some(*key),
            _ => None,
        })
        .collect();
    assert_eq!(keys.len(), 1, "{fx:?}");
    keys[0]
}

fn created(s: &mut AppState, key: u64, id: &str) -> Vec<Effect> {
    s.update(Msg::Response {
        followup: Followup::ThreadCreated(key),
        result: Ok(Outcome::Thread(v1::Thread {
            id: id.into(),
            ..Default::default()
        })),
    })
}

#[test]
fn submitting_closes_the_dialog_and_shows_a_placeholder_at_once() {
    let mut s = attached();
    let key = submit(&mut s, "Speed up");
    assert!(s.dialog.is_none());
    assert!(s.shows_pending(key));
    assert_eq!(s.selected_thread, None);
    let dump = render(&s).scene.dump();
    assert!(dump.contains("Creating \u{201c}Speed up\u{201d}"), "{dump}");
    assert!(
        dump.contains("\"creating\""),
        "a row in the sidebar: {dump}"
    );
    // Submitting again does nothing: the dialog is gone.
    assert!(sent(&s.update(Msg::Submit)).is_empty());

    created(&mut s, key, "new");
    assert_eq!(s.selected_thread.as_deref(), Some("new"));
    assert!(s.creations.is_empty());
}

#[test]
fn a_thread_created_after_the_user_moved_on_is_not_forced_on_screen() {
    let mut s = attached();
    let key = submit(&mut s, "Later");
    let elsewhere = super::workspace().threads[0].id.clone();
    s.update(Msg::SelectThread(elsewhere.clone()));
    assert!(!s.shows_pending(key));
    created(&mut s, key, "new");
    assert_eq!(s.selected_thread, Some(elsewhere));
    assert!(s.creations.is_empty());
}

#[test]
fn the_answer_never_closes_a_dialog_opened_meanwhile() {
    let mut s = attached();
    let key = submit(&mut s, "One");
    s.update(Msg::OpenAddProject);
    created(&mut s, key, "new");
    assert!(matches!(s.dialog, Some(Dialog::AddProject { .. })));
}

#[test]
fn a_failure_brings_the_dialog_back_as_submitted() {
    let mut s = attached();
    let key = submit(&mut s, "Doomed");
    s.update(Msg::Response {
        followup: Followup::ThreadCreated(key),
        result: Err("branch urushi/doomed already exists".into()),
    });
    assert!(s.creations.is_empty());
    assert_eq!(s.selected_pending, None);
    assert_eq!(
        s.error.as_deref(),
        Some("branch urushi/doomed already exists")
    );
    match &s.dialog {
        Some(Dialog::NewThread { title, .. }) => assert_eq!(title.text(), "Doomed"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn several_creates_can_run_at_once() {
    let mut s = attached();
    let a = submit(&mut s, "A");
    let b = submit(&mut s, "B");
    assert_ne!(a, b);
    assert!(s.shows_pending(b));
    // Clicking the first one's row shows it.
    s.update(Msg::SelectPending(a));
    assert!(s.shows_pending(a));
    created(&mut s, b, "b");
    assert!(s.shows_pending(a), "B's answer leaves A on screen");
    created(&mut s, a, "a");
    assert_eq!(s.selected_thread.as_deref(), Some("a"));
}

#[test]
fn the_base_is_fetched_ahead_only_by_daemons_that_can() {
    let prepares = |fx: &[Effect]| -> Vec<v1::PrepareThreadBaseRequest> {
        bodies(fx)
            .into_iter()
            .filter_map(|b| match b {
                Body::PrepareThreadBase(r) => Some(r.clone()),
                _ => None,
            })
            .collect()
    };
    let mut s = attached();
    let open = Msg::OpenNewThread {
        repo: Some(repo()),
        project: None,
    };
    assert!(prepares(&s.update(open.clone())).is_empty(), "1.30");

    s.dialog = None;
    s.daemon_minor = urushi_client::PREPARE_THREAD_BASE_SINCE_MINOR;
    let fx = s.update(open);
    assert!(
        matches!(&prepares(&fx)[..], [r] if r.project_id == repo() && r.base_ref.is_empty()),
        "{fx:?}"
    );
    let fx = s.update(Msg::PickBase(Some("dev".into())));
    assert!(matches!(&prepares(&fx)[..], [r] if r.base_ref == "dev"));
    let fx = s.update(Msg::PickProject(Some("f1".into())));
    assert!(matches!(&prepares(&fx)[..], [r] if r.folder_id == "f1" && r.base_ref == "dev"));

    // An error answer (an older daemon after all) shows nothing.
    s.update(Msg::Response {
        followup: Followup::Ignore,
        result: Err("unknown request".into()),
    });
    assert_eq!(s.error, None);
}
