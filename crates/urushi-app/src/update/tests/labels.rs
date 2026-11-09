//! Columns (added, renamed, recolored, moved, removed after asking), task colors and
//! tags (from the editor and the card menu), and the tag filter.

use urushi_client::v1::{self, request::Body};

use super::board::board;
use super::{attached, bodies, named};
use crate::keys::Named;
use crate::msg::{BoardMsg, Followup, Msg};
use crate::organize::{MenuTarget, Prompt};
use crate::state::{AppState, Dialog, Field};

/// A 1.8 daemon's board, with task "a" tagged "ui" and the panel open.
fn with_labels() -> AppState {
    let mut s = attached();
    s.daemon_minor = 8;
    let mut workspace = super::workspace();
    let mut b = board();
    b.columns[0].tasks[0].tags = vec!["ui".into()];
    b.columns[0].tasks[0].color = v1::LabelColor::Teal.into();
    workspace.boards = vec![b];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s
}

fn open_task(s: &mut AppState, id: &str) {
    s.update(Msg::PressTask(id.into()));
    s.update(Msg::PressEnded);
}

fn board_msg(m: BoardMsg) -> Msg {
    Msg::Board(m)
}

#[test]
fn the_editor_sends_color_and_tags_only_when_they_change() {
    let mut s = with_labels();
    open_task(&mut s, "a");
    // Opening and saving changes nothing.
    let fx = s.update(Msg::Submit);
    assert!(bodies(&fx).is_empty(), "{fx:?}");

    open_task(&mut s, "a");
    s.update(board_msg(BoardMsg::PickColor(v1::LabelColor::Red.into())));
    s.update(board_msg(BoardMsg::RemoveTag("ui".into())));
    s.update(board_msg(BoardMsg::AddTag("backend".into())));
    // Enter in the tag field adds the typed tag and keeps the editor open.
    s.update(Msg::FocusField(Field::TaskTag));
    s.update(Msg::Paste("  Docs ".into()));
    assert!(bodies(&s.update(Msg::Key(named(Named::Enter)))).is_empty());
    assert!(s.dialog.is_some());
    // A tag typed but not added goes with Save.
    s.update(Msg::Paste("later".into()));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::UpdateTask(v1::UpdateTaskRequest { task_id, title: None, notes: None, color: Some(c), tags: Some(tags) })]
                if task_id == "a" && *c == i32::from(v1::LabelColor::Red)
                    && tags.tags == ["backend", "Docs", "later"]
        ),
        "{fx:?}"
    );
}

#[test]
fn a_new_task_is_created_with_its_color_and_tags() {
    let mut s = with_labels();
    s.update(Msg::NewTask {
        board: "b".into(),
        column: "todo".into(),
    });
    s.update(Msg::Paste("New".into()));
    s.update(board_msg(BoardMsg::PickColor(v1::LabelColor::Blue.into())));
    s.update(board_msg(BoardMsg::AddTag("ui".into())));
    s.update(board_msg(BoardMsg::AddTag("UI".into())));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::CreateTask(v1::CreateTaskRequest { color, tags, .. })]
                if *color == i32::from(v1::LabelColor::Blue) && tags == &["ui"]
        ),
        "{fx:?}"
    );
}

#[test]
fn the_card_menu_colors_a_task() {
    let mut s = with_labels();
    s.update(Msg::OpenMenu(MenuTarget::Task("b".into())));
    let fx = s.update(board_msg(BoardMsg::ColorTask {
        task: "b".into(),
        color: v1::LabelColor::Green.into(),
    }));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::UpdateTask(v1::UpdateTaskRequest { task_id, color: Some(c), tags: None, .. })]
            if task_id == "b" && *c == i32::from(v1::LabelColor::Green)
    ));
    assert!(s.menu.is_none());
}

#[test]
fn columns_are_added_renamed_colored_moved_and_removed_after_asking() {
    let mut s = with_labels();
    s.update(Msg::OpenPrompt(Prompt::NewColumn("b".into())));
    s.update(Msg::Paste("Review".into()));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            &fx[..],
            [crate::msg::Effect::Request { body: Body::CreateColumn(v1::CreateColumnRequest { board_id, name, index: u32::MAX }), followup: Followup::CloseDialog }]
                if board_id == "b" && name == "Review"
        ),
        "{fx:?}"
    );
    s.dialog = None;

    s.update(Msg::OpenPrompt(Prompt::RenameColumn("todo".into())));
    let Some(Dialog::Prompt { field, .. }) = &s.dialog else {
        panic!("{:?}", s.dialog)
    };
    assert_eq!(field.text(), "To do", "the prompt starts with the name");
    s.dialog = None;

    let fx = s.update(board_msg(BoardMsg::ColorColumn {
        column: "done".into(),
        color: v1::LabelColor::Purple.into(),
    }));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::UpdateColumn(v1::UpdateColumnRequest { column_id, name: None, color: Some(_) })] if column_id == "done"
    ));
    let fx = s.update(board_msg(BoardMsg::MoveColumn {
        column: "done".into(),
        index: 0,
    }));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::MoveColumn(v1::MoveColumnRequest { column_id, index: 0 })] if column_id == "done"
    ));

    // Removing asks first, saying where the tasks go; Enter doesn't confirm.
    s.update(board_msg(BoardMsg::OpenDeleteColumn("todo".into())));
    assert!(matches!(
        &s.dialog,
        Some(Dialog::DeleteColumn { tasks: 2, into, .. }) if into == "Done"
    ));
    assert!(bodies(&s.update(Msg::Key(named(Named::Enter)))).is_empty());
    let fx = s.update(board_msg(BoardMsg::ConfirmDeleteColumn));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::DeleteColumn(v1::DeleteColumnRequest { column_id })] if column_id == "todo"
    ));
}

#[test]
fn the_tag_filter_resets_with_another_board() {
    let mut s = with_labels();
    s.update(Msg::OpenMenu(MenuTarget::TagFilter("b".into())));
    s.update(board_msg(BoardMsg::FilterTag(Some("UI".into()))));
    assert_eq!(s.board.tag_filter.as_deref(), Some("UI"));
    assert!(s.menu.is_none());
    let (_, _, a) = s.workspace.task("a").unwrap();
    let (_, _, b) = s.workspace.task("b").unwrap();
    let filter = s.board.tag_filter.as_deref();
    assert!(crate::board::shows(filter, a) && !crate::board::shows(filter, b));
    s.update(Msg::SelectBoard("b".into()));
    assert!(s.board.tag_filter.is_some(), "the same board keeps it");
    s.update(Msg::SelectBoard("other".into()));
    assert!(s.board.tag_filter.is_none());
}
