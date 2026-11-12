//! Wire → service for creating threads, and preparing for it (ADR 0054).

use std::sync::Arc;

use urushi_proto::convert;
use urushi_proto::v1::{self, response::Outcome};

use super::handlers::{ok, optional_id, parse};
use crate::service::{Service, ServiceResult, ThreadRequest};

/// Empty strings on the wire mean "not given".
fn given(value: String) -> Option<String> {
    Some(value).filter(|v| !v.trim().is_empty())
}

/// `CreateThread` → the new `Thread`.
pub(super) async fn create_thread(
    service: &Arc<Service>,
    r: v1::CreateThreadRequest,
) -> ServiceResult<Outcome> {
    let request = ThreadRequest {
        title: r.title,
        base_ref: given(r.base_ref),
        branch: given(r.branch),
        initial_tab: !r.skip_initial_tab,
        project: optional_id(&r.folder_id, "folder_id")?,
        task: match optional_id(&r.task_id, "task_id")? {
            Some(task) => Some((task, optional_id(&r.subtask_id, "subtask_id")?)),
            None => None,
        },
        start_task_agent: r.start_task_agent,
    };
    let thread = service
        .create_thread(parse(&r.project_id, "project_id")?, request)
        .await?;
    Ok(Outcome::Thread(convert::thread_to_proto(&thread)))
}

/// `PrepareThreadBase` → `Ok` at once; the fetch runs in the background.
pub(super) fn prepare_thread_base(
    service: &Arc<Service>,
    r: v1::PrepareThreadBaseRequest,
) -> ServiceResult<Outcome> {
    service.prepare_thread_base(
        parse(&r.project_id, "project_id")?,
        given(r.base_ref),
        optional_id(&r.folder_id, "folder_id")?,
    )?;
    Ok(ok())
}
