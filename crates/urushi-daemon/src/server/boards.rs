//! Wire → service for task boards (protocol 1.5; subtasks since 1.6; colors, tags and
//! columns since 1.8).

use urushi_proto::convert;
use urushi_proto::v1::{request::Body, response::Outcome};

use super::handlers::{invalid, ok, parse};
use crate::service::{NewTask, Service, ServiceResult};

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
                &NewTask {
                    title: &r.title,
                    notes: &r.notes,
                    color: convert::color_from_proto(r.color),
                    tags: &r.tags,
                    subtasks: &r.subtasks,
                },
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
            let patch = urushi_core::TaskPatch {
                title: r.title,
                notes: r.notes,
                color: r.color.map(convert::color_from_proto),
                tags: r.tags.map(|t| t.tags),
            };
            service.update_task(parse(&r.task_id, "task_id")?, &patch)?;
            ok()
        }
        Body::CreateColumn(r) => {
            let column =
                service.create_column(parse(&r.board_id, "board_id")?, &r.name, index(r.index))?;
            Outcome::Column(convert::column_to_proto(&column))
        }
        Body::UpdateColumn(r) => {
            service.update_column(
                parse(&r.column_id, "column_id")?,
                r.name.as_deref(),
                r.color.map(convert::color_from_proto),
            )?;
            ok()
        }
        Body::MoveColumn(r) => {
            service.move_column(parse(&r.column_id, "column_id")?, index(r.index))?;
            ok()
        }
        Body::DeleteColumn(r) => {
            service.delete_column(parse(&r.column_id, "column_id")?)?;
            ok()
        }
        Body::MoveTask(r) => {
            service.move_task(
                parse(&r.task_id, "task_id")?,
                parse(&r.column_id, "column_id")?,
                index(r.index),
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

/// A wire position; past the end means last.
fn index(i: u32) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}
