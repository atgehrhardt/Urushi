//! Creating and renaming threads. Every thread always gets its own worktree (ADR 0004).

use std::collections::BTreeSet;
use std::sync::Arc;

use urushi_core::thread::NewThread;
use urushi_core::{
    BranchName, PaneId, ProjectId, RepoId, Slug, Tab, TabId, TaskId, Thread, ThreadId,
};
use urushi_core::{BranchPrefix, IssueKey};
use urushi_mux::TermSize;

use super::task_agent::TaskAgent;
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
    /// Branch to create; `None` generates `<prefix><slug>-<short id>`, or for a Jira
    /// issue `<prefix><KEY>-<slug>-<short id>`.
    pub branch: Option<String>,
    /// Open a first tab with a shell. Clients that let the user pick what to run first
    /// turn this off.
    pub initial_tab: bool,
    /// Project to file the thread under; `None` leaves it ungrouped.
    pub project: Option<ProjectId>,
    /// The task (and maybe subtask) on a board this thread is for; it gets linked to
    /// the thread when the thread is recorded.
    pub task: Option<(TaskId, Option<TaskId>)>,
    /// Open the first tab running the user's task agent, with `task` as its prompt
    /// (ADR 0014). Needs `task`; wins over `initial_tab`.
    pub start_task_agent: bool,
}

impl Service {
    /// Creates a thread: a new branch and worktree from the requested base (or the
    /// repo's default base, or its current branch), plus, if asked, a first tab with a
    /// shell in the worktree. With `update_base_from_remote` on, the base is first
    /// fetched from its remote (ADR 0028).
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
            start_task_agent,
        } = request;
        let branch = parse_branch(branch)?;
        let title = thread_title(&title);
        let (project, settings, worktree_root, issue_key, refresh) = self.read(|s| {
            if let Some(id) = project_id {
                s.project(id)?;
            }
            // Refuse up front, before making a worktree, if the task isn't there.
            let issue_key = task.map(|l| s.task_issue_key(l)).transpose()?.flatten();
            let project = s.repo(repo_id)?.clone();
            let settings = s.effective_settings(repo_id);
            let root = s
                .settings
                .worktree_root
                .clone()
                .unwrap_or_else(|| self.home().join("worktrees"));
            let refresh = s.settings.update_base_from_remote;
            Ok((project, settings, root, issue_key, refresh))
        })?;
        // Resolve the agent and the prompt before making a worktree, so a refusal
        // leaves nothing behind.
        let agent = self
            .task_agent_for(start_task_agent, task, settings.default_shell.clone())
            .await?;

        let id = ThreadId::from_uuid(new_uuid());
        let worktree_path = worktree_dir(&worktree_root, &project, &title, id);

        let git = self.git.clone();
        let repo = project.repo_root.clone();
        let base_ref = base_ref
            .filter(|b| !b.trim().is_empty())
            .or(project.default_base.clone());
        let prefix = settings.branch_prefix.clone();
        let branch = issue_branch(branch, issue_key.as_ref(), &prefix, &title, id)?;
        let created_at = now();
        let (mut thread, preexisting) = blocking(move || {
            let (base, base_commit) = start_point(&*git, &repo, base_ref, refresh)?;
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
            open: initial_tab || agent.is_some(),
            shell: settings.default_shell,
            task,
            agent,
        };
        let started = match self.finish_thread_creation(&mut thread, preexisting, first_tab) {
            Ok(started) => started,
            Err(e) => {
                self.rollback_worktree(&project.repo_root, &thread).await;
                return Err(e);
            }
        };
        if let Some((pane, command_line)) = started {
            self.start_task_agent(thread.id, pane, &command_line).await;
        }
        Ok(thread)
    }

    /// Records the thread, and opens its first tab if asked. Returns the pane and command
    /// line of a task agent still to be typed in.
    fn finish_thread_creation(
        &self,
        thread: &mut Thread,
        preexisting: BTreeSet<BranchName>,
        first: FirstTab,
    ) -> ServiceResult<Option<(PaneId, String)>> {
        thread
            .mark_ready()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let mut first_tab = None;
        let mut to_type = None;
        if first.open {
            let shell = first.shell;
            let tab_id = TabId::from_uuid(new_uuid());
            let pane = PaneId::from_uuid(new_uuid());
            thread.tabs.push(tab_id);
            let env = first.agent.as_ref().map(TaskAgent::env).unwrap_or_default();
            self.spawn_session_with_env(pane, thread, shell, TermSize::DEFAULT, env)?;
            let title = first
                .agent
                .as_ref()
                .map_or(DEFAULT_TAB_TITLE, |a| a.agent.display_name());
            first_tab = Some(Tab::new(tab_id, thread.id, title, pane));
            to_type = first.agent.map(|a| (pane, a.command_line));
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
                && let target = subtask.unwrap_or(task)
                && let Some(board) = state.boards.iter_mut().find(|b| b.task(target).is_some())
                && let Err(e) = board.link_thread(target, thread.id)
            {
                tracing::warn!(thread = %thread.id, error = %e, "thread not linked to its task");
            }
            Ok(())
        });
        if result.is_err() {
            self.kill_sessions(pane);
        }
        result.map(|()| to_type)
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
    task: Option<(TaskId, Option<TaskId>)>,
    /// The task agent the first tab starts, if any.
    agent: Option<TaskAgent>,
}

/// The title to use: the user's, trimmed, or "New thread" when blank.
fn thread_title(title: &str) -> String {
    match title.trim() {
        "" => DEFAULT_THREAD_TITLE.to_owned(),
        t => t.to_owned(),
    }
}

/// Where a thread's worktree goes: `<root>/<repo slug>-<short>/<title slug>-<short>`.
fn worktree_dir(
    root: &std::path::Path,
    repo: &urushi_core::Repo,
    title: &str,
    id: ThreadId,
) -> std::path::PathBuf {
    root.join(format!(
        "{}-{}",
        Slug::from_title(&repo.name),
        repo.id.short()
    ))
    .join(format!("{}-{}", Slug::from_title(title), id.short()))
}

/// With no branch asked for, a thread for a Jira issue gets one named after the issue,
/// which Jira's code integrations link to it (ADR 0026); otherwise `branch` as given.
fn issue_branch(
    branch: Option<BranchName>,
    key: Option<&IssueKey>,
    prefix: &BranchPrefix,
    title: &str,
    id: ThreadId,
) -> ServiceResult<Option<BranchName>> {
    match (branch, key) {
        (None, Some(key)) => BranchName::for_issue_thread(prefix, key, title, id)
            .map(Some)
            .map_err(|e| ServiceError::Invalid(e.to_string())),
        (branch, _) => Ok(branch),
    }
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

/// The base and the commit a new thread starts from: [`resolve_base`], then, with
/// `refresh`, updated from the base's remote so it doesn't start from a stale local copy
/// (ADR 0028).
fn start_point(
    git: &dyn crate::git::Git,
    repo: &std::path::Path,
    base_ref: Option<String>,
    refresh: bool,
) -> ServiceResult<(String, String)> {
    let (base, commit) = resolve_base(git, repo, base_ref)?;
    if !refresh {
        return Ok((base, commit));
    }
    let commit = super::fresh_base::fresh_base(git, repo, &base, commit);
    Ok((base, commit))
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
