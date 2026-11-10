//! Reordering boards: dragging a board's tab onto another, the drop line, and Move left /
//! Move right in the tab's menu (protocol 1.17).

use urushi_client::v1::{self, request::Body};
use urushi_ui::Point;

use super::{attached, bodies, named, workspace};
use crate::board::BOARD_ORDER_SINCE_MINOR;
use crate::keys::Named;
use crate::msg::{BoardMsg, Effect, Msg};
use crate::organize::{Dragged, MenuTarget, board_drop_index};
use crate::state::AppState;

/// The sample workspace plus boards A, B and C (ids `a`, `b`, `c`), on a daemon that
/// can reorder them, with the board panel open.
fn with_boards() -> AppState {
    let mut s = attached();
    s.daemon_minor = BOARD_ORDER_SINCE_MINOR;
    let mut w = workspace();
    for id in ["a", "b", "c"] {
        w.boards.push(v1::Board {
            id: id.into(),
            name: id.to_uppercase(),
            columns: vec![v1::BoardColumn {
                id: format!("{id}-todo"),
                name: "To do".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    s.update(Msg::ToggleBoard);
    s.update(Msg::BoardReveal(1.0));
    s
}

fn moves(fx: &[Effect]) -> Vec<(String, u32)> {
    bodies(fx)
        .into_iter()
        .filter_map(|b| match b {
            Body::MoveBoard(r) => Some((r.board_id.clone(), r.index)),
            _ => None,
        })
        .collect()
}

fn drag(s: &mut AppState, board: &str) {
    s.update(Msg::Pointer(Point::new(10.0, 10.0)));
    s.update(Msg::DragBoard(board.into()));
}

#[test]
fn a_dragged_board_takes_the_place_of_the_tab_it_is_dropped_on() {
    let mut s = with_boards();
    drag(&mut s, "a");
    assert_eq!(
        s.dragging,
        Some((Dragged::Board("a".into()), Point::new(10.0, 10.0)))
    );
    let fx = s.update(Msg::DropBoard("c".into()));
    assert_eq!(moves(&fx), [("a".to_owned(), 2)]);
    assert!(s.dragging.is_none());

    drag(&mut s, "c");
    let fx = s.update(Msg::DropBoard("a".into()));
    assert_eq!(moves(&fx), [("c".to_owned(), 0)]);
}

#[test]
fn dropping_a_board_on_itself_or_cancelling_sends_nothing() {
    let mut s = with_boards();
    drag(&mut s, "b");
    assert!(s.update(Msg::DropBoard("b".into())).is_empty());
    drag(&mut s, "b");
    s.update(Msg::DragCancel);
    assert!(s.dragging.is_none());
    // A task drop while a board is dragged doesn't move the board.
    drag(&mut s, "b");
    let fx = s.update(Msg::DropTask {
        column: "a-todo".into(),
        before: None,
    });
    assert!(moves(&fx).is_empty());
    // Escape ends the drag, and the panel stays open.
    drag(&mut s, "b");
    s.update(Msg::DragOver(Some(Box::new(Msg::DropBoard("a".into())))));
    s.update(Msg::Key(named(Named::Escape)));
    assert!(s.dragging.is_none());
    assert_eq!(s.board.tab_over, None);
    assert!(s.board.open);
}

#[test]
fn the_drop_line_follows_only_tabs_that_would_move_the_board() {
    let mut s = with_boards();
    drag(&mut s, "b");
    s.update(Msg::DragOver(Some(Box::new(Msg::DropBoard("c".into())))));
    assert_eq!(s.board.tab_over.as_deref(), Some("c"));
    s.update(Msg::DragOver(Some(Box::new(Msg::DropBoard("b".into())))));
    assert_eq!(s.board.tab_over, None);
    s.update(Msg::DragOver(Some(Box::new(Msg::DropBoard("a".into())))));
    assert_eq!(s.board.tab_over.as_deref(), Some("a"));
    s.update(Msg::DragOver(None));
    assert_eq!(s.board.tab_over, None);
    s.update(Msg::DragOver(Some(Box::new(Msg::DropBoard("a".into())))));
    s.update(Msg::DragCancel);
    assert_eq!(s.board.tab_over, None);
}

#[test]
fn move_left_and_right_send_the_neighbouring_index() {
    let mut s = with_boards();
    let menu_moves = |s: &mut AppState, board: &str| {
        s.update(Msg::OpenMenu(MenuTarget::Board(board.into())));
        let frame = render(s);
        let mut found: Vec<(String, u32)> = frame
            .hits
            .iter()
            .filter_map(|h| match &h.on_click {
                Some(Msg::Board(BoardMsg::MoveBoard { board, index })) => {
                    Some((board.clone(), *index))
                }
                _ => None,
            })
            .collect();
        found.sort();
        s.update(Msg::CloseMenu);
        found
    };
    // The first can only go right, the last only left, the middle both ways.
    assert_eq!(menu_moves(&mut s, "a"), [("a".to_owned(), 1)]);
    assert_eq!(
        menu_moves(&mut s, "b"),
        [("b".to_owned(), 0), ("b".to_owned(), 2)]
    );
    assert_eq!(menu_moves(&mut s, "c"), [("c".to_owned(), 1)]);
    s.daemon_minor = BOARD_ORDER_SINCE_MINOR - 1;
    assert!(menu_moves(&mut s, "b").is_empty());
    s.daemon_minor = BOARD_ORDER_SINCE_MINOR;

    s.update(Msg::OpenMenu(MenuTarget::Board("b".into())));
    let fx = s.update(Msg::Board(BoardMsg::MoveBoard {
        board: "b".into(),
        index: 0,
    }));
    assert_eq!(moves(&fx), [("b".to_owned(), 0)]);
    assert!(s.menu.is_none());
}

#[test]
fn an_older_daemon_cannot_drag_boards() {
    let mut s = with_boards();
    s.daemon_minor = BOARD_ORDER_SINCE_MINOR - 1;
    drag(&mut s, "a");
    assert!(s.dragging.is_none());
}

#[test]
fn closing_the_panel_ends_a_board_drag() {
    let mut s = with_boards();
    drag(&mut s, "a");
    s.update(Msg::DragOver(Some(Box::new(Msg::DropBoard("b".into())))));
    s.update(Msg::ToggleBoard);
    assert!(s.dragging.is_none());
    assert_eq!(s.board.tab_over, None);
}

#[test]
fn drop_indexes_count_without_the_dragged_board() {
    let s = with_boards();
    let b = s.workspace.boards();
    assert_eq!(board_drop_index(b, "a", "b"), Some(1));
    assert_eq!(board_drop_index(b, "a", "c"), Some(2));
    assert_eq!(board_drop_index(b, "c", "b"), Some(1));
    assert_eq!(board_drop_index(b, "c", "c"), None);
    assert_eq!(board_drop_index(b, "x", "a"), None);
}

fn render(s: &AppState) -> urushi_ui::Frame<Msg> {
    let mut text = urushi_ui::TextSystem::bundled_only();
    let root = crate::view::view(
        s,
        &urushi_ui::Theme::dark(),
        &std::collections::HashMap::new(),
    );
    urushi_ui::build(
        &root,
        urushi_ui::Rect::new(0.0, 0.0, 1200.0, 800.0),
        &mut text,
        None,
    )
}

#[test]
fn tabs_drag_open_a_menu_and_accept_a_dragged_board() {
    let mut s = with_boards();
    let frame = render(&s);
    let draggable = |f: &urushi_ui::Frame<Msg>| {
        f.hits
            .iter()
            .filter(|h| matches!(&h.on_drag, Some(Msg::DragBoard(_))))
            .count()
    };
    let menus = |f: &urushi_ui::Frame<Msg>| {
        f.hits
            .iter()
            .filter(|h| {
                matches!(
                    &h.on_secondary_click,
                    Some(Msg::OpenMenu(MenuTarget::Board(_)))
                )
            })
            .count()
    };
    assert_eq!(draggable(&frame), 3);
    assert_eq!(menus(&frame), 3);

    // Only tabs take a dragged board: the columns don't.
    drag(&mut s, "a");
    let frame = render(&s);
    let targets: Vec<&Msg> = frame
        .hits
        .iter()
        .filter_map(|h| h.on_drop.as_ref())
        .collect();
    assert_eq!(targets.len(), 3);
    assert!(targets.iter().all(|m| matches!(m, Msg::DropBoard(_))));
    s.update(Msg::DragCancel);

    // An older daemon can't reorder, so nothing offers it.
    s.daemon_minor = BOARD_ORDER_SINCE_MINOR - 1;
    let frame = render(&s);
    assert_eq!(draggable(&frame), 0);
    assert_eq!(menus(&frame), 0);
}

#[test]
fn a_single_board_has_nothing_to_reorder() {
    let mut s = with_boards();
    let mut w = workspace();
    w.boards.push(v1::Board {
        id: "only".into(),
        name: "Only".into(),
        ..Default::default()
    });
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    let frame = render(&s);
    assert!(
        !frame
            .hits
            .iter()
            .any(|h| matches!(&h.on_drag, Some(Msg::DragBoard(_))))
    );
}
