//! A project's parent branch and its parent-branch thread (ADR 0029).
//!
//! A project can name a branch in one repo as its parent branch. New threads of that
//! repo filed under the project start from it, and, unless the project's settings
//! switch it off, the project has a *parent-branch thread*: a thread whose terminals
//! work on the parent branch itself. Git won't check a branch out twice, so that
//! thread runs in the checkout that already has the branch (usually the repo's own
//! folder) when there is one, and in a new worktree of its own otherwise.
//!
//! A parent-branch thread never owns a branch. Closing it is deleting it through the
//! normal plan → confirm → `DeleteThread` flow, which removes only a worktree Urushi
//! made for it and switches the thread off in the project's settings.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use urushi_core::thread::NewThread;
use urushi_core::{
    BaseCheckout, BranchName, PaneId, Project, ProjectBase, ProjectId, ProjectSettings, RepoId,
    Tab, TabId, Thread, ThreadId, base_checkout,
};
use urushi_mux::TermSize;

use super::threads::{DEFAULT_TAB_TITLE, worktree_dir};
use super::{Service, ServiceError, ServiceResult, blocking, new_uuid, now};
use crate::git::Git;
use crate::store::ThreadRecord;

/// Project settings as a client asked for them. The branch may still name a
/// remote-tracking branch (`origin/x`), which is turned into the local branch `x`.
#[derive(Clone, Debug)]
pub struct ProjectSettingsRequest {
    /// The parent branch's repo and the branch the user picked. `None`: no parent branch.
    pub base: Option<(RepoId, String)>,
    /// Keep a parent-branch thread pinned at the top of the project.
    pub base_thread: bool,
}

impl Service {
    /// Replaces a project's settings, all or nothing.
    ///
    /// Turning the parent-branch thread on (with a parent branch) creates it. A change
    /// that would need an existing one closed (switching it off, or another parent
    /// branch) is refused: closing it removes things, so it goes through the deletion
    /// warning, which also switches it off.
    pub async fn update_project_settings(
        self: &Arc<Self>,
        id: ProjectId,
        request: ProjectSettingsRequest,
    ) -> ServiceResult<Project> {
        let repo_root = self.read(|s| {
            s.project(id)?;
            request
                .base
                .as_ref()
                .map(|(repo, _)| s.repo(*repo).map(|r| r.repo_root.clone()))
                .transpose()
        })?;
        let base = match (request.base, repo_root) {
            (Some((repo, branch)), Some(root)) => {
                let git = self.git.clone();
                let branch = blocking(move || local_parent_branch(&*git, &root, &branch)).await?;
                Some(ProjectBase { repo, branch })
            }
            _ => None,
        };
        let settings = ProjectSettings {
            base,
            base_thread: request.base_thread,
        };
        let existing = self.read(|s| {
            Ok(s.project_base_thread(id)
                .map(|r| (r.thread.repo, r.thread.primary_branch.clone())))
        })?;
        let wanted = settings
            .wanted_base_thread()
            .map(|b| (b.repo, b.branch.clone()));
        match (existing, wanted) {
            (Some(have), Some(want)) if have == want => {}
            (Some(_), _) => {
                return Err(ServiceError::Precondition(
                    "close the project's parent-branch thread first".into(),
                ));
            }
            (None, Some(_)) => {
                self.create_project_base_thread(id, settings.clone())
                    .await?;
                return self.read(|s| s.project(id).cloned());
            }
            (None, None) => {}
        }
        self.mutate(|state| {
            let project = state.project_mut(id)?;
            project.settings = settings;
            Ok(project.clone())
        })
    }

    /// After the first thread of a project without a parent branch was created from a
    /// local branch, makes that branch the project's parent branch (and so, unless it is
    /// switched off, opens the parent-branch thread). This is what makes the
    /// parent-branch thread appear "on by default". Failures are only logged: the
    /// thread that triggered it already exists.
    pub(crate) async fn adopt_project_base(self: &Arc<Self>, thread: &Thread) {
        let Some(project) = thread.project else {
            return;
        };
        let adopt = self.read(|s| {
            let p = s.project(project)?;
            let first = s
                .threads
                .iter()
                .filter(|r| r.thread.project == Some(project))
                .count()
                == 1;
            Ok((first && p.settings.base.is_none()).then_some(p.settings.base_thread))
        });
        let Ok(Some(base_thread)) = adopt else {
            return;
        };
        let Ok(branch) = BranchName::new(thread.base_ref.clone()) else {
            return; // Started from a commit, tag or remote branch: nothing to adopt.
        };
        let request = ProjectSettingsRequest {
            base: Some((thread.repo, branch.to_string())),
            base_thread,
        };
        if let Err(e) = self.update_project_settings(project, request).await {
            tracing::warn!(%project, error = %e, "could not adopt the project's parent branch");
        }
    }

