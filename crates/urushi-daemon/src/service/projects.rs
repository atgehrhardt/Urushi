//! Adding and removing projects (git repositories).

use std::path::PathBuf;
use std::sync::Arc;

use urushi_core::{Project, ProjectId};

use super::{Service, ServiceError, ServiceResult, blocking, new_uuid, now};

impl Service {
    /// Adds the repository containing `path`. If it's already added, the existing
    /// project is returned.
    pub async fn add_project(
        self: &Arc<Self>,
        path: PathBuf,
        name: Option<String>,
    ) -> ServiceResult<Project> {
        if path.as_os_str().is_empty() {
            return Err(ServiceError::Invalid(
                "a repository path is required".into(),
            ));
        }
        let git = self.git.clone();
        let root = blocking(move || Ok(git.repo_root(&path)?)).await?;
        let canonical = std::fs::canonicalize(&root).unwrap_or(root);

        self.mutate(|state| {
            if let Some(existing) = state.projects.iter().find(|p| p.repo_root == canonical) {
                return Ok(existing.clone());
            }
            let default_name = canonical
                .file_name()
                .map_or_else(|| "repository".into(), |n| n.to_string_lossy().into_owned());
            let project = Project {
                id: ProjectId::from_uuid(new_uuid()),
                name: name
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or(default_name),
                repo_root: canonical.clone(),
                default_base: None,
                settings: urushi_core::ProjectSettingsOverrides::default(),
                added_at: now(),
            };
            state.projects.push(project.clone());
            Ok(project)
        })
    }

    /// Removes a project. Refused while it still has threads, since removing it would
    /// orphan their worktrees and branches. Delete the threads first.
    pub fn remove_project(&self, id: ProjectId) -> ServiceResult<()> {
        self.mutate(|state| {
            state.project(id)?;
            let threads = state
                .threads
                .iter()
                .filter(|r| r.thread.project == id)
                .count();
            if threads > 0 {
                return Err(ServiceError::Precondition(format!(
                    "project still has {threads} thread(s); delete them first"
                )));
            }
            state.projects.retain(|p| p.id != id);
            Ok(())
        })
    }

    /// Local branches of a project, plus the one checked out in its main worktree.
    pub async fn list_branches(
        &self,
        id: ProjectId,
    ) -> ServiceResult<(Vec<String>, Option<String>)> {
        let repo = self.read(|s| Ok(s.project(id)?.repo_root.clone()))?;
        let git = self.git.clone();
        blocking(move || Ok((git.local_branches(&repo)?, git.current_branch(&repo)?))).await
    }
}
