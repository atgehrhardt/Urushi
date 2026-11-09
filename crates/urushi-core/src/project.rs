//! A project: a named group of threads, made by the user to organize work.
//!
//! Projects are only organization. A project can hold threads from any repo, and a
//! thread is in at most one project (`Thread::project`). Deleting a project ungroups its
//! threads; it never touches a worktree or branch.

use serde::{Deserialize, Serialize};

use crate::{ProjectId, Timestamp};

/// A user-made group of threads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Identity.
    pub id: ProjectId,
    /// Display name.
    pub name: String,
    /// When it was created.
    pub created_at: Timestamp,
}
