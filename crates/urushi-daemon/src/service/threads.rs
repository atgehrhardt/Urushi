//! Creating and renaming threads. Every thread always gets its own worktree (ADR 0004).

use std::collections::BTreeSet;
use std::sync::Arc;

use urushi_core::thread::NewThread;
use urushi_core::{
    BranchName, PaneId, ProjectId, RepoId, Slug, SubtaskId, Tab, TabId, TaskId, Thread, ThreadId,
};
use urushi_mux::TermSize;

use super::{Service, ServiceError, ServiceResult, blocking, new_uuid, now};
use crate::store::ThreadRecord;

/// Title used when the user doesn't give one.
const DEFAULT_THREAD_TITLE: &str = "New thread";
/// Title of a thread's first tab.
pub(crate) const DEFAULT_TAB_TITLE: &str = "Terminal";

/// What the user asked for when creating a thread.
#[derive(Clone, Debug, Default)]
pub struct ThreadRequest {
    /// Title; empty uses "New thread".
    pub title: String,
    /// Ref to branch from; `None` uses the project's default base, or its current branch.
    pub base_ref: Option<String>,
    /// Branch to create; `None` generates `<prefix><slug>-<short id>`.
    pub branch: Option<String>,
    /// Open a first tab with a shell. Clients that let the user pick what to run first
    /// turn this off.
    pub initial_tab: bool,
    /// Project to file the thread under; `None` leaves it ungrouped.
    pub project: Option<ProjectId>,
    /// The task (and maybe subtask) on a board this thread is for; it gets linked to
    /// the thread when the thread is recorded.
    pub task: Option<(TaskId, Option<SubtaskId>)>,
}

impl Service {
    /// Creates a thread: a new branch and worktree from the requested base (or the
    /// repo's default base, or its current branch), plus, if asked, a first tab with a
    /// shell in the worktree.
    ///
    /// All-or-nothing: if anything fails after the worktree was created, the worktree
    /// and branch are removed again and no thread is recorded.
    pub async fn create_thread(
        self: &Arc<Self>,
        repo_id: RepoId,
        request: ThreadRequest,
    ) -> ServiceResult<Thread> {
        let ThreadRequest {
            title,
            base_ref,
            branch,
            initial_tab,
            project: project_id,
            task,
        } = request;
        let branch = parse_branch(branch)?;
        let title = match title.trim() {
            "" => DEFAULT_THREAD_TITLE.to_owned(),
            t => t.to_owned(),
        };
        let (project, settings, worktree_root) = self.read(|s| {
            if let Some(id) = project_id {
                s.project(id)?;
            }
            // Refuse up front, before making a worktree, if the task isn't there.
            if let Some(link) = task {
                s.task_link_target(link)?;
            }
            let project = s.repo(repo_id)?.clone();
            let settings = s.effective_settings(repo_id);
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
            let (base, base_commit) = resolve_base(&*git, &repo, base_ref)?;
            let preexisting: BTreeSet<BranchName> = git
                .local_branches(&repo)?
                .into_iter()
                .filter_map(|b| BranchName::new(b).ok())
                .collect();
            let thread = Thread::new(
                NewThread {
                    id,
                    repo: repo_id,
                    project: project_id,
                    title,
                    worktree_path,
                    base_ref: base,
                    base_commit,
                    created_at,
                    branch,
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

        let first_tab = FirstTab {
            open: initial_tab,
            shell: settings.default_shell,
            task,
        };
        match self.finish_thread_creation(&mut thread, preexisting, first_tab) {
            Ok(()) => Ok(thread),
            Err(e) => {
                self.rollback_worktree(&project.repo_root, &thread).await;
                Err(e)
            }
        }
    }

    /// Records the thread, and opens its first tab if asked.
    fn finish_thread_creation(
        &self,
        thread: &mut Thread,
        preexisting: BTreeSet<BranchName>,
        first: FirstTab,
    ) -> ServiceResult<()> {
        thread
            .mark_ready()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let mut first_tab = None;
        if first.open {
            let shell = first.shell;
            let tab_id = TabId::from_uuid(new_uuid());
            let pane = PaneId::from_uuid(new_uuid());
            thread.tabs.push(tab_id);
            self.spawn_session(pane, thread, shell, TermSize::DEFAULT)?;
            first_tab = Some(Tab::new(tab_id, thread.id, DEFAULT_TAB_TITLE, pane));
        }
        let record = ThreadRecord {
            thread: thread.clone(),
            preexisting_branches: preexisting,
        };
        let pane = first_tab.as_ref().map(Tab::focused);
        let result = self.mutate(|state| {
            state.repo(thread.repo)?;
            if let Some(project) = thread.project {
                state.project(project)?;
            }
            state.threads.push(record);
            state.tabs.extend(first_tab);
            // The task may have been deleted while the worktree was made; the thread
            // stands on its own, so that just leaves it unlinked.
            if let Some((task, subtask)) = first.task
                && let Some(board) = state.boards.iter_mut().find(|b| b.task(task).is_some())
                && let Err(e) = board.link_thread(task, subtask, thread.id)
            {
                tracing::warn!(thread = %thread.id, error = %e, "thread not linked to its task");
            }
            Ok(())
        });
        if result.is_err() {
            self.kill_sessions(pane);
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

/// Whether a new thread opens a first tab, and the shell it runs (`None` = login shell).
struct FirstTab {
    open: bool,
    shell: Option<String>,
    /// The board task to link the thread to once it's recorded.
    task: Option<(TaskId, Option<SubtaskId>)>,
}

/// Validates a branch name the user typed. Blank means "generate one".
fn parse_branch(branch: Option<String>) -> ServiceResult<Option<BranchName>> {
    branch
        .map(|b| b.trim().to_owned())
        .filter(|b| !b.is_empty())
        .map(BranchName::new)
        .transpose()
        .map_err(|e| ServiceError::Invalid(e.to_string()))
}

/// The ref to branch from (`base_ref`, else the repo's current branch, else `HEAD`) and
/// the commit it points to. A ref that doesn't exist is the user's mistake, so it is
/// reported as such rather than as a git failure.
fn resolve_base(
    git: &dyn crate::git::Git,
    repo: &std::path::Path,
    base_ref: Option<String>,
) -> ServiceResult<(String, String)> {
    let base = match base_ref {
        Some(base) => base,
        None => git.current_branch(repo)?.unwrap_or_else(|| "HEAD".into()),
    };
    match git.resolve_commit(repo, &base) {
        Ok(commit) => Ok((base, commit)),
        Err(crate::git::GitError::Failed { .. }) => Err(ServiceError::Invalid(format!(
            "cannot start from {base:?}: no branch, tag or commit has that name"
        ))),
        Err(e) => Err(e.into()),
    }
}
