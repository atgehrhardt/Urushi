//! The threads-bar scenarios (UR-62, ADR 0048): "Billing launch" colored teal and
//! "Onboarding" purple, with "Invoice API" unread and orange Markdown notes on "Pricing
//! page" (`thread-extras`), the notes editor for "Pricing page" (`thread-notes`), the
//! same with words selected to format (`thread-notes-format`, ADR 0058), and the project
//! menu's Color page (`project-color-menu`), and "Pricing page" shelved and open
//! (`shelved-thread`, ADR 0060).

use urushi_client::v1;
use urushi_ui::Point;

use crate::msg::{Msg, ThreadMsg};
use crate::organize::{MenuPage, MenuTarget};
use crate::state::{AppState, Dialog};

/// The threads-bar scenarios.
pub(super) const SCENARIOS: &[&str] = &[
    "thread-extras",
    "thread-notes",
    "thread-notes-format",
    "project-color-menu",
    "shelved-thread",
];

/// "Pricing page"'s notes.
const NOTES: &str = "**Waiting** on the _copy review_ from marketing.\n\n## Then\n- [ ] Ship behind the `pricing_v2` flag\n- [x] Draft the ~~old~~ new copy\n\nSee [the brief](https://example.com/brief).";

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
                thread.notes = NOTES.into();
                thread.notes_color = v1::LabelColor::Orange.into();
            }
            _ => {}
        }
    }
    state.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    match name {
        "thread-notes" => drop(state.update(Msg::Thread(ThreadMsg::OpenNotes("t3".into())))),
        "thread-notes-format" => {
            state.update(Msg::Thread(ThreadMsg::OpenNotes("t3".into())));
            if let Some(Dialog::ThreadNotes { notes, .. }) = &mut state.dialog {
                let start = NOTES.find("from marketing").unwrap_or(0);
                notes.select(start, start + "from marketing".len());
            }
        }
        "shelved-thread" => {
            let mut workspace = state.workspace.raw().clone();
            if let Some(t) = workspace.threads.iter_mut().find(|t| t.id == "t3") {
                t.shelved = true;
            }
            state.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
            state.update(Msg::SelectThread("t3".into()));
        }
        "project-color-menu" => {
            state.update(Msg::Pointer(Point::new(150.0, 110.0)));
            state.update(Msg::OpenMenu(MenuTarget::Project("f1".into())));
            state.update(Msg::MenuPage(MenuPage::Color));
        }
        _ => {}
    }
}
