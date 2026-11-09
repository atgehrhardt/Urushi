//! The task-board panel (opening, editing, dragging, deleting) and the status bar's
//! agent usage polling.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_ui::Point;

use super::{attached, bodies, connected, id, key, named, shortcut};
use crate::keys::{KeyPress, Mods, Named};
use crate::msg::{Effect, Followup, Msg};
use crate::organize::Prompt;
use crate::state::{AppState, Dialog, Field};

pub(super) fn board() -> v1::Board {
    let task = |id: &str, title: &str| v1::Task {
        id: id.into(),
        title: title.into(),
        notes: "line one\nline two".into(),
        ..Default::default()
    };
    v1::Board {
        id: "b".into(),
        name: "Sprint".into(),
        created_at_ms: 0,
        columns: vec![
            v1::BoardColumn {
                id: "todo".into(),
                name: "To do".into(),
                tasks: vec![task("a", "A"), task("b", "B")],
                ..Default::default()
            },
            v1::BoardColumn {
                id: "done".into(),
                name: "Done".into(),
                tasks: vec![],
                ..Default::default()
            },
        ],
    }
}

/// Connected to a 1.5 daemon, with one board, and the panel open.
fn with_board() -> AppState {
    let mut s = attached();
    s.daemon_minor = 5;
    let mut workspace = super::workspace();
    workspace.boards = vec![board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s
}

#[test]
fn the_panel_covers_the_terminals_until_closed() {
    let mut s = with_board();
    assert!(s.terminals_covered());
    assert!(
        s.update(Msg::Key(key("x", Mods::default()))).is_empty(),
        "typing stays out of the terminal"
    );
    assert!(s.update(Msg::Text("x".into())).is_empty());
    s.update(Msg::Key(named(Named::Escape)));
    assert!(!s.board.open);
    // Still sliding shut: still covered.
    s.update(Msg::BoardReveal(0.3));
    assert!(s.terminals_covered());
    s.update(Msg::BoardReveal(0.0));
    assert!(!s.terminals_covered());
    assert!(!s.update(Msg::Key(key("x", Mods::default()))).is_empty());
}

#[test]
fn clicking_a_task_opens_it_and_saving_sends_only_what_changed() {
    let mut s = with_board();
    s.update(Msg::PressTask("a".into()));
    assert!(s.dialog.is_none(), "opens on release, not on press");
    s.update(Msg::PressEnded);
    let Some(Dialog::Task { notes, column, .. }) = &s.dialog else {
        panic!("{:?}", s.dialog);
    };
    assert_eq!(notes.text(), "line one\nline two");
    assert_eq!(column, "todo");

    // Unchanged notes aren't sent: only the title and column are.
    s.update(Msg::Key(named(Named::End)));
    s.update(Msg::Text("2".into()));
    s.update(Msg::PickColumn("done".into()));
    let fx = s.update(Msg::Submit);
    let sent = bodies(&fx);
    assert!(
        matches!(
            sent[..],
            [
                Body::MoveTask(v1::MoveTaskRequest {
                    index: u32::MAX,
                    ..
                }),
                Body::UpdateTask(v1::UpdateTaskRequest {
                    title: Some(_),
                    notes: None,
                    ..
                })
            ]
        ),
        "{sent:?}"
    );
    if let Body::UpdateTask(u) = sent[1] {
        assert_eq!(u.title.as_deref(), Some("A2"));
    }
}

#[test]
fn notes_take_several_lines_and_the_shortcut_saves() {
    let mut s = with_board();
    s.update(Msg::PressTask("a".into()));
    s.update(Msg::PressEnded);
    s.update(Msg::FocusField(Field::TaskNotes));
    // Enter starts a line in the notes instead of saving.
    assert!(bodies(&s.update(Msg::Key(named(Named::Enter)))).is_empty());
    s.update(Msg::Text("three".into()));
    // Up and Home go to the start of the line above.
    s.update(Msg::Key(named(Named::Up)));
    s.update(Msg::Key(named(Named::Home)));
    s.update(Msg::Text("> ".into()));
    s.update(Msg::Paste("pasted\r\nlines ".into()));
    let Some(Dialog::Task { notes, .. }) = &s.dialog else {
        panic!("{:?}", s.dialog);
    };
    assert_eq!(notes.text(), "line one\n> pasted\nlines line two\nthree");
    let save = KeyPress {
        mods: shortcut_or_ctrl(),
        ..named(Named::Enter)
    };
    let fx = s.update(Msg::Key(save));
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::UpdateTask(v1::UpdateTaskRequest { title: None, notes: Some(n), .. })]
                if n == "line one\n> pasted\nlines line two\nthree"
        ),
        "{fx:?}"
    );
    // Enter in the title still saves.
    s.update(Msg::CloseDialog);
    s.update(Msg::PressTask("b".into()));
    s.update(Msg::PressEnded);
    s.update(Msg::Text("!".into()));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    assert!(matches!(bodies(&fx)[..], [Body::UpdateTask(_)]), "{fx:?}");
}

