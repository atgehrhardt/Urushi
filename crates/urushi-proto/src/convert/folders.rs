//! Projects (`v1::Folder`, ADR 0010) and their settings: core ⇄ wire. Settings arrived
//! in 1.18; a folder from an older peer has none and gets the defaults (no parent
//! branch, parent-branch thread on).

use urushi_core::{BranchName, Project, ProjectBase, ProjectSettings};

use super::{ConvertError, millis_from_proto, millis_to_proto, parse_id};
use crate::v1;

/// Core project → wire (`v1::Folder`).
pub fn project_to_proto(p: &Project) -> v1::Folder {
    v1::Folder {
        id: p.id.to_string(),
        name: p.name.clone(),
        created_at_ms: millis_to_proto(p.created_at),
        settings: Some(project_settings_to_proto(&p.settings)),
    }
}

/// Wire project (`v1::Folder`) → core.
pub fn project_from_proto(f: &v1::Folder) -> Result<Project, ConvertError> {
    Ok(Project {
        id: parse_id(&f.id, "folder.id")?,
        name: f.name.clone(),
        created_at: millis_from_proto(f.created_at_ms),
        settings: f
            .settings
            .as_ref()
            .map(project_settings_from_proto)
            .transpose()?
            .unwrap_or_default(),
    })
}

/// Core project settings → wire.
pub fn project_settings_to_proto(s: &ProjectSettings) -> v1::FolderSettings {
    v1::FolderSettings {
        base_repo_id: s
            .base
            .as_ref()
            .map(|b| b.repo.to_string())
            .unwrap_or_default(),
        base_branch: s
            .base
            .as_ref()
            .map(|b| b.branch.to_string())
            .unwrap_or_default(),
        base_thread: Some(s.base_thread),
    }
}

/// Wire project settings → core. A parent branch needs both its repo and its name; a
/// blank repo means none. The branch must be a valid name, which is all that can be
/// checked without the repo (the daemon turns `origin/x` into `x` before this).
pub fn project_settings_from_proto(
    s: &v1::FolderSettings,
) -> Result<ProjectSettings, ConvertError> {
    let base = if s.base_repo_id.trim().is_empty() {
        None
    } else {
        Some(ProjectBase {
            repo: parse_id(&s.base_repo_id, "folder.settings.base_repo_id")?,
            branch: BranchName::new(s.base_branch.trim().to_owned())
                .map_err(|e| ConvertError::new("folder.settings.base_branch", e))?,
        })
    };
    Ok(ProjectSettings {
        base,
        base_thread: s.base_thread.unwrap_or(true),
    })
}
