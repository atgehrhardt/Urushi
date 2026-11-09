//! The task editor's subtasks, starting threads from tasks and subtasks, and where a
//! dragged task would land.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_core::{RepoId, ThreadId};

use super::board::board;
use super::{attached, bodies, named};
use crate::keys::Named;
use crate::msg::{Followup, Msg};
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
    // Renaming: click the title, edit, Enter.
    s.update(Msg::RenameSubtask("s1".into()));
    assert_eq!(s.focus, Some(Field::SubtaskTitle));
    s.update(Msg::Text(" one".into()));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    assert!(matches!(
        bodies(&fx)[..],
        [Body::UpdateSubtask(v1::UpdateSubtaskRequest { title: Some(t), done: None, .. })]
            if t == "First one"
    ));
    assert_eq!(s.focus, Some(Field::NewSubtask));
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