/// The dialogs' command modifier: Cmd on Apple platforms, Ctrl elsewhere.
fn shortcut_or_ctrl() -> Mods {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        shortcut()
    } else {
        Mods {
            ctrl: true,
            ..Mods::default()
        }
    }
}

#[test]
fn the_board_shortcut_opens_and_closes_the_panel() {
    let mut s = attached();
    s.daemon_minor = 5;
    let toggle = || Msg::Key(key("b", shortcut()));
    assert!(
        s.update(toggle()).is_empty(),
        "nothing reaches the terminal"
    );
    assert!(s.board.open && s.terminals_covered());
    s.update(toggle());
    assert!(!s.board.open);
}

#[test]
fn new_tasks_need_a_title() {
    let mut s = with_board();
    s.update(Msg::NewTask {
        board: "b".into(),
        column: "done".into(),
    });
    assert!(bodies(&s.update(Msg::Submit)).is_empty());
    s.update(Msg::Text("Write docs".into()));
    let fx = s.update(Msg::Submit);
    assert!(matches!(
        bodies(&fx)[..],
        [Body::CreateTask(v1::CreateTaskRequest { column_id, title, .. })]
            if column_id == "done" && title == "Write docs"
    ));
}

#[test]
fn dragging_a_task_moves_it_where_it_is_dropped() {
    let mut s = with_board();
    s.update(Msg::Pointer(Point::new(10.0, 10.0)));
    s.update(Msg::PressTask("a".into()));
    s.update(Msg::DragTask("a".into()));
    s.update(Msg::DragMove(Point::new(50.0, 60.0)));
    assert_eq!(s.board.dragging, Some(("a".into(), Point::new(50.0, 60.0))));
    assert!(s.board.pressed.is_none(), "a drag doesn't open the task");
    let fx = s.update(Msg::DropTask {
        column: "done".into(),
        before: None,
    });
    assert!(matches!(
        bodies(&fx)[..],
        [Body::MoveTask(v1::MoveTaskRequest { task_id, column_id, index: 0 })]
            if task_id == "a" && column_id == "done"
    ));
    // Dropped where it already was: nothing to do.
    s.update(Msg::DragTask("b".into()));
    assert!(
        bodies(&s.update(Msg::DropTask {
            column: "todo".into(),
            before: None
        }))
        .is_empty()
    );
    // Escape ends a drag.
    s.update(Msg::DragTask("b".into()));
    s.update(Msg::Key(named(Named::Escape)));
    assert!(s.board.dragging.is_none());
    assert!(s.board.open, "the first Escape only ends the drag");
}

#[test]
fn boards_are_created_selected_and_only_deleted_after_confirming() {
    let mut s = with_board();
    s.update(Msg::OpenPrompt(Prompt::NewBoard));
    s.update(Msg::Text("Next".into()));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(bodies(&fx)[..], [Body::CreateBoard(v1::CreateBoardRequest { name })] if name == "Next")
    );
    s.update(Msg::Response {
        followup: Followup::BoardCreated,
        result: Ok(Outcome::Board(v1::Board {
            id: "n".into(),
            ..Default::default()
        })),
    });
    assert_eq!(s.board.selected.as_deref(), Some("n"));
    assert!(s.dialog.is_none());

    s.update(Msg::OpenDeleteBoard("b".into()));
    assert!(matches!(
        s.dialog,
        Some(Dialog::DeleteBoard { tasks: 2, .. })
    ));
    assert!(
        bodies(&s.update(Msg::Submit)).is_empty(),
        "Enter doesn't delete"
    );
    let fx = s.update(Msg::ConfirmDeleteBoard);
    assert!(
        matches!(bodies(&fx)[..], [Body::DeleteBoard(v1::DeleteBoardRequest { board_id })] if board_id == "b")
    );
}

