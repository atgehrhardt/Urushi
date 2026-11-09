//! A repo: a git repository the user added to the workspace.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{RepoId, RepoSettingsOverrides, Timestamp};

/// A git repository the user added. Every thread is created in a repo, and gets its own
/// worktree of it. (Organizing threads is what [`crate::Project`]s are for.)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repo {
    /// Identity.
    pub id: RepoId,
    /// Display name. Defaults to the repository's folder name.
    pub name: String,
    /// Root of the main working tree, on the daemon's machine.
    pub repo_root: PathBuf,
    /// Base for new threads (e.g. `main`). `None` uses the repository's current `HEAD`.
    pub default_base: Option<String>,
    /// Per-repo setting overrides.
    #[serde(default)]
    pub settings: RepoSettingsOverrides,
    /// When the repo was added.
    pub added_at: Timestamp,
}