    /// Creates the project's parent-branch thread and saves `settings` with it, all or
    /// nothing. `settings` must want the thread.
    async fn create_project_base_thread(
        self: &Arc<Self>,
        project: ProjectId,
        settings: ProjectSettings,
    ) -> ServiceResult<()> {
        let base = settings
            .wanted_base_thread()
            .cloned()
            .ok_or_else(|| ServiceError::Internal("no parent-branch thread wanted".into()))?;
        let (repo, shell, worktree_root) = self.read(|s| {
            let root = s
                .settings
                .worktree_root
                .clone()
                .unwrap_or_else(|| self.home().join("worktrees"));
            Ok((
                s.repo(base.repo)?.clone(),
                s.effective_settings(base.repo).default_shell,
                root,
            ))
        })?;
        let id = ThreadId::from_uuid(new_uuid());
        let title = base.branch.to_string();
        let new_worktree = worktree_dir(&worktree_root, &repo, &title, id);
        let git = self.git.clone();
        let (root, branch) = (repo.repo_root.clone(), base.branch.clone());
        let created_at = now();
        let (mut thread, preexisting) = blocking(move || {
            let commit = git.resolve_commit(&root, branch.as_str())?;
            let trees = git.worktrees(&root)?;
            let place = base_checkout(
                &branch,
                trees
                    .iter()
                    .map(|w| (w.path.as_path(), w.branch.as_deref())),
            );
            let (path, attached) = match place {
                BaseCheckout::Attach(path) => (path, true),
                BaseCheckout::NewWorktree => {
                    if let Some(parent) = new_worktree.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| ServiceError::Internal(e.to_string()))?;
                    }
                    git.worktree_add_existing(&root, &new_worktree, &branch)?;
                    (new_worktree, false)
                }
            };
            let spec = NewThread {
                id,
                repo: base.repo,
                project: Some(project),
                title,
                worktree_path: path,
                base_ref: branch.to_string(),
                base_commit: commit,
                created_at,
                branch: None,
            };
            // Every branch there is, so the thread can never own (and delete) one.
            let preexisting: BTreeSet<BranchName> = git
                .local_branches(&root)?
                .into_iter()
                .filter_map(|b| BranchName::new(b).ok())
                .collect();
            Ok((Thread::project_base(spec, branch, attached), preexisting))
        })
        .await?;

        if let Err(e) = self.record_project_base_thread(&mut thread, preexisting, shell, settings) {
            if !thread.is_attached() {
                self.remove_own_worktree(repo.repo_root, thread.worktree_path)
                    .await;
            }
            return Err(e);
        }
        Ok(())
    }

    /// Opens the thread's first tab and records it together with the project settings
    /// that asked for it. Refused if the project got a parent-branch thread meanwhile.
    fn record_project_base_thread(
        &self,
        thread: &mut Thread,
        preexisting: BTreeSet<BranchName>,
        shell: Option<String>,
        settings: ProjectSettings,
    ) -> ServiceResult<()> {
        thread
            .mark_ready()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let tab_id = TabId::from_uuid(new_uuid());
        let pane = PaneId::from_uuid(new_uuid());
        thread.tabs.push(tab_id);
        self.spawn_session_with_env(pane, &*thread, shell, TermSize::DEFAULT, Vec::new())?;
        let tab = Tab::new(tab_id, thread.id, DEFAULT_TAB_TITLE, pane);
        let project = thread.project;
        let result = self.mutate(|state| {
            let project = project.ok_or_else(|| ServiceError::Internal("no project".into()))?;
            state.repo(thread.repo)?;
            state.project(project)?;
            if state.project_base_thread(project).is_some() {
                return Err(ServiceError::Conflict(
                    "the project already has a parent-branch thread".into(),
                ));
            }
            state.project_mut(project)?.settings = settings;
            state.threads.push(ThreadRecord {
                thread: thread.clone(),
                preexisting_branches: preexisting,
            });
            state.tabs.push(tab);
            Ok(())
        });
        if result.is_err() {
            self.kill_sessions([pane]);
        }
        result
    }

    /// Takes back a worktree Urushi made for a parent-branch thread that couldn't be
    /// recorded. Unlike a failed ordinary thread, the branch stays: it isn't the
    /// thread's.
    async fn remove_own_worktree(&self, repo: PathBuf, path: PathBuf) {
        let git = self.git.clone();
        let result = blocking(move || {
            git.worktree_remove(&repo, &path)?;
            git.worktree_prune(&repo)?;
            Ok(())
        })
        .await;
        if let Err(e) = result {
            tracing::error!(error = %e, "failed to remove a parent-branch thread's worktree");
        }
    }
}

/// The local branch a picked parent branch stands for: a local branch as is, or for a
/// remote-tracking branch (`origin/x`) the local `x`, which is created, tracking it, if
/// it doesn't exist yet. Anything else (a tag, a commit, an unknown name) is refused.
fn local_parent_branch(
    git: &dyn Git,
    repo: &std::path::Path,
    picked: &str,
) -> ServiceResult<BranchName> {
    let picked = picked.trim();
    let invalid = || {
        ServiceError::Invalid(format!(
            "{picked:?} is not a branch of this repo; pick a local or remote branch"
        ))
    };
    let locals = git.local_branches(repo)?;
    if locals.iter().any(|b| b == picked) {
        return BranchName::new(picked.to_owned()).map_err(|_| invalid());
    }
    let remote = git
        .remote_branches(repo)?
        .into_iter()
        .find(|(remote, branch)| format!("{remote}/{branch}") == picked)
        .ok_or_else(invalid)?;
    let local = BranchName::new(remote.1).map_err(|_| invalid())?;
    if !locals.iter().any(|b| b == local.as_str()) {
        git.create_tracking_branch(repo, &local, picked)?;
    }
    Ok(local)
}
