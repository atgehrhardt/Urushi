//! A dialog saving (UR-68, ADR 0055): "Saving…" and no second submit until the daemon
//! answers; the answer closes only a dialog of the kind that was saved.

use std::collections::HashMap;

use urushi_client::v1::{self, response::Outcome};
use urushi_core::ThreadId;
use urushi_ui::{Point, Rect, TextSystem, Theme, build};

use super::{attached, bodies};
use crate::msg::{Followup, Msg};
use crate::organize::Prompt;
use crate::state::{AppState, Dialog};

fn renaming() -> AppState {
    let mut s = attached();
    s.update(Msg::OpenPrompt(Prompt::RenameThread(
        ThreadId::from_u128(10).to_string(),
    )));
    s.update(Msg::Paste(" 2".into()));
    s
}

fn dump(s: &AppState) -> String {
    let mut text = TextSystem::bundled_only();
    build(
        &crate::view::view(s, &Theme::dark(), &HashMap::new()),
        Rect::new(0.0, 0.0, 1200.0, 800.0),
        &mut text,
        None::<Point>,
    )
    .scene
    .dump()
}

fn answer(s: &mut AppState, result: Result<Outcome, String>) {
    s.update(Msg::Response {
        followup: Followup::CloseDialog,
        result,
    });
}

#[test]
fn a_saving_dialog_says_so_and_cannot_be_sent_twice() {
    let mut s = renaming();
    assert_eq!(bodies(&s.update(Msg::Submit)).len(), 1);
    assert!(s.dialog_saving());
    assert!(dump(&s).contains("Saving\u{2026}"));
    assert!(s.update(Msg::Submit).is_empty(), "no second rename");
    answer(&mut s, Ok(Outcome::Ok(v1::Empty {})));
    assert!(s.dialog.is_none());
    assert!(!s.dialog_saving());
}

#[test]
fn a_refused_save_can_be_tried_again() {
    let mut s = renaming();
    s.update(Msg::Submit);
    answer(&mut s, Err("daemon unavailable".into()));
    assert!(matches!(s.dialog, Some(Dialog::Prompt { .. })));
    assert!(!s.dialog_saving());
    assert_eq!(bodies(&s.update(Msg::Submit)).len(), 1);
}

#[test]
fn the_answer_leaves_a_different_dialog_open() {
    let mut s = renaming();
    s.update(Msg::Submit);
    s.update(Msg::CloseDialog);
    s.update(Msg::OpenAddProject);
    answer(&mut s, Ok(Outcome::Ok(v1::Empty {})));
    assert!(matches!(s.dialog, Some(Dialog::AddProject { .. })));
}
