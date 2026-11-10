//! Project settings (ADR 0029): a project's parent branch and its parent-branch thread.
//! On the wire a project is a `Folder` (ADR 0010).

use urushi_proto::v1::{self, request::Body, response::Outcome};

use crate::requests::unexpected;
use crate::{Client, ClientError};

impl Client {
    /// Replaces a project's settings and returns the project. Needs protocol 1.18.
    ///
    /// Switching the parent-branch thread on (with a parent branch) opens it. A change
    /// that would need an open one closed is refused with `FAILED_PRECONDITION`; close
    /// it first with [`Client::plan_thread_deletion`] and [`Client::delete_thread`].
    pub async fn update_project_settings(
        &self,
        project_id: &str,
        settings: v1::FolderSettings,
    ) -> Result<v1::Folder, ClientError> {
        let body = Body::UpdateFolderSettings(v1::UpdateFolderSettingsRequest {
            folder_id: project_id.into(),
            settings: Some(settings),
        });
        match self.request(body).await? {
            Outcome::Folder(folder) => Ok(folder),
            _ => Err(unexpected("UpdateFolderSettings")),
        }
    }
}
