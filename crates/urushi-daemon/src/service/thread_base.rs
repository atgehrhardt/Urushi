//! Where a new thread starts, and making its worktree there (ADR 0028, ADR 0055).
//!
//! A thread starts from the ref asked for, its project's parent branch, the repo's
//! default base or its current branch, updated from the remote when that is on. The
//! GUI can ask for that update ahead of time (`PrepareThreadBase`), while the user still
//! types the thread's title, and the create then reuses the fetch.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use urushi_core::thread::NewThread;
use urushi_core::{BranchName, BranchPrefix, ProjectId, RepoId, Thread};

use super::base_fetch::BaseFetches;
use super::timing::Stopwatch;
use super::{Service, ServiceError, ServiceResult, fresh_base};
use crate::git::{Git, GitError};

impl Service {
    /// Starts fetching the base a new thread in `repo` (and `project`) would start from,
    /// so creating it can reuse the fetch. Answers at once; the fetch runs in the
    /// background, at most a few at a time, and only with `update_base_from_remote` on.
    pub fn prepare_thread_base(
        self: &Arc<Self>,
        repo: RepoId,
        base_ref: Option<String>,
        project: Option<ProjectId>,
    ) -> ServiceResult<()> {
        let (root, base, refresh) = self.read(|s| {
            let parent = match project {
                Some(id) => s.project(id)?.settings.base_in(repo).cloned(),
                None => None,
            };
            let r = s.repo(repo)?;
            let base = start_ref(base_ref, parent, r.default_base.clone());
            Ok((
                r.repo_root.clone(),
                base,
                s.settings.update_base_from_remote,
            ))
        })?;
        if !refresh {
            return Ok(());
        }
        let Some(slot) = self.base_fetches.prefetch_slot() else {
            tracing::debug!("enough bases are being fetched already; not fetching another");
            return Ok(());
        };
        let git = Arc::clone(&self.git);
        tokio::task::spawn_blocking(move || {
            let mut watch = Stopwatch::start("prepare thread base");
            let outcome = resolve_base(&*git, &root, base).and_then(|(base, _)| {
                fresh_base::prefetch(&*git, slot.fetches(), &root, &base).map_err(Into::into)
            });
            match outcome {
                Ok(Some(origin)) => watch.lap_with("fetch", origin.as_str()),
                Ok(None) => watch.lap_with("fetch", "untracked"),
                Err(e) => {
                    tracing::debug!(error = %e, "couldn't fetch a new thread's base ahead");
                    watch.lap_with("fetch", "failed");
                }
            }
        });
        Ok(())
    }
}

/// Everything needed to make a new thread's worktree, off the async runtime.
pub(super) struct WorktreePlan {
    pub spec: NewThread,
    pub prefix: BranchPrefix,
    pub repo: PathBuf,
    /// The ref to start from; `None` uses the repo's current branch.
    pub base_ref: Option<String>,
    /// Update the base from its remote first (ADR 0028).
    pub refresh: bool,
}

/// Resolves where a thread starts, then makes its branch and worktree. Returns the
/// thread (still `Creating`) and the branches that existed before it. Blocking.
pub(super) fn make_worktree(
    git: &dyn Git,
    fetches: &BaseFetches,
    plan: WorktreePlan,
    watch: &mut Stopwatch,
) -> ServiceResult<(Thread, BTreeSet<BranchName>)> {
    let WorktreePlan {
        mut spec,
        prefix,
        repo,
        base_ref,
        refresh,
    } = plan;
    let (base, commit) = resolve_base(git, &repo, base_ref)?;
    watch.lap("resolve base");
    let commit = if refresh {
        let (commit, outcome) = fresh_base::fresh_base(git, fetches, &repo, &base, commit);
        watch.lap_with("update base", outcome);
        commit
    } else {
        commit
    };
    spec.base_ref = base;
    spec.base_commit = commit;
    let preexisting: BTreeSet<BranchName> = git
        .local_branches(&repo)?
        .into_iter()
        .filter_map(|b| BranchName::new(b).ok())
        .collect();
    let thread = Thread::new(spec, &prefix).map_err(|e| ServiceError::Invalid(e.to_string()))?;
    if preexisting.contains(&thread.primary_branch) {
        return Err(ServiceError::Conflict(format!(
            "branch {} already exists",
            thread.primary_branch
        )));
    }
    if let Some(parent) = thread.worktree_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ServiceError::Internal(e.to_string()))?;
    }
    watch.lap("list branches");
    git.worktree_add(
        &repo,
        &thread.worktree_path,
        &thread.primary_branch,
        &thread.base_commit,
    )?;
    watch.lap("add worktree");
    Ok((thread, preexisting))
}

/// What a thread starts from: the ref asked for, else its project's parent branch (in
/// this repo), else the repo's default base. `None` leaves it to [`resolve_base`].
pub(super) fn start_ref(
    requested: Option<String>,
    parent: Option<BranchName>,
    repo_default: Option<String>,
) -> Option<String> {
    requested
        .filter(|b| !b.trim().is_empty())
        .or_else(|| parent.map(|b| b.to_string()))
        .or(repo_default)
}

/// The ref to branch from (`base_ref`, else the repo's current branch, else `HEAD`) and
/// the commit it points to. A ref that doesn't exist is the user's mistake, so it is
/// reported as such rather than as a git failure.
fn resolve_base(
    git: &dyn Git,
    repo: &Path,
    base_ref: Option<String>,
) -> ServiceResult<(String, String)> {
    let base = match base_ref {
        Some(base) => base,
        None => git.current_branch(repo)?.unwrap_or_else(|| "HEAD".into()),
    };
    match git.resolve_commit(repo, &base) {
        Ok(commit) => Ok((base, commit)),
        Err(GitError::Failed { .. }) => Err(ServiceError::Invalid(format!(
            "cannot start from {base:?}: no branch, tag or commit has that name"
        ))),
        Err(e) => Err(e.into()),
    }
}
