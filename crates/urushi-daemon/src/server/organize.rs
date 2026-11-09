//! Wire → service for organizing threads (projects are `Folder`s on the wire, ADR 0010).

use std::sync::Arc;

use urushi_proto::convert;
use urushi_proto::v1::{self, response::Outcome};

use super::connection::Connection;
use super::handlers::{ok, optional_id, parse};
use crate::service::{Assign, ServiceError, ServiceResult, ThreadUpdate};

/// `UpdateThread`: empty `status` / `folder_id` clear them; unset fields are kept.
pub(super) fn update_thread(
    connection: &Arc<Connection>,
    r: v1::UpdateThreadRequest,
) -> ServiceResult<Outcome> {
    let status = r
        .status
        .map(|s| {
            convert::status_from_proto(&s)
                .map(|s| s.map_or(Assign::Clear, Assign::Set))
                .map_err(|e| ServiceError::Invalid(e.to_string()))
        })
        .transpose()?;
    let project = r
        .folder_id
        .map(|f| optional_id(&f, "folder_id").map(|p| p.map_or(Assign::Clear, Assign::Set)))
        .transpose()?;
    connection.service.update_thread(
        parse(&r.thread_id, "thread_id")?,
        ThreadUpdate {
            title: r.title,
            pinned: r.pinned,
            status,
            project,
        },
    )?;
    Ok(ok())
}
