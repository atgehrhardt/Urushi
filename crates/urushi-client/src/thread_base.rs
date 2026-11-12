//! Asking the daemon to fetch a new thread's base while the user still fills in the New
//! Thread dialog, so creating the thread can reuse the fetch (ADR 0054).

use urushi_proto::v1::{self, request::Body};

use crate::{Client, ClientError};

/// The first protocol minor version with `PrepareThreadBase`.
pub const PREPARE_THREAD_BASE_SINCE_MINOR: u32 = 31;

impl Client {
    /// Has the daemon start fetching the base a thread created with the same `repo_id`,
    /// `base_ref` and `project_id` would start from. Returns at once. Needs protocol
    /// 1.31.
    pub async fn prepare_thread_base(
        &self,
        repo_id: &str,
        base_ref: &str,
        project_id: &str,
    ) -> Result<(), ClientError> {
        let body = Body::PrepareThreadBase(v1::PrepareThreadBaseRequest {
            project_id: repo_id.to_owned(),
            base_ref: base_ref.to_owned(),
            folder_id: project_id.to_owned(),
        });
        self.ok(body, "PrepareThreadBase").await
    }
}
