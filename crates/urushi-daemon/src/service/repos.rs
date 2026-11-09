//! Adding and removing repos (git repositories).

use std::path::PathBuf;
use std::sync::Arc;

use urushi_core::{Repo, RepoId};

use super::{Service, ServiceError, ServiceResult, blocking, new_uuid, now};

impl Service {
    /// Adds the repository containing `path`. If it's already added, the existing
    /// repo is returned.
    pub async fn add_repo(
        self: &Arc<Self>,
        path: PathBuf,
        name: Option<String>,
    ) -> ServiceResult<Repo> {
        if path.as_os_str().is_empty() {
            return Err(ServiceError::Invalid(
                "a repository path is required".into(),
            ));
        }
        let git = self.git.clone();
        let root = blocking(move || Ok(git.repo_root(&path)?)).await?;
        let canonical = std::fs::canonicalize(&root).unwrap_or(root);

        self.mutate(|state| {
            if let Some(existing) = state.repos.iter().find(|p| p.repo_root == canonical) {
                return Ok(existing.clone());
            }
            let default_name = canonical
                .file_name()
                .map_or_else(|| "repository".into(), |n| n.to_string_lossy().into_owned());
            let repo = Repo {
                id: RepoId::from_uuid(new_uuid()),
                name: name
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or(default_name),
                repo_root: canonical.clone(),
                default_base: None,
                settings: urushi_core::RepoSettingsOverrides::default(),
                added_at: now(),
            };
            state.repos.push(repo.clone());
            Ok(repo)
        })
    }

    /// Removes a repo. Refused while it still has threads, since removing it would
    /// orphan their worktrees and branches. Delete the threads first.
    pub fn remove_repo(&self, id: RepoId) -> ServiceResult<()> {
        self.mutate(|state| {
            state.repo(id)?;
            let threads = state.threads.iter().filter(|r| r.thread.repo == id).count();
            if threads > 0 {
                return Err(ServiceError::Precondition(format!(
                    "repo still has {threads} thread(s); delete them first"
                )));
            }
            state.repos.retain(|p| p.id != id);
            Ok(())
        })
    }

    /// Local branches of a repo, plus the one checked out in its main worktree.
    pub async fn list_branches(&self, id: RepoId) -> ServiceResult<(Vec<String>, Option<String>)> {
        let repo = self.read(|s| Ok(s.repo(id)?.repo_root.clone()))?;
        let git = self.git.clone();
        blocking(move || Ok((git.local_branches(&repo)?, git.current_branch(&repo)?))).await
    }
}
