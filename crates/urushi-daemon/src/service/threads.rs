//! Creating and renaming threads. Every thread always gets its own worktree (ADR 0004).

use std::collections::BTreeSet;
use std::sync::Arc;

use urushi_core::thread::NewThread;
use urushi_core::{BranchName, PaneId, ProjectId, Slug, Tab, TabId, Thread, ThreadId};
use urushi_mux::TermSize;

use super::{Service, ServiceError, ServiceResult, blocking, new_uuid, now};
use crate::store::ThreadRecord;

/// Title used when the user doesn't give one.
const DEFAULT_THREAD_TITLE: &str = "New thread";
/// Title of a thread's first tab.
pub(crate) const DEFAULT_TAB_TITLE: &str = "Terminal";

impl Service {
    /// Creates a thread: a new branch and worktree from `base_ref` (or the project's
    /// default base, or its current branch), plus a first tab with a shell in the worktree.
    ///
    /// All-or-nothing: if anything fails after the worktree was created, the worktree
    /// and branch are removed again and no thread is recorded.
    pub async fn create_thread(
        self: &Arc<Self>,
        project_id: ProjectId,
        title: &str,
        base_ref: Option<String>,
    ) -> ServiceResult<Thread> {
        let title = match title.trim() {
            "" => DEFAULT_THREAD_TITLE.to_owned(),
            t => t.to_owned(),
        };
        let (project, settings, worktree_root) = self.read(|s| {
            let project = s.project(project_id)?.clone();
            let settings = s.effective_settings(project_id);
            let root = s
                .settings
                .worktree_root
                .clone()
                .unwrap_or_else(|| self.home().join("worktrees"));
            Ok((project, settings, root))
        })?;

        let id = ThreadId::from_uuid(new_uuid());
        let slug = Slug::from_title(&title);
        let worktree_path = worktree_root
            .join(format!(
                "{}-{}",
                Slug::from_title(&project.name),
                project.id.short()
            ))
            .join(format!("{slug}-{}", id.short()));

        let git = self.git.clone();
        let repo = project.repo_root.clone();
        let base_ref = base_ref
            .filter(|b| !b.trim().is_empty())
            .or(project.default_base.clone());
        let prefix = settings.branch_prefix.clone();
        let created_at = now();
        let (mut thread, preexisting) = blocking(move || {
            let base = match base_ref {
                Some(base) => base,
                None => git.current_branch(&repo)?.unwrap_or_else(|| "HEAD".into()),
            };
            let base_commit = git.resolve_commit(&repo, &base)?;
            let preexisting: BTreeSet<BranchName> = git
                .local_branches(&repo)?
                .into_iter()
                .filter_map(|b| BranchName::new(b).ok())
                .collect();
            let thread = Thread::new(
                NewThread {
                    id,
                    project: project_id,
                    title,
                    worktree_path,
                    base_ref: base,
                    base_commit,
                    created_at,
                },
                &prefix,
            )
            .map_err(|e| ServiceError::Invalid(e.to_string()))?;
            if preexisting.contains(&thread.primary_branch) {
                return Err(ServiceError::Conflict(format!(
                    "branch {} already exists",
                    thread.primary_branch
                )));
            }
            if let Some(parent) = thread.worktree_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| ServiceError::Internal(e.to_string()))?;
            }
            git.worktree_add(
                &repo,
                &thread.worktree_path,
                &thread.primary_branch,
                &thread.base_commit,
            )?;
            Ok((thread, preexisting))
        })
        .await?;

        match self.finish_thread_creation(&mut thread, preexisting, settings.default_shell) {
            Ok(()) => Ok(thread),
            Err(e) => {
                self.rollback_worktree(&project.repo_root, &thread).await;
                Err(e)
            }
        }
    }

    fn finish_thread_creation(
        &self,
        thread: &mut Thread,
        preexisting: BTreeSet<BranchName>,
        shell: Option<String>,
    ) -> ServiceResult<()> {
        thread
            .mark_ready()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let tab_id = TabId::from_uuid(new_uuid());
        let pane = PaneId::from_uuid(new_uuid());
        thread.tabs.push(tab_id);
        self.spawn_session(pane, thread, shell, TermSize::DEFAULT)?;
        let record = ThreadRecord {
            thread: thread.clone(),
            preexisting_branches: preexisting,
        };
        let tab = Tab::new(tab_id, thread.id, DEFAULT_TAB_TITLE, pane);
        let result = self.mutate(|state| {
            state.project(thread.project)?;
            state.threads.push(record);
            state.tabs.push(tab);
            Ok(())
        });
        if result.is_err() {
            self.kill_sessions([pane]);
        }
        result
    }

    async fn rollback_worktree(&self, repo: &std::path::Path, thread: &Thread) {
        let git = self.git.clone();
        let repo = repo.to_path_buf();
        let (path, branch) = (thread.worktree_path.clone(), thread.primary_branch.clone());
        let result = blocking(move || {
            let _ = git.worktree_remove(&repo, &path);
            let _ = git.worktree_prune(&repo);
            git.delete_branch(&repo, &branch)?;
            Ok(())
        })
        .await;
        if let Err(e) = result {
            tracing::error!(thread = %thread.id, error = %e, "failed to roll back thread creation");
        }
    }

    /// Renames a thread. The branch and worktree keep their original names.
    pub fn rename_thread(&self, id: ThreadId, title: &str) -> ServiceResult<()> {
        let title = title.trim();
        if title.is_empty() {
            return Err(ServiceError::Invalid("title must not be empty".into()));
        }
        self.mutate(|state| {
            title.clone_into(&mut state.thread_mut(id)?.thread.title);
            Ok(())
        })
    }
}
