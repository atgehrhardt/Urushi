//! Writing a new subtask with all its details in an editor of its own (ADR 0025).

use urushi_client::v1::{self, request::Body, response::Outcome};

use super::tasks::{open_task, with_subtasks};
use super::{attached, bodies};
use crate::msg::{BoardMsg, Followup, Msg};
use crate::state::{AppState, Dialog, Field};

/// A 1.14 daemon, task "a" (two subtasks) open in the editor.
fn editing_a() -> AppState {
    let mut s = with_subtasks();
    s.daemon_minor = 14;
    open_task(&mut s, "a");
    s
}

fn on_screen(s: &AppState) -> (&str, bool, usize) {
    match &s.dialog {
        Some(Dialog::Task {
            title,
            nav,
            subtasks,
            ..
        }) => (
            title.text(),
            nav.draft_of_previous.is_some(),
            subtasks.drafts.len(),
        ),
        other => panic!("{other:?}"),
    }
}

fn type_into(s: &mut AppState, field: Field, text: &str) {
    s.update(Msg::FocusField(field));
    s.update(Msg::Text(text.into()));
}

#[test]
fn a_new_subtask_gets_every_detail_and_is_created_with_them_on_save() {
    let mut s = editing_a();
    type_into(&mut s, Field::NewSubtask, "Docs");
    // New subtask… takes what was typed as its title, in an editor of its own.
    assert!(bodies(&s.update(Msg::Board(BoardMsg::NewSubtask))).is_empty());
    assert_eq!(on_screen(&s), ("Docs", true, 0));
    type_into(&mut s, Field::TaskNotes, "Every endpoint");
    s.update(Msg::Board(BoardMsg::PickColor(v1::LabelColor::Teal.into())));
    s.update(Msg::Board(BoardMsg::AddTag("api".into())));
    s.update(Msg::Board(BoardMsg::PickKind(v1::TaskKind::Story.into())));

    // Back (or Done) keeps it in the task's list, unsaved, and it opens as it was left.
    assert!(bodies(&s.update(Msg::Board(BoardMsg::EditorBack))).is_empty());
    assert_eq!(on_screen(&s), ("A", false, 1));
    s.update(Msg::Board(BoardMsg::OpenSubtaskDraft(0)));
    let Some(Dialog::Task { notes, .. }) = &s.dialog else {
        panic!("{:?}", s.dialog);
    };
    assert_eq!(notes.text(), "Every endpoint");

    // Save from the subtask's editor saves its task, with it.
    let fx = s.update(Msg::Submit);
    let [Body::CreateSubtask(r)] = &bodies(&fx)[..] else {
        panic!("{fx:?}");
    };
    assert_eq!(
        (r.task_id.as_str(), r.title.as_str(), r.notes.as_str()),
        ("a", "Docs", "Every endpoint")
    );
    assert_eq!(r.color, i32::from(v1::LabelColor::Teal));
    assert_eq!(r.tags, ["api"]);
    assert_eq!(r.kind, i32::from(v1::TaskKind::Story));
    assert!(s.dialog.is_none(), "closes once saved");
}

#[test]
fn a_new_tasks_detailed_subtasks_go_with_it() {
    let mut s = with_subtasks();
    s.daemon_minor = 14;
    s.update(Msg::NewTask {
        board: "b".into(),
        column: "todo".into(),
    });
    s.update(Msg::Text("Plan".into()));
    type_into(&mut s, Field::NewSubtask, "quick");
    s.update(Msg::Key(super::named(crate::keys::Named::Enter)));
    s.update(Msg::Board(BoardMsg::NewSubtask));
    assert_eq!(on_screen(&s), ("", true, 0), "nothing typed: no title yet");
    // A blank title can't be saved.
    assert!(s.update(Msg::Submit).is_empty());
    type_into(&mut s, Field::TaskTitle, "detailed");
    type_into(&mut s, Field::TaskNotes, "with notes");
    let fx = s.update(Msg::Submit);
    let [Body::CreateTask(r)] = &bodies(&fx)[..] else {
        panic!("{fx:?}");
    };
    assert!(r.subtasks.is_empty(), "sent with their details instead");
    let sent: Vec<_> = r
        .subtask_details
        .iter()
        .map(|d| (d.title.as_str(), d.notes.as_str()))
        .collect();
    assert_eq!(sent, [("quick", ""), ("detailed", "with notes")]);
}

