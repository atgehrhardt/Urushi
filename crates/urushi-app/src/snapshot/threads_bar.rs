//! The threads-bar scenarios (UR-62, ADR 0048): "Billing launch" colored teal and
//! "Onboarding" purple, with "Invoice API" unread and notes on "Pricing page"
//! (`thread-extras`), the notes editor for "Pricing page" (`thread-notes`), and the
//! project menu's Color page (`project-color-menu`).

use urushi_client::v1;
use urushi_ui::Point;

use crate::msg::{Msg, ThreadMsg};
use crate::organize::{MenuPage, MenuTarget};
use crate::state::AppState;

/// The threads-bar scenarios.
pub(super) const SCENARIOS: &[&str] = &["thread-extras", "thread-notes", "project-color-menu"];

pub(super) fn show(state: &mut AppState, name: &str) {
    let mut workspace = state.workspace.raw().clone();
    for (id, color) in [("f1", v1::LabelColor::Teal), ("f2", v1::LabelColor::Purple)] {
        if let Some(folder) = workspace.folders.iter_mut().find(|f| f.id == id) {
            folder.color = color.into();
        }
    }
    for thread in &mut workspace.threads {
        match thread.id.as_str() {
            "t2" => thread.unread = true,
            "t3" => {
                thread.notes =
                    "Waiting on the copy review from marketing.\nThen ship behind the flag.".into();
            }
            _ => {}
        }
    }
    state.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    match name {
        "thread-notes" => drop(state.update(Msg::Thread(ThreadMsg::OpenNotes("t3".into())))),
        "project-color-menu" => {
            state.update(Msg::Pointer(Point::new(150.0, 110.0)));
            state.update(Msg::OpenMenu(MenuTarget::Project("f1".into())));
            state.update(Msg::MenuPage(MenuPage::Color));
        }
        _ => {}
    }
}
