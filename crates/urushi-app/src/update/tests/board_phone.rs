//! The task board on a phone (UR-58, ADR 0046): one column at a time, chosen by its tab
//! or a sideways swipe; cards dropped on a column's tab; the board picker and the
//! board's "⋯" menu.

use std::collections::HashMap;

use urushi_client::v1::{self, request::Body};
use urushi_ui::{Frame, Point, Rect, TextSystem, Theme, build};

use super::{attached, bodies};
use crate::msg::{JiraMsg, Msg};
use crate::organize::{MenuTarget, Prompt};
use crate::state::AppState;

/// A phone connected to a 1.12 daemon with a Jira board (To Do, Done) and an ordinary
/// one (To do, Done), the panel open on the Jira board.
fn phone() -> AppState {
    let mut s = attached();
    s.compact = true;
    s.daemon_minor = 12;
    let mut workspace = super::workspace();
    workspace.boards = vec![super::jira::jira_board(), super::board::board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s.update(Msg::BoardReveal(1.0));
    s
}

fn shown(s: &AppState) -> String {
    let board = s.board.board(&s.workspace).unwrap();
    s.board.shown_column(board).unwrap().id.clone()
}

fn render(s: &AppState) -> Frame<Msg> {
    let mut text = TextSystem::bundled_only();
    build(
        &crate::view::view(s, &Theme::dark(), &HashMap::new()),
        Rect::new(0.0, 0.0, 393.0, 852.0),
        &mut text,
        None::<Point>,
    )
}

/// Where the accessibility node labelled `label` is.
fn rect_of(frame: &Frame<Msg>, label: &str) -> Rect {
    frame
        .access
        .iter()
        .find(|a| a.label == label)
        .unwrap_or_else(|| panic!("{label} missing"))
        .rect
}

fn center(r: Rect) -> Point {
    Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0)
}

#[test]
fn columns_show_one_at_a_time_chosen_by_tab_or_swipe() {
    let mut s = phone();
    assert_eq!(shown(&s), "todo");
    let dump = render(&s).scene.dump();
    assert!(dump.contains("\"Checkout\""), "{dump}");

    // A swipe to the left shows the next column, and stops at the last.
    s.update(Msg::Swipe(-120.0));
    assert_eq!(shown(&s), "done");
    s.update(Msg::Swipe(-120.0));
    assert_eq!(shown(&s), "done");
    let dump = render(&s).scene.dump();
    assert!(!dump.contains("\"Checkout\""), "only Done's cards: {dump}");
    s.update(Msg::Swipe(120.0));
    assert_eq!(shown(&s), "todo");
    s.update(Msg::Swipe(120.0));
    assert_eq!(shown(&s), "todo", "nothing before the first");

    // A tap on a column's tab shows it.
    let frame = render(&s);
    let tab = center(rect_of(&frame, "Column Done tab"));
    let Some(urushi_ui::Hit { on_click, .. }) = frame.hit(tab) else {
        panic!("no tab under {tab:?}");
    };
    let msg = on_click.clone().unwrap();
    assert!(matches!(&msg, Msg::ShowColumn(c) if c == "done"), "{msg:?}");
    s.update(msg);
    assert_eq!(shown(&s), "done");

    // Another board starts at its first column.
    s.update(Msg::SelectBoard("b".into()));
    assert_eq!(s.board.column, None);
    assert_eq!(shown(&s), "todo");
}

#[test]
fn swipes_change_columns_only_on_a_phones_board_in_plain_view() {
    let mut s = phone();
    s.compact = false;
    s.update(Msg::Swipe(-120.0));
    assert_eq!(shown(&s), "todo", "not on a desktop");

    let mut s = phone();
    s.update(Msg::OpenMenu(MenuTarget::Boards));
    s.update(Msg::Swipe(-120.0));
    assert_eq!(shown(&s), "todo", "not under a menu");

    let mut s = phone();
    s.update(Msg::OpenPrompt(Prompt::NewBoard));
    s.update(Msg::Swipe(-120.0));
    assert_eq!(shown(&s), "todo", "not under a dialog");

    let mut s = phone();
    s.update(Msg::DragTask("j1".into()));
    s.update(Msg::Swipe(-120.0));
    assert_eq!(shown(&s), "todo", "not while dragging a card");

    let mut s = phone();
    s.update(Msg::ToggleBoard);
    s.update(Msg::Swipe(-120.0));
    assert_eq!(shown(&s), "todo", "not with the panel closed");
}

#[test]
fn a_card_dropped_on_a_column_tab_moves_to_that_column() {
    let mut s = phone();
    s.update(Msg::Pointer(Point::new(100.0, 300.0)));
    s.update(Msg::DragTask("j1".into()));
    let frame = render(&s);
    let tab = center(rect_of(&frame, "Column Done tab"));
    let drop = frame.drop_target(tab).expect("the tab takes the card");
    let fx = s.update(drop);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::MoveTask(v1::MoveTaskRequest { task_id, column_id, .. })]
                if task_id == "j1" && column_id == "done"
        ),
        "{fx:?}"
    );
}

#[test]
fn the_board_picker_lists_the_boards_and_ways_to_add_one() {
    let mut s = phone();
    s.update(Msg::OpenMenu(MenuTarget::Boards));
    let dump = render(&s).scene.dump();
    for text in [
        "\"ACME board\"",
        "\"Sprint\"",
        "\"✓\"",
        "\"New board…\"",
        "\"Import from Jira…\"",
    ] {
        assert!(dump.contains(text), "{text} missing: {dump}");
    }
    s.update(Msg::SelectBoard("b".into()));
    assert!(s.menu.is_none(), "choosing a board closes the menu");
    assert_eq!(s.board.board(&s.workspace).unwrap().id, "b");
}

#[test]
fn a_phones_board_menu_holds_the_header_actions() {
    let mut s = phone();
    s.jira = Some(v1::JiraAccount {
        connected: true,
        ..Default::default()
    });
    s.update(Msg::OpenMenu(MenuTarget::Board("jb".into())));
    let frame = render(&s);
    let dump = frame.scene.dump();
    for text in ["\"Synced\"", "\"Sync now\"", "\"Remove…\""] {
        assert!(dump.contains(text), "{text} missing: {dump}");
    }
    let sync = center(rect_of(&frame, "Sync now"));
    let msg = frame.hit(sync).and_then(|h| h.on_click.clone()).unwrap();
    assert!(
        matches!(&msg, Msg::Jira(JiraMsg::Sync(b)) if b == "jb"),
        "{msg:?}"
    );

    s.update(Msg::CloseMenu);
    s.update(Msg::SelectBoard("b".into()));
    s.update(Msg::OpenMenu(MenuTarget::Board("b".into())));
    let dump = render(&s).scene.dump();
    for text in ["\"Rename…\"", "\"Delete…\""] {
        assert!(dump.contains(text), "{text} missing: {dump}");
    }
    s.update(Msg::OpenDeleteBoard("b".into()));
    assert!(s.menu.is_none(), "the question replaces the menu");
    assert!(s.dialog.is_some());
}
