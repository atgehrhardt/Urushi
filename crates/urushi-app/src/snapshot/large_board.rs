//! A large Jira board (UR-66): `jira-large-board`, 500 issues with long notes, tags,
//! epics and many assignees, and `jira-large-filter-menu`, its Filters menu with more
//! quick filters than a window holds, so it scrolls. The checks show that only the
//! cards in sight are built and that drawing it again shapes no new text.

use urushi_client::v1;

use super::jira::{account, board, issue};
use crate::msg::Msg;
use crate::organize::MenuTarget;
use crate::state::AppState;

/// The large-board scenarios.
pub(super) const SCENARIOS: &[&str] = &["jira-large-board", "jira-large-filter-menu"];

/// Issues on the large board.
pub(super) const ISSUES: usize = 500;

/// The ACME board with [`ISSUES`] issues spread over its columns, and 40 quick
/// filters.
fn large_board() -> v1::Board {
    let mut b = board();
    for c in &mut b.columns {
        c.tasks.clear();
    }
    let notes = "Customers asked for this in the last review. ".repeat(40);
    let columns = b.columns.len();
    for n in 0..ISSUES {
        let kind = if n % 7 == 0 {
            v1::TaskKind::Bug
        } else {
            v1::TaskKind::Story
        };
        let mut t = issue(
            &format!("ACME-{}", 1000 + n),
            (kind, "To Do"),
            &format!("Person {}", n % 23),
            if n % 2 == 0 { "ACME-7" } else { "ACME-8" },
        );
        t.title = format!("Issue number {n} with a title long enough to be cut short");
        t.notes.clone_from(&notes);
        t.tags = vec![format!("area-{}", n % 11), "customer".into()];
        b.columns[n % columns].tasks.push(t);
    }
    if let Some(jira) = b.jira.as_mut() {
        jira.quick_filters = (1..=40)
            .map(|id| v1::JiraQuickFilter {
                id,
                name: format!("Quick filter {id}"),
                jql: format!("labels = q{id}"),
            })
            .collect();
    }
    b
}

/// Sets up a large-board scenario on a connected state.
pub(super) fn show(state: &mut AppState, name: &str) {
    let mut workspace = state.workspace.raw().clone();
    workspace.boards.insert(0, large_board());
    state.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    state.now_ms = 1_000_000;
    state.jira_accounts = vec![account()];
    state.update(Msg::ToggleBoard);
    state.update(Msg::BoardReveal(1.0));
    state.update(Msg::SelectBoard("jb".into()));
    if name == "jira-large-filter-menu" {
        state.update(Msg::Pointer(urushi_ui::Point::new(1000.0, 100.0)));
        state.update(Msg::OpenMenu(MenuTarget::Filter("jb".into())));
    }
}

#[cfg(test)]
mod tests {
    use urushi_ui::{Primitive, TextSystem, Theme};

    use super::super::{scenario, scenario_frame};
    use super::ISSUES;

    #[test]
    fn a_large_board_builds_only_the_cards_in_sight() {
        let theme = Theme::dark();
        let mut text = TextSystem::bundled_only();
        let frame = scenario_frame("jira-large-board", &theme, &mut text, 1280.0, 820.0)
            .expect("a scenario");
        let cards = frame
            .access
            .iter()
            .filter(|a| a.label.starts_with("Task "))
            .count();
        // Three columns of a few cards each, not 500.
        assert!((3..60).contains(&cards), "{cards} cards built");
        // Each column still counts all its issues.
        let dump = frame.scene.dump();
        assert!(
            dump.contains(&format!("\"{}\"", ISSUES.div_ceil(3))),
            "{dump}"
        );
    }

    #[test]
    fn drawing_a_large_board_again_shapes_no_new_text() {
        let theme = Theme::dark();
        let mut text = TextSystem::bundled_only();
        scenario("jira-large-board", &theme, &mut text, 1280.0, 820.0);
        let shaped = text.shaped_count();
        let scene =
            scenario("jira-large-board", &theme, &mut text, 1280.0, 820.0).expect("a scenario");
        assert_eq!(text.shaped_count(), shaped, "all from the cache");
        let texts = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Text(_)))
            .count();
        assert!(texts < 600, "{texts} texts drawn");
    }

    #[test]
    fn a_long_filters_menu_stops_at_the_window_and_scrolls() {
        let theme = Theme::dark();
        let mut text = TextSystem::bundled_only();
        let frame = scenario_frame("jira-large-filter-menu", &theme, &mut text, 1280.0, 820.0)
            .expect("a scenario");
        let menu = frame
            .access
            .iter()
            .find(|a| a.label == "Menu")
            .expect("the menu");
        assert!(menu.rect.bottom() <= 820.0, "{:?}", menu.rect);
        assert!(
            frame.scroll_regions.iter().any(|r| r.max_offset() > 0.0
                && menu
                    .rect
                    .contains(urushi_ui::Point::new(r.rect.x + 1.0, r.rect.y + 1.0))),
            "the menu scrolls"
        );
    }
}
