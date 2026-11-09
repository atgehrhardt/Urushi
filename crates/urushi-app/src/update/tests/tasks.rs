//! The task editor's subtasks, starting threads from tasks and subtasks, and where a
//! dragged task would land.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{RepoId, ThreadId};

use super::board::board;
use super::{attached, bodies, named};
use crate::keys::Named;
use crate::msg::{BoardMsg, Effect, Followup, Msg};
use crate::organize::MenuTarget;
use crate::state::{AppState, Dialog, Field, TaskLink};

fn thread_id() -> String {
    ThreadId::from_u128(10).to_string()
}

/// A 1.6 daemon's workspace: task "a" has two subtasks, the second started as the
/// workspace's thread; task "b" has none.
fn with_subtasks() -> AppState {
    let mut s = attached();
    s.daemon_minor = 6;
    let mut workspace = super::workspace();
    let mut b = board();
    b.columns[0].tasks[0].subtasks = vec![
        v1::Subtask {
            id: "s1".into(),
            title: "First".into(),
            ..Default::default()
        },
        v1::Subtask {
            id: "s2".into(),
            title: "Second".into(),
            done: true,
            thread_id: thread_id(),
            ..Default::default()
        },
    ];
    workspace.boards = vec![b];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s
}

fn open_task(s: &mut AppState, id: &str) {
    s.update(Msg::PressTask(id.into()));
    s.update(Msg::PressEnded);
}

#[test]
fn an_existing_tasks_subtasks_change_right_away() {
    let mut s = with_subtasks();
    open_task(&mut s, "a");
    let fx = s.update(Msg::ToggleSubtask("s1".into()));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::UpdateSubtask(v1::UpdateSubtaskRequest { subtask_id, title: None, done: Some(true) })]
            if subtask_id == "s1"
    ));
    // Enter in the "add a subtask" field adds it and keeps the editor open.
    s.update(Msg::FocusField(Field::NewSubtask));
    assert!(
        bodies(&s.update(Msg::Key(named(Named::Enter)))).is_empty(),
        "blank: saves"
    );
    open_task(&mut s, "a");
    s.update(Msg::FocusField(Field::NewSubtask));
    s.update(Msg::Text("Third".into()));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::CreateSubtask(v1::CreateSubtaskRequest { task_id, title })]
            if task_id == "a" && title == "Third"
    ));
    assert!(
        matches!(&s.dialog, Some(Dialog::Task { subtasks, .. }) if subtasks.new.text().is_empty())
    );
    let fx = s.update(Msg::DeleteSubtask("s2".into()));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::DeleteSubtask(v1::DeleteSubtaskRequest { subtask_id })] if subtask_id == "s2"
    ));
}

#[test]
fn a_new_tasks_subtasks_are_created_with_it() {
    let mut s = with_subtasks();
    s.update(Msg::NewTask {
        board: "b".into(),
        column: "todo".into(),
    });
    s.update(Msg::Text("Plan".into()));
    s.update(Msg::FocusField(Field::NewSubtask));
    for step in ["one", "two", "three"] {
        s.update(Msg::Text(step.into()));
        assert!(bodies(&s.update(Msg::Key(named(Named::Enter)))).is_empty());
    }
    s.update(Msg::RemoveDraftSubtask(1));
    // Typed but not added yet: goes along too.
    s.update(Msg::Text("four".into()));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::CreateTask(v1::CreateTaskRequest { title, subtasks, .. })]
                if title == "Plan" && *subtasks == ["one", "three", "four"]
        ),
        "{fx:?}"
    );
}

