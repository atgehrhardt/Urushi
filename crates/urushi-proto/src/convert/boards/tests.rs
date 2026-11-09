//! Board conversions and wire round trips.
#![allow(clippy::unwrap_used, reason = "test code")]

use prost::Message;
use urushi_core::{BoardId, ColumnId, TaskId, ThreadId, Timestamp};

use super::*;
use crate::v1::{request::Body, response::Outcome};

fn sample() -> Board {
    let mut board = Board::new(
        BoardId::from_u128(1),
        "Sprint",
        Timestamp::from_unix_millis(10),
        [2, 3, 4].map(ColumnId::from_u128),
    )
    .unwrap();
    let task = Task::new(
        TaskId::from_u128(5),
        "Ship it",
        "notes\nmore",
        Timestamp::from_unix_millis(20),
    )
    .unwrap();
    board.add_task(ColumnId::from_u128(3), task).unwrap();
    let (parent, sub) = (TaskId::from_u128(5), TaskId::from_u128(6));
    let step = |id: u128, title: &str, notes: &str| {
        Task::new(
            TaskId::from_u128(id),
            title,
            notes,
            Timestamp::from_unix_millis(30),
        )
        .unwrap()
    };
    board.add_subtask(parent, step(6, "step one", "")).unwrap();
    board
        .add_subtask(parent, step(7, "two", "its notes"))
        .unwrap();
    board
        .add_subtask(TaskId::from_u128(7), step(11, "deeper", ""))
        .unwrap();
    board
        .update_task(
            sub,
            &urushi_core::TaskPatch {
                done: Some(true),
                color: Some(Some(urushi_core::LabelColor::Pink)),
                tags: Some(vec!["nested".into()]),
                ..Default::default()
            },
        )
        .unwrap();
    board
        .update_task(
            parent,
            &urushi_core::TaskPatch {
                color: Some(Some(urushi_core::LabelColor::Orange)),
                tags: Some(vec!["ui".into(), "Bug fix".into()]),
                ..Default::default()
            },
        )
        .unwrap();
    board
        .update_column(
            ColumnId::from_u128(4),
            None,
            Some(Some(urushi_core::LabelColor::Green)),
        )
        .unwrap();
    board.link_thread(parent, ThreadId::from_u128(8)).unwrap();
    board.link_thread(sub, ThreadId::from_u128(9)).unwrap();
    board
        .link_thread(TaskId::from_u128(11), ThreadId::from_u128(10))
        .unwrap();
    board
}

#[test]
fn boards_round_trip() {
    let board = sample();
    assert_eq!(board_from_proto(&board_to_proto(&board)).unwrap(), board);
}

#[test]
fn subtasks_carry_every_task_field_at_any_depth() {
    let wire = board_to_proto(&sample());
    let task = &wire.columns[1].tasks[0];
    let (one, two) = (&task.subtasks[0], &task.subtasks[1]);
    assert!(one.done);
    assert_eq!(one.tags, ["nested"]);
    assert_eq!(one.color, i32::from(v1::LabelColor::Pink));
    assert_eq!(two.notes, "its notes");
    assert_eq!(two.subtasks[0].title, "deeper");
    assert!(!two.subtasks[0].thread_id.is_empty());
}

#[test]
fn bad_ids_are_rejected() {
    let mut wire = board_to_proto(&sample());
    wire.columns[1].tasks[0].id = "nope".into();
    assert_eq!(board_from_proto(&wire).unwrap_err().field, "board.task.id");
    let mut wire = board_to_proto(&sample());
    wire.columns[1].tasks[0].subtasks[1].thread_id = "nope".into();
    assert_eq!(
        board_from_proto(&wire).unwrap_err().field,
        "board.task.subtask.thread_id"
    );
}

#[test]
fn unlinked_tasks_have_empty_thread_ids() {
    let wire = board_to_proto(&sample());
    let task = &wire.columns[1].tasks[0];
    assert!(!task.thread_id.is_empty());
    assert_eq!(task.subtasks[1].thread_id, "");
    assert!(task.subtasks[0].done);
}