fn usage(agent: v1::Agent) -> v1::AgentUsage {
    v1::AgentUsage {
        agent: agent.into(),
        plan: "Max 5x".into(),
        windows: vec![v1::UsageWindow {
            label: "5h".into(),
            used_percent: 12.0,
            resets_at_ms: 0,
        }],
        ..Default::default()
    }
}

#[test]
fn ticks_ask_for_the_focused_panes_agent_usage_one_at_a_time() {
    let mut s = attached();
    s.daemon_minor = 5;
    let fx = s.update(Msg::Tick(1_000));
    assert_eq!(s.now_ms, 1_000);
    assert!(matches!(
        &fx[..],
        [Effect::Request {
            body: Body::GetAgentUsage(v1::GetAgentUsageRequest { pane_id }),
            followup: Followup::Usage(_),
        }] if *pane_id == id(100)
    ));
    assert!(
        s.update(Msg::Tick(2_000)).is_empty(),
        "one request at a time"
    );

    s.update(Msg::Response {
        followup: Followup::Usage(id(100)),
        result: Ok(Outcome::AgentUsage(usage(v1::Agent::Claude))),
    });
    assert_eq!(s.shown_usage().map(|u| u.plan.as_str()), Some("Max 5x"));

    // A plain shell shows nothing; a failure clears the bar without a banner.
    assert!(!s.update(Msg::Tick(3_000)).is_empty());
    s.update(Msg::Response {
        followup: Followup::Usage(id(100)),
        result: Ok(Outcome::AgentUsage(usage(v1::Agent::Unspecified))),
    });
    assert!(s.shown_usage().is_none());
    s.update(Msg::Tick(4_000));
    s.update(Msg::Response {
        followup: Followup::Usage(id(100)),
        result: Err("gone".into()),
    });
    assert!(s.error.is_none());
    assert!(s.usage.is_none());

    // Usage of another pane isn't shown for this one.
    s.usage = Some((id(999), usage(v1::Agent::Claude)));
    assert!(s.shown_usage().is_none());
}

#[test]
fn older_daemons_are_not_asked_for_usage() {
    let mut s = connected();
    assert!(s.update(Msg::Tick(1)).is_empty());
}

#[test]
fn a_cards_context_menu_opens_moves_and_deletes_it_without_the_editor() {
    use crate::msg::BoardMsg;
    use crate::organize::{MenuPage, MenuTarget};
    let mut s = with_board();
    s.update(Msg::Pointer(Point::new(300.0, 120.0)));
    s.update(Msg::OpenMenu(MenuTarget::Task("a".into())));
    let menu = s
        .menu
        .as_ref()
        .expect("a secondary click opens the card's menu");
    assert_eq!(menu.at, Point::new(300.0, 120.0));
    assert!(s.board.pressed.is_none() && s.board.dragging.is_none());

    // Delete goes straight to the daemon; no editor, no dialog.
    let fx = s.update(Msg::Board(BoardMsg::DeleteTask("a".into())));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::DeleteTask(v1::DeleteTaskRequest { task_id })] if task_id == "a"
    ));
    assert!(s.menu.is_none() && s.dialog.is_none());

    // Move to another column: to its bottom. Its own column does nothing.
    s.update(Msg::OpenMenu(MenuTarget::Task("b".into())));
    s.update(Msg::MenuPage(MenuPage::Move));
    let fx = s.update(Msg::Board(BoardMsg::MoveTask {
        task: "b".into(),
        column: "done".into(),
    }));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::MoveTask(v1::MoveTaskRequest { task_id, column_id, index: u32::MAX })]
            if task_id == "b" && column_id == "done"
    ));
    let fx = s.update(Msg::Board(BoardMsg::MoveTask {
        task: "b".into(),
        column: "todo".into(),
    }));
    assert!(fx.is_empty());

    // Open shows the editor.
    s.update(Msg::OpenMenu(MenuTarget::Task("b".into())));
    s.update(Msg::Board(BoardMsg::OpenTask("b".into())));
    assert!(s.menu.is_none());
    assert!(matches!(&s.dialog, Some(Dialog::Task { task: Some(t), .. }) if t == "b"));
}
