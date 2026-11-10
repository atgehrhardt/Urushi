//! The quick-filter scenarios (protocol 1.21): a board of Urushi's own with quick
//! filters as chips, one on, and a selection that can be saved (`board-quick-filters`);
//! saving a Jira board's selection, with the JQL built from it
//! (`quick-filter-save`); and a quick filter chip's menu (`quick-filter-menu`).

use urushi_client::v1;

use crate::filter::Dim;
use crate::filter::quick::QuickRef;
use crate::msg::{FilterMsg, Msg};
use crate::organize::MenuTarget;
use crate::state::AppState;

/// The quick-filter scenarios.
pub(super) const SCENARIOS: &[&str] = &[
    "board-quick-filters",
    "quick-filter-save",
    "quick-filter-menu",
];

/// Sets up a quick-filter scenario on a connected state.
pub(super) fn show(state: &mut AppState, name: &str) {
    if name == "board-quick-filters" {
        local(state);
        return;
    }
    super::jira::show(state, "jira-board");
    if name == "quick-filter-save" {
        state.update(Msg::Filter(FilterMsg::Toggle(Dim::Type, "Bug".into())));
        state.update(Msg::Filter(FilterMsg::Mine));
        state.update(Msg::Filter(FilterMsg::OpenSave));
        state.update(Msg::Paste("My bugs".into()));
    } else {
        state.update(Msg::Pointer(urushi_ui::Point::new(760.0, 100.0)));
        state.update(Msg::OpenMenu(MenuTarget::QuickFilter(
            "jb".into(),
            QuickRef::Jira(1),
        )));
    }
}

/// The sample board, keeping two quick filters ("Legal" on), with a tag chosen.
fn local(state: &mut AppState) {
    let mut workspace = state.workspace.raw().clone();
    if let Some(b) = workspace.boards.iter_mut().find(|b| b.id == "b1") {
        b.quick_filters = vec![
            v1::QuickFilter {
                id: "q1".into(),
                name: "Legal".into(),
                rule: Some(v1::QuickFilterRule {
                    tags: vec!["legal".into()],
                    ..Default::default()
                }),
            },
            v1::QuickFilter {
                id: "q2".into(),
                name: "Webhooks".into(),
                rule: Some(v1::QuickFilterRule {
                    search: "webhook".into(),
                    ..Default::default()
                }),
            },
        ];
    }
    state.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    state.update(Msg::ToggleBoard);
    state.update(Msg::BoardReveal(1.0));
    state.update(Msg::SelectBoard("b1".into()));
    state.update(Msg::Filter(FilterMsg::Saved("q1".into())));
    state.update(Msg::Filter(FilterMsg::Toggle(Dim::Label, "copy".into())));
}

#[cfg(test)]
mod tests {
    use super::super::*;

    fn dump(name: &str, width: f32) -> String {
        let theme = Theme::dark();
        let mut text = TextSystem::bundled_only();
        scenario(name, &theme, &mut text, width, 820.0)
            .unwrap()
            .dump()
    }

    #[test]
    fn a_boards_own_quick_filters_are_chips_and_the_selection_can_be_saved() {
        let board = dump("board-quick-filters", 1280.0);
        for text in [
            "\"Legal\"",
            "\"Webhooks\"",
            "\"Save filter…\"",
            "\"Filters (2)  ▾\"",
        ] {
            assert!(board.contains(text), "{text} missing: {board}");
        }
        // "Legal" and "copy" both: only the pricing card has both tags.
        assert!(board.contains("\"Pricing page copy\""), "{board}");
        assert!(!board.contains("\"Invoice PDF export\""), "{board}");
        // A phone has no room for chips; the menu lists them.
        let phone = dump("board-quick-filters", 402.0);
        assert!(!phone.contains("\"Webhooks\""), "{phone}");
    }

    #[test]
    fn saving_a_jira_selection_shows_its_jql() {
        let save = dump("quick-filter-save", 1280.0);
        for text in [
            "\"Save quick filter\"",
            "\"My bugs\"",
            "\"JQL\"",
            "assignee = currentUser() AND issuetype in (\\\"Bug\\\")",
            "\"Saved to the board in Jira, which evaluates it.\"",
        ] {
            assert!(save.contains(text), "{text} missing: {save}");
        }
        let menu = dump("quick-filter-menu", 1280.0);
        for text in ["\"Edit…\"", "\"Delete…\""] {
            assert!(menu.contains(text), "{text} missing: {menu}");
        }
    }
}