/// Every 1.5, 1.6 and 1.8 board message survives encode → decode.
#[test]
fn messages_round_trip_on_the_wire() {
    let bodies = [
        Body::CreateBoard(v1::CreateBoardRequest { name: "b".into() }),
        Body::RenameBoard(v1::RenameBoardRequest {
            board_id: "1".into(),
            name: "c".into(),
        }),
        Body::DeleteBoard(v1::DeleteBoardRequest {
            board_id: "1".into(),
        }),
        Body::CreateTask(v1::CreateTaskRequest {
            board_id: "1".into(),
            column_id: "2".into(),
            title: "t".into(),
            notes: "n".into(),
            subtasks: vec!["s1".into(), "s2".into()],
            color: v1::LabelColor::Teal.into(),
            tags: vec!["ui".into()],
        }),
        Body::CreateSubtask(v1::CreateSubtaskRequest {
            task_id: "3".into(),
            title: "s".into(),
        }),
        Body::UpdateSubtask(v1::UpdateSubtaskRequest {
            subtask_id: "4".into(),
            title: None,
            done: Some(false),
        }),
        Body::DeleteSubtask(v1::DeleteSubtaskRequest {
            subtask_id: "4".into(),
        }),
        Body::UpdateTask(v1::UpdateTaskRequest {
            task_id: "3".into(),
            title: None,
            notes: Some(String::new()),
            color: Some(v1::LabelColor::Unspecified.into()),
            tags: Some(v1::TagList {
                tags: vec!["a".into(), "b".into()],
            }),
        }),
        Body::MoveTask(v1::MoveTaskRequest {
            task_id: "3".into(),
            column_id: "2".into(),
            index: 4,
        }),
        Body::DeleteTask(v1::DeleteTaskRequest {
            task_id: "3".into(),
        }),
        Body::GetAgentUsage(v1::GetAgentUsageRequest {
            pane_id: "p".into(),
        }),
    ];
    requests_round_trip(bodies);
    let outcomes = [
        Outcome::Board(board_to_proto(&sample())),
        Outcome::Task(board_to_proto(&sample()).columns[1].tasks[0].clone()),
        Outcome::Subtask(board_to_proto(&sample()).columns[1].tasks[0].subtasks[0].clone()),
        Outcome::Column(board_to_proto(&sample()).columns[1].clone()),
        Outcome::AgentUsage(v1::AgentUsage {
            agent: v1::Agent::Claude.into(),
            plan: "Max 20x".into(),
            windows: vec![v1::UsageWindow {
                label: "5h".into(),
                used_percent: 42.5,
                resets_at_ms: 1_000,
            }],
            unavailable: String::new(),
            fetched_at_ms: 900,
        }),
    ];
    for outcome in outcomes {
        let response = v1::Response {
            id: 7,
            outcome: Some(outcome),
        };
        let decoded = v1::Response::decode(response.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, response);
    }
}

/// Unset optional fields keep a value; set ones (even empty) replace it.
#[test]
fn unset_fields_are_told_apart_from_empty_ones() {
    // An unset `notes` is "keep", which differs from an empty one.
    let keep = v1::UpdateTaskRequest {
        task_id: "3".into(),
        title: None,
        notes: None,
        color: None,
        tags: None,
    };
    let decoded = v1::UpdateTaskRequest::decode(keep.encode_to_vec().as_slice()).unwrap();
    assert!(decoded.notes.is_none());
    // Unset color and tags keep them; a set "unspecified" color and empty tags clear them.
    assert!(decoded.color.is_none() && decoded.tags.is_none());
    let clear = v1::UpdateTaskRequest {
        color: Some(0),
        tags: Some(v1::TagList::default()),
        ..keep
    };
    let decoded = v1::UpdateTaskRequest::decode(clear.encode_to_vec().as_slice()).unwrap();
    assert_eq!(decoded.color, Some(0));
    assert!(decoded.tags.is_some_and(|t| t.tags.is_empty()));
    // Likewise an unset `done` keeps the check mark, unlike `false`.
    let keep = v1::UpdateSubtaskRequest {
        subtask_id: "4".into(),
        title: Some("x".into()),
        done: None,
    };
    let decoded = v1::UpdateSubtaskRequest::decode(keep.encode_to_vec().as_slice()).unwrap();
    assert!(decoded.done.is_none());
}

fn requests_round_trip(bodies: impl IntoIterator<Item = Body>) {
    for body in bodies {
        let request = v1::Request {
            id: 7,
            body: Some(body),
        };
        let decoded = v1::Request::decode(request.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, request);
    }
}

/// The 1.8 column requests survive encode → decode.
#[test]
fn column_requests_round_trip_on_the_wire() {
    requests_round_trip([
        Body::CreateColumn(v1::CreateColumnRequest {
            board_id: "1".into(),
            name: "Review".into(),
            index: 2,
        }),
        Body::UpdateColumn(v1::UpdateColumnRequest {
            column_id: "2".into(),
            name: None,
            color: Some(v1::LabelColor::Purple.into()),
        }),
        Body::MoveColumn(v1::MoveColumnRequest {
            column_id: "2".into(),
            index: 0,
        }),
        Body::DeleteColumn(v1::DeleteColumnRequest {
            column_id: "2".into(),
        }),
        Body::NestTask(v1::NestTaskRequest {
            task_id: "3".into(),
            parent_id: "5".into(),
            index: 1,
        }),
    ]);
}
