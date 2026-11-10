//! Client-side types for the project-settings dialog (ADR 0029): a project's parent
//! branch, which its new threads start from, and whether it keeps a parent-branch thread
//! at its top. The settings live in the daemon; this is only the dialog's draft.
//!
//! Closing an open parent-branch thread removes things, so a Save that needs it closed
//! (switching it off, or another parent branch) first shows the usual deletion warning
//! for it; the settings are sent once it is closed ([`PendingSettings`]).

use urushi_client::v1;

use crate::organize::Select;

/// What the project-settings dialog holds until Save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSettingsDraft {
    /// The project (a `Folder` on the wire).
    pub project: String,
    /// The parent branch's repo; `None`: no parent branch.
    pub repo: Option<String>,
    /// The parent branch as picked: a local branch, or a remote one (`origin/x`).
    pub branch: Option<String>,
    /// Keep a parent-branch thread at the project's top.
    pub base_thread: bool,
    /// The selector that is open, if any.
    pub open: Option<Select>,
}

impl ProjectSettingsDraft {
    /// A draft of the project's settings as the daemon reported them.
    pub fn from_folder(folder: &v1::Folder) -> Self {
        let settings = folder.settings.clone().unwrap_or_default();
        let repo = Some(settings.base_repo_id).filter(|r| !r.is_empty());
        Self {
            project: folder.id.clone(),
            branch: repo
                .as_ref()
                .map(|_| settings.base_branch)
                .filter(|b| !b.is_empty()),
            repo,
            base_thread: settings.base_thread.unwrap_or(true),
            open: None,
        }
    }

    /// The settings to send. A repo without a branch is no parent branch.
    pub fn to_settings(&self) -> v1::FolderSettings {
        let (repo, branch) = match (&self.repo, &self.branch) {
            (Some(repo), Some(branch)) => (repo.clone(), branch.clone()),
            _ => (String::new(), String::new()),
        };
        v1::FolderSettings {
            base_repo_id: repo,
            base_branch: branch,
            base_thread: Some(self.base_thread),
        }
    }

    /// Whether saving needs `thread`, the project's open parent-branch thread, closed
    /// first: it is switched off, or the parent branch is another one. `remotes` are the
    /// repo's remote branches, since picking `origin/x` means the local branch `x`.
    pub fn needs_closing(&self, thread: &v1::Thread, remotes: &[String]) -> bool {
        let same_branch = self.branch.as_deref().is_some_and(|b| {
            b == thread.primary_branch
                || (remotes.iter().any(|r| r == b)
                    && b.split_once('/').map(|(_, rest)| rest) == Some(&thread.primary_branch))
        });
        let same = self.repo.as_deref() == Some(thread.project_id.as_str()) && same_branch;
        !(self.base_thread && same)
    }
}

/// A change in the project-settings dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectSettingsMsg {
    /// Open or close a selector.
    ToggleSelect(Select),
    /// Choose the parent branch's repo (`None`: no parent branch).
    PickRepo(Option<String>),
    /// Choose the parent branch.
    PickBranch(String),
    /// Switch the parent-branch thread on or off.
    ToggleBaseThread,
}

/// Project settings waiting for the project's parent-branch thread to be closed.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingSettings {
    /// The project.
    pub project: String,
    /// What to save once the thread is closed.
    pub settings: v1::FolderSettings,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(repo: &str, branch: &str, on: Option<bool>) -> v1::Folder {
        v1::Folder {
            id: "p".into(),
            settings: Some(v1::FolderSettings {
                base_repo_id: repo.into(),
                base_branch: branch.into(),
                base_thread: on,
            }),
            ..Default::default()
        }
    }

    fn base(repo: &str, branch: &str) -> v1::Thread {
        v1::Thread {
            project_id: repo.into(),
            primary_branch: branch.into(),
            project_base: true,
            ..Default::default()
        }
    }

    #[test]
    fn drafts_come_from_the_folder_and_go_back_unchanged() {
        let draft = ProjectSettingsDraft::from_folder(&folder("r", "main", None));
        assert_eq!(draft.repo.as_deref(), Some("r"));
        assert_eq!(draft.branch.as_deref(), Some("main"));
        assert!(draft.base_thread, "unset is on");
        assert_eq!(
            draft.to_settings(),
            folder("r", "main", Some(true)).settings.unwrap()
        );

        let none = ProjectSettingsDraft::from_folder(&v1::Folder::default());
        assert_eq!(
            (none.repo, none.branch, none.base_thread),
            (None, None, true)
        );
        let half = ProjectSettingsDraft {
            branch: None,
            ..ProjectSettingsDraft::from_folder(&folder("r", "main", Some(false)))
        };
        assert_eq!(
            half.to_settings().base_repo_id,
            "",
            "no branch, no parent branch"
        );
    }

    #[test]
    fn only_switching_off_or_another_branch_closes_the_thread() {
        let draft = ProjectSettingsDraft::from_folder(&folder("r", "main", None));
        let thread = base("r", "main");
        assert!(!draft.needs_closing(&thread, &[]));
        let off = ProjectSettingsDraft {
            base_thread: false,
            ..draft.clone()
        };
        assert!(off.needs_closing(&thread, &[]));
        let other = ProjectSettingsDraft {
            branch: Some("develop".into()),
            ..draft.clone()
        };
        assert!(other.needs_closing(&thread, &[]));
        let elsewhere = ProjectSettingsDraft {
            repo: Some("s".into()),
            ..draft.clone()
        };
        assert!(elsewhere.needs_closing(&thread, &[]));
        // `origin/main` is the branch the thread already has; a local `team/main` isn't.
        let remote = ProjectSettingsDraft {
            branch: Some("origin/main".into()),
            ..draft.clone()
        };
        assert!(!remote.needs_closing(&thread, &["origin/main".into()]));
        let local = ProjectSettingsDraft {
            branch: Some("team/main".into()),
            ..draft
        };
        assert!(local.needs_closing(&thread, &["origin/main".into()]));
    }
}
