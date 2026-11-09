//! Wire → service for task boards (protocol 1.5; subtasks since 1.6).

use urushi_proto::convert;
use urushi_proto::v1::{request::Body, response::Outcome};

use super::handlers::{invalid, ok, parse};
use crate::service::{Service, ServiceResult};

/// Handles the board requests; anything else is refused as a routing bug.
pub(super) fn route(service: &Service, body: Body) -> ServiceResult<Outcome> {
    Ok(match body {
        Body::CreateBoard(r) => {
            Outcome::Board(convert::board_to_proto(&service.create_board(&r.name)?))
        }
        Body::RenameBoard(r) => {
            service.rename_board(parse(&r.board_id, "board_id")?, &r.name)?;
            ok()
        }
        Body::DeleteBoard(r) => {
            service.delete_board(parse(&r.board_id, "board_id")?)?;
            ok()
        }
        Body::CreateTask(r) => {
            let task = service.create_task(
                parse(&r.board_id, "board_id")?,
                parse(&r.column_id, "column_id")?,
                (&r.title, &r.notes),
                &r.subtasks,
            )?;
            Outcome::Task(convert::task_to_proto(&task))
        }
        Body::CreateSubtask(r) => {
            let subtask = service.create_subtask(parse(&r.task_id, "task_id")?, &r.title)?;
            Outcome::Subtask(convert::subtask_to_proto(&subtask))
        }
        Body::UpdateSubtask(r) => {
            service.update_subtask(
                parse(&r.subtask_id, "subtask_id")?,
                r.title.as_deref(),
                r.done,
            )?;
            ok()
        }
        Body::DeleteSubtask(r) => {
            service.delete_subtask(parse(&r.subtask_id, "subtask_id")?)?;
            ok()
        }
        Body::UpdateTask(r) => {
            service.update_task(
                parse(&r.task_id, "task_id")?,
                r.title.as_deref(),
                r.notes.as_deref(),
            )?;
            ok()
        }
        Body::MoveTask(r) => {
            service.move_task(
                parse(&r.task_id, "task_id")?,
                parse(&r.column_id, "column_id")?,
                usize::try_from(r.index).unwrap_or(usize::MAX),
            )?;
            ok()
        }
        Body::DeleteTask(r) => {
            service.delete_task(parse(&r.task_id, "task_id")?)?;
            ok()
        }
        _ => invalid("not a board request"),
    })
}
