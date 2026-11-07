//! Reading and changing settings.

use std::path::PathBuf;

use urushi_core::{BranchPrefix, ProjectId, ProjectSettingsOverrides, Settings};
use urushi_proto::{convert, v1};

use super::{Service, ServiceError, ServiceResult};

impl Service {
    /// Global settings.
    pub fn settings(&self) -> Settings {
        self.read(|s| Ok(s.settings.clone())).unwrap_or_default()
    }

    /// Applies a partial update. Unset fields stay as they are. Returns the new settings.
    pub fn update_settings(&self, patch: &v1::SettingsPatch) -> ServiceResult<Settings> {
        self.mutate(|state| {
            let mut next = state.settings.clone();
            apply_patch(&mut next, patch)?;
            state.settings = next.clone();
            Ok(next)
        })
    }

    /// Replaces a project's overrides.
    pub fn update_project_settings(
        &self,
        project: ProjectId,
        overrides: ProjectSettingsOverrides,
    ) -> ServiceResult<()> {
        self.mutate(|state| {
            state.project_mut(project)?.settings = overrides;
            Ok(())
        })
    }
}

fn apply_patch(settings: &mut Settings, patch: &v1::SettingsPatch) -> ServiceResult<()> {
    if let Some(v) = patch.delete_remote_branches {
        settings.delete_remote_branches = v;
    }
    if let Some(prefix) = &patch.branch_prefix {
        settings.branch_prefix =
            BranchPrefix::new(prefix.clone()).map_err(|e| ServiceError::Invalid(e.to_string()))?;
    }
    if let Some(shell) = &patch.default_shell {
        settings.default_shell = Some(shell.clone()).filter(|s| !s.trim().is_empty());
    }
    if let Some(root) = &patch.worktree_root {
        settings.worktree_root = Some(PathBuf::from(root)).filter(|p| !p.as_os_str().is_empty());
    }
    if let Some(serve) = patch.tailnet_serve {
        settings.tailnet.serve = serve;
    }
    if let Some(port) = patch.tailnet_port {
        settings.tailnet.port =
            convert::port_from_proto(port).map_err(|e| ServiceError::Invalid(e.to_string()))?;
    }
    if patch.set_allowed_logins {
        settings.tailnet.allowed_logins = patch
            .allowed_logins
            .iter()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty())
            .collect();
    }
    if let Some(v) = patch.allow_tagged_nodes {
        settings.tailnet.allow_tagged_nodes = v;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_changes_only_set_fields() {
        let mut s = Settings::default();
        let patch = v1::SettingsPatch {
            delete_remote_branches: Some(false),
            tailnet_port: Some(9000),
            set_allowed_logins: true,
            allowed_logins: vec![" me@example.com ".into(), String::new()],
            ..Default::default()
        };
        apply_patch(&mut s, &patch).unwrap();
        assert!(!s.delete_remote_branches);
        assert_eq!(s.tailnet.port, 9000);
        assert_eq!(s.tailnet.allowed_logins, vec!["me@example.com"]);
        assert_eq!(s.branch_prefix.as_str(), "urushi/", "untouched");
    }

    #[test]
    fn invalid_values_are_rejected() {
        let mut s = Settings::default();
        let bad_prefix = v1::SettingsPatch {
            branch_prefix: Some("a..b/".into()),
            ..Default::default()
        };
        assert!(apply_patch(&mut s, &bad_prefix).is_err());
        let bad_port = v1::SettingsPatch {
            tailnet_port: Some(70_000),
            ..Default::default()
        };
        assert!(apply_patch(&mut s, &bad_port).is_err());
    }
}
