//! A project: a git repository the user added to the workspace.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{ProjectId, ProjectSettingsOverrides, Timestamp};

/// A git repository the user added. Threads are created inside a project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Identity.
    pub id: ProjectId,
    /// Display name. Defaults to the repository's folder name.
    pub name: String,
    /// Root of the main working tree, on the daemon's machine.
    pub repo_root: PathBuf,
    /// Base for new threads (e.g. `main`). `None` uses the repository's current `HEAD`.
    pub default_base: Option<String>,
    /// Per-project setting overrides.
    #[serde(default)]
    pub settings: ProjectSettingsOverrides,
    /// When the project was added.
    pub added_at: Timestamp,
}
