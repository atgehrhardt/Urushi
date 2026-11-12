//! A project: a named group of threads, made by the user to organize work.
//!
//! Projects are only organization. A project can hold threads from any repo, and a
//! thread is in at most one project (`Thread::project`). Deleting a project ungroups its
//! threads; it never touches a worktree or branch.
//!
//! A project can have a **parent branch** (ADR 0029): a branch in one repo that the
//! project's new threads start from. While its settings ask for it, the project also has
//! a *parent-branch thread*, listed first, whose terminals run on that branch itself.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{BranchName, LabelColor, ProjectId, RepoId, Timestamp};

/// A user-made group of threads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Identity.
    pub id: ProjectId,
    /// Display name.
    pub name: String,
    /// When it was created.
    pub created_at: Timestamp,
    /// Project-specific settings. Projects saved before settings existed get the
    /// defaults.
    #[serde(default)]
    pub settings: ProjectSettings,
    /// The color its sidebar section is shown in, from the board palette (ADR 0048).
    /// `None`: no color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<LabelColor>,
}

/// The branch a project's work starts from, and the repo it lives in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBase {
    /// The repo the branch is in.
    pub repo: RepoId,
    /// A local branch of that repo.
    pub branch: BranchName,
}

/// What a project can be configured with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSettings {
    /// The parent branch. `None`: new threads start from the repo's default, and there
    /// is no parent-branch thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<ProjectBase>,
    /// Whether the project keeps a parent-branch thread pinned at its top. On unless
    /// the user switches it off; it has no effect while `base` is unset.
    #[serde(default = "on")]
    pub base_thread: bool,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            base: None,
            base_thread: true,
        }
    }
}

impl ProjectSettings {
    /// The parent branch to start a new thread in `repo` from, if the project has one
    /// in that repo.
    pub fn base_in(&self, repo: RepoId) -> Option<&BranchName> {
        self.base
            .as_ref()
            .filter(|b| b.repo == repo)
            .map(|b| &b.branch)
    }

    /// The parent-branch thread the project should have: its base, if it has one and
    /// the thread is switched on.
    pub fn wanted_base_thread(&self) -> Option<&ProjectBase> {
        self.base.as_ref().filter(|_| self.base_thread)
    }
}

fn on() -> bool {
    true
}

/// Where a project's parent-branch thread runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BaseCheckout {
    /// In a checkout that already has the branch (usually the repo's own folder). It
    /// belongs to the user, so Urushi never removes it.
    Attach(PathBuf),
    /// In a new worktree that checks the branch out. Urushi made it, so closing the
    /// thread removes it (but never the branch).
    NewWorktree,
}

/// Picks where a parent-branch thread for `branch` runs. Git won't check a branch out
/// twice, so if any worktree (`(path, checked-out branch)`) already has it, the thread
/// runs there; otherwise it gets a worktree of its own.
pub fn base_checkout<'a>(
    branch: &BranchName,
    worktrees: impl IntoIterator<Item = (&'a Path, Option<&'a str>)>,
) -> BaseCheckout {
    worktrees
        .into_iter()
        .find(|(_, b)| *b == Some(branch.as_str()))
        .map_or(BaseCheckout::NewWorktree, |(path, _)| {
            BaseCheckout::Attach(path.to_path_buf())
        })
}

