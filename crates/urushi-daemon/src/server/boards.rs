//! Wire → service for task boards (protocol 1.5; subtasks since 1.6; colors, tags,
//! columns and nested tasks since 1.8; kinds and Jira boards since 1.12). Task changes
//! are async: on a Jira board they are written to Jira first.

use urushi_proto::convert;
use urushi_proto::v1::{self, request::Body, response::Outcome};

use super::handlers::{invalid, ok, parse};
use crate::service::{AttachmentData, NewTask, Service, ServiceResult};

/// Handles the board requests; anything else is refused as a routing bug.
pub(super) async fn route(service: &Service, body: Body) -> ServiceResult<Outcome> {
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
            let task = service
                .create_task(
                    parse(&r.board_id, "board_id")?,
                    parse(&r.column_id, "column_id")?,
                    &NewTask {
                        title: &r.title,
                        notes: &r.notes,
                        color: convert::color_from_proto(r.color),
                        tags: &r.tags,
                        subtasks: &r.subtasks,
                        kind: convert::kind_from_proto(r.kind),
                    },
                )
                .await?;
            Outcome::Task(convert::task_to_proto(&task))
        }
        Body::CreateSubtask(r) => {
            let subtask = service
                .create_subtask(parse(&r.task_id, "task_id")?, &r.title)
                .await?;
            Outcome::Subtask(convert::subtask_to_proto(&subtask))
        }
        // Since 1.8 a subtask is a task; these stay for 1.6 clients.
        Body::UpdateSubtask(r) => {
            let patch = urushi_core::TaskPatch {
                title: r.title,
                done: r.done,
                ..Default::default()
            };
            service
                .update_task(parse(&r.subtask_id, "subtask_id")?, &patch)
                .await?;
            ok()
        }
        Body::DeleteSubtask(r) => {
            service.delete_task(parse(&r.subtask_id, "subtask_id")?)?;
            ok()
        }
        Body::NestTask(r) => {
            service.nest_task(
                parse(&r.task_id, "task_id")?,
                parse(&r.parent_id, "parent_id")?,
                index(r.index),
            )?;
            ok()
        }
        Body::UpdateTask(r) => {
            let patch = urushi_core::TaskPatch {
                title: r.title,
                notes: r.notes,
                color: r.color.map(convert::color_from_proto),
                tags: r.tags.map(|t| t.tags),
                done: None,
                kind: r.kind.map(convert::kind_from_proto),
            };
            service
                .update_task(parse(&r.task_id, "task_id")?, &patch)
                .await?;
            ok()
        }
        body @ (Body::CreateColumn(_)
        | Body::UpdateColumn(_)
        | Body::MoveColumn(_)
        | Body::DeleteColumn(_)) => columns(service, body)?,
        Body::MoveTask(r) => {
            service
                .move_task(
                    parse(&r.task_id, "task_id")?,
                    parse(&r.column_id, "column_id")?,
                    index(r.index),
                )
                .await?;
            ok()
        }
        Body::DeleteTask(r) => {
            service.delete_task(parse(&r.task_id, "task_id")?)?;
            ok()
        }
        _ => invalid("not a board request"),
    })
}

/// Column requests (since 1.8).
fn columns(service: &Service, body: Body) -> ServiceResult<Outcome> {
    Ok(match body {
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
        _ => invalid("not a column request"),
    })
}

/// A wire position; past the end means last.
fn index(i: u32) -> usize {
    usize::try_from(i).unwrap_or(usize::MAX)
}

/// Attachments (since 1.8): their file IO runs off the runtime, so these are async.
pub(super) async fn route_attachments(service: &Service, body: Body) -> ServiceResult<Outcome> {
    Ok(match body {
        Body::AddAttachment(r) => {
            let attachment = service
                .add_attachment(parse(&r.task_id, "task_id")?, &r.name, r.data)
                .await?;
            Outcome::Attachment(convert::attachment_to_proto(&attachment))
        }
        Body::GetAttachment(r) => {
            let id = parse(&r.attachment_id, "attachment_id")?;
            let data = match service.get_attachment(id, r.thumbnail).await? {
                AttachmentData::File { media_type, bytes } => v1::AttachmentData {
                    attachment_id: r.attachment_id,
                    media_type,
                    data: bytes,
                    width: 0,
                    height: 0,
                },
                AttachmentData::Thumbnail {
                    width,
                    height,
                    rgba,
                } => v1::AttachmentData {
                    attachment_id: r.attachment_id,
                    media_type: "image/x-rgba".into(),
                    data: rgba,
                    width,
                    height,
                },
            };
            Outcome::AttachmentData(data)
        }
        Body::DeleteAttachment(r) => {
            service.delete_attachment(parse(&r.attachment_id, "attachment_id")?)?;
            ok()
        }
        _ => invalid("not an attachment request"),
    })
}
