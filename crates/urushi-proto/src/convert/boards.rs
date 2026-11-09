//! Task boards: core ⇄ wire (`v1::Board`, since protocol 1.5; subtasks and thread links
//! since 1.6).

use urushi_core::{Board, Column, Subtask, Task, ThreadId};

use super::{ConvertError, millis_from_proto, millis_to_proto, parse_id};
use crate::v1;

/// Core board → wire.
pub fn board_to_proto(b: &Board) -> v1::Board {
    v1::Board {
        id: b.id.to_string(),
        name: b.name.clone(),
        created_at_ms: millis_to_proto(b.created_at),
        columns: b
            .columns
            .iter()
            .map(|c| v1::BoardColumn {
                id: c.id.to_string(),
                name: c.name.clone(),
                tasks: c.tasks.iter().map(task_to_proto).collect(),
            })
            .collect(),
    }
}

/// Core task → wire (also the answer to `CreateTask`).
pub fn task_to_proto(t: &Task) -> v1::Task {
    v1::Task {
        id: t.id.to_string(),
        title: t.title.clone(),
        notes: t.notes.clone(),
        created_at_ms: millis_to_proto(t.created_at),
        subtasks: t.subtasks.iter().map(subtask_to_proto).collect(),
        thread_id: link_to_proto(t.thread),
    }
}

/// Core subtask → wire (also the answer to `CreateSubtask`).
pub fn subtask_to_proto(s: &Subtask) -> v1::Subtask {
    v1::Subtask {
        id: s.id.to_string(),
        title: s.title.clone(),
        done: s.done,
        thread_id: link_to_proto(s.thread),
    }
}

fn link_to_proto(thread: Option<ThreadId>) -> String {
    thread.map(|t| t.to_string()).unwrap_or_default()
}

fn link_from_proto(id: &str, field: &'static str) -> Result<Option<ThreadId>, ConvertError> {
    if id.is_empty() {
        Ok(None)
    } else {
        parse_id(id, field).map(Some)
    }
}

fn task_from_proto(t: &v1::Task) -> Result<Task, ConvertError> {
    Ok(Task {
        id: parse_id(&t.id, "board.task.id")?,
        title: t.title.clone(),
        notes: t.notes.clone(),
        created_at: millis_from_proto(t.created_at_ms),
        subtasks: t
            .subtasks
            .iter()
            .map(|s| {
                Ok(Subtask {
                    id: parse_id(&s.id, "board.task.subtask.id")?,
                    title: s.title.clone(),
                    done: s.done,
                    thread: link_from_proto(&s.thread_id, "board.task.subtask.thread_id")?,
                })
            })
            .collect::<Result<_, ConvertError>>()?,
        thread: link_from_proto(&t.thread_id, "board.task.thread_id")?,
    })
}

/// Wire board → core. Ids are validated; text is taken as sent.
pub fn board_from_proto(b: &v1::Board) -> Result<Board, ConvertError> {
    Ok(Board {
        id: parse_id(&b.id, "board.id")?,
        name: b.name.clone(),
        created_at: millis_from_proto(b.created_at_ms),
        columns: b
            .columns
            .iter()
            .map(|c| {
                Ok(Column {
                    id: parse_id(&c.id, "board.column.id")?,
                    name: c.name.clone(),
                    tasks: c
                        .tasks
                        .iter()
                        .map(task_from_proto)
                        .collect::<Result<_, ConvertError>>()?,
                })
            })
            .collect::<Result<_, ConvertError>>()?,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use prost::Message;
    use urushi_core::{BoardId, ColumnId, SubtaskId, TaskId, ThreadId, Timestamp};

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
        let (parent, sub) = (TaskId::from_u128(5), SubtaskId::from_u128(6));
        board
            .add_subtask(parent, Subtask::new(sub, "step one").unwrap())
            .unwrap();
        board
            .add_subtask(
                parent,
                Subtask::new(SubtaskId::from_u128(7), "two").unwrap(),
            )
            .unwrap();
        board.update_subtask(sub, None, Some(true)).unwrap();
        board
            .link_thread(parent, None, ThreadId::from_u128(8))
            .unwrap();
        board
            .link_thread(parent, Some(sub), ThreadId::from_u128(9))
            .unwrap();
        board
    }

    #[test]
    fn boards_round_trip() {
        let board = sample();
        assert_eq!(board_from_proto(&board_to_proto(&board)).unwrap(), board);
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

    /// Every 1.5 and 1.6 board message survives encode → decode.
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
        for body in bodies {
            let request = v1::Request {
                id: 7,
                body: Some(body),
            };
            let decoded = v1::Request::decode(request.encode_to_vec().as_slice()).unwrap();
            assert_eq!(decoded, request);
        }
        let outcomes = [
            Outcome::Board(board_to_proto(&sample())),
            Outcome::Task(board_to_proto(&sample()).columns[1].tasks[0].clone()),
            Outcome::Subtask(board_to_proto(&sample()).columns[1].tasks[0].subtasks[0].clone()),
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
        // An unset `notes` is "keep", which differs from an empty one.
        let keep = v1::UpdateTaskRequest {
            task_id: "3".into(),
            title: None,
            notes: None,
        };
        let decoded = v1::UpdateTaskRequest::decode(keep.encode_to_vec().as_slice()).unwrap();
        assert!(decoded.notes.is_none());
        // Likewise an unset `done` keeps the check mark, unlike `false`.
        let keep = v1::UpdateSubtaskRequest {
            subtask_id: "4".into(),
            title: Some("x".into()),
            done: None,
        };
        let decoded = v1::UpdateSubtaskRequest::decode(keep.encode_to_vec().as_slice()).unwrap();
        assert!(decoded.done.is_none());
    }
}