/// Moves project `id` to `index` in the list (counted without it; clamped to the end).
/// The list's order is the order projects are shown in. Returns `false`, changing
/// nothing, if there is no such project.
pub fn move_project(projects: &mut Vec<Project>, id: ProjectId, index: usize) -> bool {
    crate::order::move_where(projects, |p| p.id == id, index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects(n: u128) -> Vec<Project> {
        (0..n)
            .map(|i| Project {
                id: ProjectId::from_u128(i),
                name: format!("p{i}"),
                created_at: Timestamp::unknown(),
                settings: ProjectSettings::default(),
                color: None,
            })
            .collect()
    }

    #[test]
    fn a_color_is_stored_only_when_set() {
        let mut p = projects(1).remove(0);
        let plain = serde_json::to_string(&p).unwrap();
        assert!(!plain.contains("color"), "{plain}");
        assert_eq!(serde_json::from_str::<Project>(&plain).unwrap(), p);
        p.color = Some(LabelColor::Teal);
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"color\":\"teal\""), "{json}");
        assert_eq!(serde_json::from_str::<Project>(&json).unwrap(), p);
    }

    fn order(projects: &[Project]) -> Vec<&str> {
        projects.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn projects_move_up_and_down() {
        let mut list = projects(3);
        assert!(move_project(&mut list, ProjectId::from_u128(2), 0));
        assert_eq!(order(&list), ["p2", "p0", "p1"]);
        assert!(move_project(&mut list, ProjectId::from_u128(2), 2));
        assert_eq!(order(&list), ["p0", "p1", "p2"]);
        assert!(move_project(&mut list, ProjectId::from_u128(0), 1));
        assert_eq!(order(&list), ["p1", "p0", "p2"]);
    }

    #[test]
    fn moving_clamps_to_the_end_and_keeps_its_place() {
        let mut list = projects(3);
        assert!(move_project(&mut list, ProjectId::from_u128(0), 99));
        assert_eq!(order(&list), ["p1", "p2", "p0"]);
        assert!(move_project(&mut list, ProjectId::from_u128(2), 1));
        assert_eq!(order(&list), ["p1", "p2", "p0"]);
    }

    #[test]
    fn an_unknown_project_changes_nothing() {
        let mut list = projects(2);
        assert!(!move_project(&mut list, ProjectId::from_u128(7), 0));
        assert_eq!(order(&list), ["p0", "p1"]);
    }

    #[test]
    fn projects_saved_without_settings_get_the_defaults() {
        let json = r#"{"id":"00000000-0000-0000-0000-000000000001","name":"p","created_at":0}"#;
        let project: Project = serde_json::from_str(json).unwrap();
        assert_eq!(project.settings, ProjectSettings::default());
        assert!(project.settings.base_thread, "on by default");
        assert_eq!(
            project.settings.wanted_base_thread(),
            None,
            "but needs a base"
        );
        let mut project = project;
        project.settings.base = Some(ProjectBase {
            repo: RepoId::from_u128(3),
            branch: BranchName::new("main").unwrap(),
        });
        let back: Project =
            serde_json::from_str(&serde_json::to_string(&project).unwrap()).unwrap();
        assert_eq!(back, project);
    }

    #[test]
    fn the_base_applies_only_in_its_repo_and_the_thread_can_be_off() {
        let mut settings = ProjectSettings {
            base: Some(ProjectBase {
                repo: RepoId::from_u128(3),
                branch: BranchName::new("develop").unwrap(),
            }),
            base_thread: true,
        };
        assert_eq!(
            settings.base_in(RepoId::from_u128(3)).unwrap().as_str(),
            "develop"
        );
        assert_eq!(settings.base_in(RepoId::from_u128(4)), None);
        assert!(settings.wanted_base_thread().is_some());
        settings.base_thread = false;
        assert_eq!(settings.wanted_base_thread(), None);
        assert!(
            settings.base_in(RepoId::from_u128(3)).is_some(),
            "still the base"
        );
    }

    #[test]
    fn a_checked_out_branch_is_attached_to_and_any_other_gets_a_worktree() {
        let main = BranchName::new("main").unwrap();
        let feature = BranchName::new("feature").unwrap();
        let trees = [
            (Path::new("/repo"), Some("main")),
            (Path::new("/wt/a"), Some("urushi/a")),
            (Path::new("/wt/b"), None),
        ];
        assert_eq!(
            base_checkout(&main, trees),
            BaseCheckout::Attach(PathBuf::from("/repo"))
        );
        assert_eq!(base_checkout(&feature, trees), BaseCheckout::NewWorktree);
    }
}