#[test]
fn starting_a_thread_from_a_task_links_it_and_goes_there_next_time() {
    let mut s = with_subtasks();
    open_task(&mut s, "a");
    s.update(Msg::StartThread(TaskLink {
        task: "a".into(),
        subtask: Some("s1".into()),
    }));
    let Some(Dialog::NewThread {
        title,
        branch,
        task,
        repo,
        ..
    }) = &s.dialog
    else {
        panic!("{:?}", s.dialog);
    };
    assert_eq!(title.text(), "First");
    assert_eq!(
        branch.text(),
        "urushi/first",
        "the branch follows the title"
    );
    assert_eq!(
        *repo,
        RepoId::from_u128(1).to_string(),
        "the thread on screen's repo"
    );
    assert_eq!(task.as_ref().and_then(|l| l.subtask.as_deref()), Some("s1"));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::CreateThread(v1::CreateThreadRequest { task_id, subtask_id, title, .. })]
                if task_id == "a" && subtask_id == "s1" && title == "First"
        ),
        "{fx:?}"
    );
    s.update(Msg::Response {
        followup: Followup::ThreadCreated,
        result: Ok(Outcome::Thread(v1::Thread {
            id: "new".into(),
            ..Default::default()
        })),
    });
    assert!(!s.board.open, "the board makes way for the new thread");
    assert!(s.dialog.is_none());

    // Already started: the same button goes to the thread.
    s.selected_thread = None;
    s.update(Msg::ToggleBoard);
    s.update(Msg::StartThread(TaskLink {
        task: "a".into(),
        subtask: Some("s2".into()),
    }));
    assert_eq!(s.selected_thread, Some(thread_id()));
    assert!(s.dialog.is_none() && !s.board.open);
}

#[test]
fn a_task_can_start_a_thread_titled_after_it() {
    let mut s = with_subtasks();
    s.update(Msg::StartThread(TaskLink {
        task: "b".into(),
        subtask: None,
    }));
    assert!(matches!(
        &s.dialog,
        Some(Dialog::NewThread { title, task: Some(TaskLink { subtask: None, .. }), .. })
            if title.text() == "B"
    ));
    // Unknown tasks do nothing.
    s.dialog = None;
    s.update(Msg::StartThread(TaskLink {
        task: "zz".into(),
        subtask: None,
    }));
    assert!(s.dialog.is_none());
}

