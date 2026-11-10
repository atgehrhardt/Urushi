//! Reading and changing settings.

use std::path::PathBuf;

use urushi_core::agent::MAX_TASK_MESSAGE;
use urushi_core::{Agent, BranchPrefix, PromptDelivery, RepoId, RepoSettingsOverrides, Settings};
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

    /// Replaces a repo's overrides.
    pub fn update_repo_settings(
        &self,
        repo: RepoId,
        overrides: RepoSettingsOverrides,
    ) -> ServiceResult<()> {
        self.mutate(|state| {
            state.repo_mut(repo)?.settings = overrides;
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
    if let Some(v) = patch.update_base_from_remote {
        settings.update_base_from_remote = v;
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
    for (agent, skip) in [
        (Agent::Claude, patch.claude_skip_permissions),
        (Agent::Codex, patch.codex_skip_permissions),
        (Agent::Cursor, patch.cursor_skip_permissions),
    ] {
        if let Some(skip) = skip {
            settings.agents.set_skip_permissions(agent, skip);
        }
    }
    if let Some(agent) = patch.task_agent {
        settings.agents.task_agent = convert::agent_from_proto(agent);
    }
    if let Some(mode) = patch.task_mode {
        settings.agents.task_mode = convert::agent_mode_from_proto(mode);
    }
    if let Some(draft) = patch.task_draft {
        settings.agents.task_delivery = PromptDelivery::drafting(draft);
    }
    if let Some(custom) = patch.task_message_custom {
        settings.agents.task_message.custom = custom;
    }
    if let Some(text) = &patch.task_message {
        if text.chars().count() > MAX_TASK_MESSAGE {
            return Err(ServiceError::Invalid(format!(
                "the task message is longer than {MAX_TASK_MESSAGE} characters"
            )));
        }
        text.trim_end()
            .clone_into(&mut settings.agents.task_message.text);
    }
    if let Some(appearance) = &patch.appearance {
        settings.appearance = convert::appearance_from_proto(Some(appearance));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_agent_is_chosen_and_cleared() {
        let mut s = Settings::default();
        let choose = v1::SettingsPatch {
            task_agent: Some(v1::Agent::Claude.into()),
            task_mode: Some(v1::AgentMode::Plan.into()),
            ..Default::default()
        };
        apply_patch(&mut s, &choose).unwrap();
        assert_eq!(s.agents.task_agent, Some(Agent::Claude));
        assert_eq!(s.agents.task_mode, urushi_core::AgentMode::Plan);
        apply_patch(&mut s, &v1::SettingsPatch::default()).unwrap();
        assert_eq!(
            s.agents.task_agent,
            Some(Agent::Claude),
            "unset fields stay"
        );
        let clear = v1::SettingsPatch {
            task_agent: Some(v1::Agent::Unspecified.into()),
            ..Default::default()
        };
        apply_patch(&mut s, &clear).unwrap();
        assert_eq!(s.agents.task_agent, None);
        assert_eq!(s.agents.task_mode, urushi_core::AgentMode::Plan);
    }

    #[test]
    fn the_task_message_is_drafted_or_custom_by_choice() {
        let mut s = Settings::default();
        assert_eq!(s.agents.task_delivery, PromptDelivery::Send);
        assert_eq!(s.agents.task_message.template(), None);
        let patch = v1::SettingsPatch {
            task_draft: Some(true),
            task_message_custom: Some(true),
            task_message: Some("Pick up {key}\n\n".into()),
            ..Default::default()
        };
        apply_patch(&mut s, &patch).unwrap();
        assert_eq!(s.agents.task_delivery, PromptDelivery::Draft);
        assert_eq!(s.agents.task_message.template(), Some("Pick up {key}"));
        let off = v1::SettingsPatch {
            task_draft: Some(false),
            task_message_custom: Some(false),
            ..Default::default()
        };
        apply_patch(&mut s, &off).unwrap();
        assert_eq!(s.agents.task_delivery, PromptDelivery::Send);
        assert_eq!(s.agents.task_message.template(), None);
        assert_eq!(
            s.agents.task_message.text, "Pick up {key}",
            "kept while off"
        );
        let long = v1::SettingsPatch {
            task_message: Some("x".repeat(MAX_TASK_MESSAGE + 1)),
            ..Default::default()
        };
        assert!(matches!(
            apply_patch(&mut s, &long),
            Err(ServiceError::Invalid(_))
        ));
        assert_eq!(s.agents.task_message.text, "Pick up {key}");
    }

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
        assert!(s.update_base_from_remote, "untouched");
        let off = v1::SettingsPatch {
            update_base_from_remote: Some(false),
            ..Default::default()
        };
        apply_patch(&mut s, &off).unwrap();
        assert!(!s.update_base_from_remote);
    }

    #[test]
    fn the_appearance_is_replaced_when_set() {
        let mut s = Settings::default();
        let look = urushi_core::Appearance {
            scheme: urushi_core::Scheme::Ocean,
            logo: urushi_core::LogoStyle::Hidden,
        };
        let patch = v1::SettingsPatch {
            appearance: Some(convert::appearance_to_proto(look)),
            ..Default::default()
        };
        apply_patch(&mut s, &patch).unwrap();
        assert_eq!(s.appearance, look);
        apply_patch(&mut s, &v1::SettingsPatch::default()).unwrap();
        assert_eq!(s.appearance, look, "unset: kept");
    }

    #[test]
    fn skip_permissions_is_patched_per_agent() {
        let mut s = Settings::default();
        let on = v1::SettingsPatch {
            claude_skip_permissions: Some(true),
            cursor_skip_permissions: Some(true),
            ..Default::default()
        };
        apply_patch(&mut s, &on).unwrap();
        assert!(s.agents.skips_permissions(Agent::Claude));
        assert!(!s.agents.skips_permissions(Agent::Codex));
        let off = v1::SettingsPatch {
            claude_skip_permissions: Some(false),
            ..Default::default()
        };
        apply_patch(&mut s, &off).unwrap();
        assert!(!s.agents.skips_permissions(Agent::Claude));
        assert!(s.agents.skips_permissions(Agent::Cursor), "untouched");
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
