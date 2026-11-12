//! Wire → service for organizing threads (projects are `Folder`s on the wire, ADR 0010).

use std::sync::Arc;

use urushi_proto::convert;
use urushi_proto::v1::{self, response::Outcome};

use super::connection::Connection;
use super::handlers::{ok, optional_id, parse};
use crate::service::{
    Assign, ProjectSettingsRequest, Service, ServiceError, ServiceResult, ThreadUpdate,
};

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
            unread: r.unread,
            notes: r.notes,
        },
    )?;
    Ok(ok())
}

/// `SetFolderColor`: an unspecified (or unknown) color clears it.
pub(super) fn set_folder_color(
    service: &Service,
    r: &v1::SetFolderColorRequest,
) -> ServiceResult<Outcome> {
    service.set_project_color(
        parse(&r.folder_id, "folder_id")?,
        convert::color_from_proto(r.color),
    )?;
    Ok(ok())
}

/// `UpdateFolderSettings`: replaces a project's settings. A blank repo means no parent
/// branch; an unset `base_thread` means on.
pub(super) async fn update_folder_settings(
    service: &Arc<Service>,
    r: v1::UpdateFolderSettingsRequest,
) -> ServiceResult<Outcome> {
    let settings = r.settings.unwrap_or_default();
    let base = match optional_id(&settings.base_repo_id, "settings.base_repo_id")? {
        Some(repo) if !settings.base_branch.trim().is_empty() => Some((repo, settings.base_branch)),
        Some(_) => {
            return Err(ServiceError::Invalid(
                "a parent branch needs a branch name".into(),
            ));
        }
        None => None,
    };
    let project = service
        .update_project_settings(
            parse(&r.folder_id, "folder_id")?,
            ProjectSettingsRequest {
                base,
                base_thread: settings.base_thread.unwrap_or(true),
            },
        )
        .await?;
    Ok(Outcome::Folder(convert::project_to_proto(&project)))
}