#[test]
fn a_thread_started_from_a_task_can_be_filed_under_another_project_or_none() {
    let mut s = attached();
    let mut workspace = super::workspace();
    workspace.threads[0].folder_id = "f1".into();
    for (id, name) in [("f1", "Billing"), ("f2", "Search")] {
        workspace.folders.push(v1::Folder {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        });
    }
    workspace.boards = vec![board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    let link = TaskLink {
        task: "a".into(),
        subtask: None,
    };
    let folder = |s: &mut AppState| match bodies(&s.update(Msg::Submit))[..] {
        [Body::CreateThread(r)] => r.folder_id.clone(),
        ref other => panic!("{other:?}"),
    };

    // By default it goes where the thread near the task is.
    s.update(Msg::StartThread(link.clone()));
    assert!(matches!(&s.dialog, Some(Dialog::NewThread { project: Some(p), .. }) if p == "f1"));
    s.update(Msg::PickProject(Some("f2".into())));
    assert_eq!(folder(&mut s), "f2");

    s.update(Msg::StartThread(link));
    s.update(Msg::PickProject(None));
    assert_eq!(folder(&mut s), "");
}

#[test]
fn the_drop_line_shows_only_where_a_task_would_move() {
    let mut s = with_subtasks();
    s.update(Msg::DragTask("a".into()));
    let over = |column: &str, before: Option<&str>| {
        Msg::DragOver(Some(Box::new(Msg::DropTask {
            column: column.into(),
            before: before.map(Into::into),
        })))
    };
    s.update(over("done", None));
    assert_eq!(s.board.over, Some(("done".into(), None)));
    s.update(over("todo", Some("b")));
    assert_eq!(s.board.over, None, "already above b");
    s.update(over("todo", None));
    assert_eq!(s.board.over, Some(("todo".into(), None)));
    s.update(Msg::DragOver(None));
    assert_eq!(s.board.over, None);
    s.update(over("done", None));
    s.update(Msg::DropTask {
        column: "done".into(),
        before: None,
    });
    assert_eq!(s.board.over, None, "a drop clears it");
}

#[test]
fn older_daemons_get_no_subtask_fields() {
    let mut s = with_subtasks();
    s.daemon_minor = 5;
    open_task(&mut s, "b");
    s.update(Msg::Key(named(Named::Tab)));
    s.update(Msg::Key(named(Named::Tab)));
    assert_eq!(
        s.focus,
        Some(Field::TaskTitle),
        "Tab skips the subtask field"
    );
}

/// A 1.8 daemon: "a"'s subtask "s1" has a subtask "s3".
fn with_nested() -> AppState {
    let mut s = with_subtasks();
    s.daemon_minor = 8;
    let mut w = s.workspace.raw().clone();
    w.boards[0].columns[0].tasks[0].subtasks[0].subtasks = vec![v1::Subtask {
        id: "s3".into(),
        title: "Deeper".into(),
        ..Default::default()
    }];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    s
}

#[test]
fn a_subtask_opens_as_a_task_after_saving_its_parent() {
    let mut s = with_nested();
    open_task(&mut s, "a");
    s.update(Msg::Paste(" edited".into()));
    // Opening "s1" saves "a" first, without closing anything.
    let fx = s.update(Msg::Board(BoardMsg::OpenTask("s1".into())));
    assert!(
        matches!(
            &fx[..],
            [Effect::Request { body: Body::UpdateTask(v1::UpdateTaskRequest { task_id, title: Some(t), .. }), followup: Followup::None }]
                if task_id == "a" && t == "A edited"
        ),
        "{fx:?}"
    );
    let Some(Dialog::Task {
        task: Some(t),
        title,
        ..
    }) = &s.dialog
    else {
        panic!("{:?}", s.dialog)
    };
    assert_eq!((t.as_str(), title.text()), ("s1", "First"));
    // Edited like a task; no column move for a subtask.
    s.update(Msg::Board(BoardMsg::AddTag("deep".into())));
    s.update(Msg::PickColumn("done".into()));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::UpdateTask(v1::UpdateTaskRequest { task_id, title: None, tags: Some(_), .. })] if task_id == "s1"
        ),
        "{fx:?}"
    );
    // Deeper still, and deleting a subtask from its editor.
    s.update(Msg::Board(BoardMsg::OpenTask("s3".into())));
    let fx = s.update(Msg::DeleteTask);
    assert!(matches!(
        bodies(&fx)[..],
        [Body::DeleteSubtask(v1::DeleteSubtaskRequest { subtask_id })] if subtask_id == "s3"
    ));
}

#[test]
fn subtasks_are_split_out_and_cards_nested_under_a_chosen_task() {
    let mut s = with_nested();
    let fx = s.update(Msg::Board(BoardMsg::MakeStandalone("s3".into())));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::MoveTask(v1::MoveTaskRequest { task_id, column_id, index: u32::MAX })]
            if task_id == "s3" && column_id == "todo"
    ));
    // "Make subtask of…" lists the other tasks; Enter takes the first match.
    s.update(Msg::OpenMenu(MenuTarget::Task("b".into())));
    s.update(Msg::Board(BoardMsg::OpenNest("b".into())));
    assert_eq!(s.focus, Some(Field::NestQuery));
    s.update(Msg::Paste("deep".into()));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    assert!(
        matches!(
            &fx[..],
            [Effect::Request { body: Body::NestTask(v1::NestTaskRequest { task_id, parent_id, .. }), followup: Followup::CloseDialog }]
                if task_id == "b" && parent_id == "s3"
        ),
        "{fx:?}"
    );
}

#[test]
fn a_subtasks_menu_opens_over_its_tasks_editor() {
    let mut s = with_nested();
    open_task(&mut s, "a");
    s.update(Msg::OpenMenu(MenuTarget::Task("s1".into())));
    assert!(s.menu.is_some());
    // Other menus still don't open over dialogs.
    s.update(Msg::CloseMenu);
    s.update(Msg::OpenMenu(MenuTarget::Column("todo".into())));
    assert!(s.menu.is_none());
    // Threads link to the card and the subtask.
    let found = s.workspace.find_task("s3").unwrap();
    assert_eq!(
        TaskLink::of(&found),
        TaskLink {
            task: "a".into(),
            subtask: Some("s3".into())
        }
    );
}