#[test]
fn an_empty_new_subtask_is_dropped_and_one_can_be_removed() {
    let mut s = editing_a();
    s.update(Msg::Board(BoardMsg::NewSubtask));
    s.update(Msg::Board(BoardMsg::EditorBack));
    assert_eq!(
        on_screen(&s),
        ("A", false, 0),
        "nothing written: nothing kept"
    );
    s.update(Msg::Board(BoardMsg::NewSubtask));
    type_into(&mut s, Field::TaskTitle, "Gone");
    s.update(Msg::Board(BoardMsg::RemoveSubtaskDraft));
    assert_eq!(on_screen(&s), ("A", false, 0));
    assert!(!s.editor_dirty(), "nothing left to save");
}

#[test]
fn opening_another_task_keeps_the_new_subtask_in_its_list() {
    let mut s = editing_a();
    s.update(Msg::Board(BoardMsg::NewSubtask));
    type_into(&mut s, Field::TaskTitle, "Kept");
    s.update(Msg::Board(BoardMsg::OpenTask("s1".into())));
    let Some(Dialog::Task { nav, .. }) = &s.dialog else {
        panic!("{:?}", s.dialog);
    };
    let [Dialog::Task { subtasks, .. }] = nav.back.as_slice() else {
        panic!("{:?}", nav.back);
    };
    assert!(
        subtasks
            .drafts
            .iter()
            .map(crate::drafts::SubtaskDraft::title)
            .eq(["Kept"])
    );
}

#[test]
fn older_daemons_get_titles_and_no_subtask_editor() {
    let mut s = with_subtasks(); // 1.6
    open_task(&mut s, "a");
    assert!(!s.daemon_supports_subtask_details());
    s.update(Msg::NewTask {
        board: "b".into(),
        column: "todo".into(),
    });
    s.update(Msg::Text("Plan".into()));
    type_into(&mut s, Field::NewSubtask, "one");
    s.update(Msg::Key(super::named(crate::keys::Named::Enter)));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(&bodies(&fx)[..], [Body::CreateTask(r)] if r.subtasks == ["one"] && r.subtask_details.is_empty()),
        "{fx:?}"
    );
}

#[test]
fn a_jira_subtask_is_written_with_the_sub_task_types_fields() {
    let mut s = attached();
    s.daemon_minor = 14;
    let mut workspace = super::workspace();
    workspace.boards = vec![super::jira::jira_board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s.update(Msg::Board(BoardMsg::OpenTask("j1".into())));
    let fx = s.update(Msg::Board(BoardMsg::NewSubtask));
    assert!(
        matches!(&bodies(&fx)[..], [Body::GetJiraCreateFields(r)] if r.kind == i32::from(v1::TaskKind::Subtask)),
        "{fx:?}"
    );
    s.update(Msg::Response {
        followup: Followup::JiraCreateFields(v1::TaskKind::Subtask.into()),
        result: Ok(Outcome::JiraIssueDetails(v1::JiraIssueDetails {
            fields: vec![v1::JiraField {
                id: "duedate".into(),
                name: "Due date".into(),
                kind: v1::JiraFieldKind::Date.into(),
                required: true,
                ..Default::default()
            }],
            ..Default::default()
        })),
    });
    type_into(&mut s, Field::TaskTitle, "Fix flaky test");
    assert!(s.update(Msg::Submit).is_empty());
    assert_eq!(
        s.error.as_deref(),
        Some("Fill in Due date of the subtask \u{201c}Fix flaky test\u{201d} first.")
    );
    s.update(Msg::Board(BoardMsg::OpenSubtaskDraft(0)));
    s.update(Msg::FocusField(Field::JiraText(0)));
    s.update(Msg::Text("2026-12-01".into()));
    let fx = s.update(Msg::Submit);
    let created: Vec<_> = bodies(&fx)
        .into_iter()
        .filter_map(|b| match b {
            Body::CreateSubtask(r) => Some(r),
            _ => None,
        })
        .collect();
    let [r] = created.as_slice() else {
        panic!("{fx:?}");
    };
    assert_eq!(r.task_id, "j1");
    assert!(
        matches!(&r.jira_fields[..], [f] if f.field_id == "duedate" && f.value.as_ref().is_some_and(|v| v.text == "2026-12-01")),
        "{r:?}"
    );
}
