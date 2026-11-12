//! Creating and renaming threads. Every thread gets its own worktree on a new branch
//! (ADR 0004); a project's parent-branch thread is made in `project_base` (ADR 0029).

use std::collections::BTreeSet;
use std::sync::Arc;

use urushi_core::thread::NewThread;
use urushi_core::{
    BranchName, PaneId, ProjectId, RepoId, Slug, Tab, TabId, TaskId, Thread, ThreadId,
};
use urushi_core::{BranchPrefix, IssueKey};
use urushi_mux::TermSize;

use super::task_agent::TaskAgent;
use super::thread_base::{WorktreePlan, make_worktree, start_ref};
use super::timing::Stopwatch;
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
    /// Ref to branch from; `None` uses the parent branch of `project` (when it is in this
    /// repo), else the repo's default base, else its current branch.
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
    /// Creates a thread: a new branch and worktree from the requested base (or its
    /// project's parent branch, the repo's default base, or its current branch), plus, if
    /// asked, a first tab with a shell in the worktree. The first thread of a project
    /// without a parent branch makes its base the project's parent branch.
    /// With `update_base_from_remote` on, the base is first fetched from its remote
    /// (ADR 0028).
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
        let mut watch = Stopwatch::start("create thread");
        let branch = parse_branch(branch)?;
        let title = thread_title(&title);
        let (project, settings, worktree_root, issue_key, parent, refresh) = self.read(|s| {
            let parent = match project_id {
                Some(id) => s.project(id)?.settings.base_in(repo_id).cloned(),
                None => None,
            };
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
            Ok((project, settings, root, issue_key, parent, refresh))
        })?;
        // Resolve the agent and the prompt before making a worktree, so a refusal
        // leaves nothing behind.
        let agent = self
            .task_agent_for(start_task_agent, task, settings.default_shell.clone())
            .await?;
        watch.lap("prepare");

        let id = ThreadId::from_uuid(new_uuid());
        let prefix = settings.branch_prefix.clone();
        let plan = WorktreePlan {
            spec: NewThread {
                id,
                repo: repo_id,
                project: project_id,
                worktree_path: worktree_dir(&worktree_root, &project, &title, id),
                base_ref: String::new(),
                base_commit: String::new(),
                created_at: now(),
                branch: issue_branch(branch, issue_key.as_ref(), &prefix, &title, id)?,
                title,
            },
            prefix,
            repo: project.repo_root.clone(),
            base_ref: start_ref(base_ref, parent, project.default_base.clone()),
            refresh,
        };
        let (git, fetches) = (Arc::clone(&self.git), Arc::clone(&self.base_fetches));
        let (mut thread, preexisting, mut watch) = blocking(move || {
            let made = make_worktree(&*git, &fetches, plan, &mut watch)?;
            Ok((made.0, made.1, watch))
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
        watch.lap("record");
        if let Some((pane, agent)) = started {
            self.start_task_agent(thread.id, pane, agent).await;
            watch.lap("start task agent");
        }
        // A project's first thread may open its parent-branch thread: a second worktree
        // the new thread's reply needn't wait for.
        if thread.project.is_some() {
            let (service, created) = (Arc::clone(self), thread.clone());
            tokio::spawn(async move { service.adopt_project_base(&created).await });
        }
        watch.lap("total");
        Ok(thread)
    }

    /// Records the thread, and opens its first tab if asked. Returns the pane and the
    /// task agent still to be started in it.
    fn finish_thread_creation(
        &self,
        thread: &mut Thread,
        preexisting: BTreeSet<BranchName>,
        first: FirstTab,
    ) -> ServiceResult<Option<(PaneId, TaskAgent)>> {
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
            self.spawn_session_with_env(pane, &*thread, shell, TermSize::DEFAULT, env)?;
            let title = first
                .agent
                .as_ref()
                .map_or(DEFAULT_TAB_TITLE, |a| a.agent.display_name());
            first_tab = Some(Tab::new(tab_id, thread.id, title, pane));
            to_type = first.agent.map(|a| (pane, a));
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
pub(super) fn worktree_dir(
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
