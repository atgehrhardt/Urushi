//! User settings, stored by the daemon so every client (desktop, mobile, remote) sees
//! the same values.
//!
//! Every field has a default and unknown fields are ignored, so settings saved by an older
//! or newer version still load.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{AgentSettings, BranchPrefix};

/// Global settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// When a thread is deleted, also delete its branches on remotes. Default: `true`.
    pub delete_remote_branches: bool,
    /// Prefix for branches Urushi creates. Default: `urushi/`.
    pub branch_prefix: BranchPrefix,
    /// Shell for new panes. `None` uses the user's login shell.
    pub default_shell: Option<String>,
    /// Where thread worktrees are created. `None` uses `<app data dir>/worktrees`.
    pub worktree_root: Option<PathBuf>,
    /// Remote access over Tailscale.
    pub tailnet: TailnetSettings,
    /// How agent CLIs (Claude, Codex, Cursor) are started.
    pub agents: AgentSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            delete_remote_branches: true,
            branch_prefix: BranchPrefix::default(),
            default_shell: None,
            worktree_root: None,
            tailnet: TailnetSettings::default(),
            agents: AgentSettings::default(),
        }
    }
}

/// Settings for serving the daemon on the user's tailnet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TailnetSettings {
    /// Whether to listen on this machine's tailnet addresses. Default: `false`.
    pub serve: bool,
    /// TCP port to listen on.
    pub port: u16,
    /// Tailnet logins allowed to connect. Empty means only the host's own login.
    pub allowed_logins: Vec<String>,
    /// Whether tagged (non-user) tailnet nodes may connect. Default: `false`.
    pub allow_tagged_nodes: bool,
}

impl TailnetSettings {
    /// Default TCP port.
    pub const DEFAULT_PORT: u16 = 7420;
}

impl Default for TailnetSettings {
    fn default() -> Self {
        Self {
            serve: false,
            port: Self::DEFAULT_PORT,
            allowed_logins: Vec::new(),
            allow_tagged_nodes: false,
        }
    }
}

/// Per-repo overrides. `None` means "use the global value".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoSettingsOverrides {
    /// Overrides [`Settings::delete_remote_branches`].
    pub delete_remote_branches: Option<bool>,
    /// Overrides [`Settings::branch_prefix`].
    pub branch_prefix: Option<BranchPrefix>,
    /// Overrides [`Settings::default_shell`].
    pub default_shell: Option<String>,
}

/// Settings that apply to one repo, after its overrides have been applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveSettings {
    /// See [`Settings::delete_remote_branches`].
    pub delete_remote_branches: bool,
    /// See [`Settings::branch_prefix`].
    pub branch_prefix: BranchPrefix,
    /// See [`Settings::default_shell`].
    pub default_shell: Option<String>,
}

impl Settings {
    /// Applies a repo's overrides on top of these settings.
    pub fn for_repo(&self, overrides: &RepoSettingsOverrides) -> EffectiveSettings {
        EffectiveSettings {
            delete_remote_branches: overrides
                .delete_remote_branches
                .unwrap_or(self.delete_remote_branches),
            branch_prefix: overrides
                .branch_prefix
                .clone()
                .unwrap_or_else(|| self.branch_prefix.clone()),
            default_shell: overrides
                .default_shell
                .clone()
                .or_else(|| self.default_shell.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_delete_remote_branches() {
        let s = Settings::default();
        assert!(s.delete_remote_branches);
        assert_eq!(s.branch_prefix.as_str(), "urushi/");
        assert!(!s.tailnet.serve);
    }

    #[test]
    fn overrides_win_over_globals() {
        let s = Settings {
            default_shell: Some("/bin/zsh".into()),
            ..Settings::default()
        };
        let o = RepoSettingsOverrides {
            delete_remote_branches: Some(false),
            branch_prefix: Some(BranchPrefix::new("agents/").unwrap()),
            default_shell: None,
        };
        let e = s.for_repo(&o);
        assert!(!e.delete_remote_branches);
        assert_eq!(e.branch_prefix.as_str(), "agents/");
        assert_eq!(e.default_shell.as_deref(), Some("/bin/zsh"));
        assert!(
            s.for_repo(&RepoSettingsOverrides::default())
                .delete_remote_branches
        );
    }

    #[test]
    fn tolerant_deserialization() {
        let s: Settings =
            serde_json::from_str(r#"{"future_field": 1, "tailnet": {"serve": true}}"#).unwrap();
        assert!(s.tailnet.serve);
        assert_eq!(s.tailnet.port, TailnetSettings::DEFAULT_PORT);
        assert!(s.delete_remote_branches);
    }
}
