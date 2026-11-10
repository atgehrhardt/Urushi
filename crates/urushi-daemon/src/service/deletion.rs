//! Deleting a thread, in two steps: plan, then confirm with the plan's token.
//!
//! [`Service::plan_thread_deletion`] collects everything deletion would destroy and
//! returns it with a token. The client must show that plan to the user. Then
//! [`Service::delete_thread`] re-computes the plan and refuses unless the token still
//! matches, so nothing can change between the warning and the deletion.
//!
//! Which branches may be deleted is decided by `urushi_core::owned_branches`. This
//! module only gathers the git facts. A project's parent-branch thread (ADR 0029) owns
//! no branch, and when it runs in a checkout Urushi didn't make, deleting it leaves
//! everything on disk alone; deleting it switches it off in its project's settings.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use urushi_core::{BranchName, OwnershipFacts, ThreadId, owned_branches};

use super::{Service, ServiceError, ServiceResult, blocking};
use crate::git::{Git, StatusCounts};
use crate::store::ThreadRecord;

/// Branch names never deleted, even if a thread somehow appears to own them.
const ALWAYS_PROTECTED: [&str; 4] = ["main", "master", "develop", "trunk"];

/// Everything a thread deletion would destroy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletionPlan {
    /// The thread.
    pub thread: ThreadId,
    /// Its worktree directory.
    pub worktree_path: PathBuf,
    /// Uncommitted changes that will be lost.
    pub changes: StatusCounts,
    /// Commits on the thread's branches that exist nowhere else.
    pub unpushed_commits: u32,
    /// Local branches to delete.
    pub local_branches: Vec<BranchName>,
    /// `(remote, branch)` pairs to delete on remotes.
    pub remote_branches: Vec<(String, String)>,
    /// Terminals that will be killed.
    pub running_panes: u32,
    /// Whether remote branches are being deleted (from settings).
    pub delete_remote_branches: bool,
    /// Problems found while planning that don't block deletion.
    pub warnings: Vec<String>,
    /// The worktree is a checkout Urushi didn't create, so it is kept.
    pub keeps_worktree: bool,
    /// Fingerprint of everything above. See [`DeletionPlan::token`].
    pub token: String,
}

impl DeletionPlan {
    /// FNV-1a fingerprint of the plan's contents. It is a consistency check, not a
    /// secret. Any change to what would be deleted changes the token.
    fn fingerprint(&self) -> String {
        let canonical = format!(
            "{}|{}|{}/{}|{}|{:?}|{:?}|{}|{}|{}",
            self.thread,
            self.worktree_path.display(),
            self.changes.modified,
            self.changes.untracked,
            self.unpushed_commits,
            self.local_branches,
            self.remote_branches,
            self.running_panes,
            self.delete_remote_branches,
            self.keeps_worktree,
        );
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in canonical.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{hash:016x}")
    }
}

struct PlanInputs {
    record: ThreadRecord,
    repo: PathBuf,
    delete_remote: bool,
    running_panes: u32,
    /// Parent branches of projects in this repo: never deleted with any thread.
    project_bases: Vec<BranchName>,
}

impl Service {
    /// Computes what deleting `thread` would destroy.
    pub async fn plan_thread_deletion(&self, thread: ThreadId) -> ServiceResult<DeletionPlan> {
        let inputs = self.read(|s| {
            let record = s.thread(thread)?.clone();
            let repo = s.repo(record.thread.repo)?.repo_root.clone();
            let delete_remote = s
                .effective_settings(record.thread.repo)
                .delete_remote_branches;
            let project_bases = s
                .projects
                .iter()
                .filter_map(|p| p.settings.base_in(record.thread.repo).cloned())
                .collect();
            let running = {
                let sessions = super::lock(&self.sessions);
                s.thread_panes(thread)
                    .iter()
                    .filter(|p| sessions.contains_key(p))
                    .count()
            };
            Ok(PlanInputs {
                record,
                repo,
                delete_remote,
                running_panes: u32::try_from(running).unwrap_or(u32::MAX),
                project_bases,
            })
        })?;
        let git = self.git.clone();
        blocking(move || Ok(compute_plan(&*git, &inputs))).await
    }

    /// Deletes `thread` if `token` matches a freshly computed plan: kills its terminals,
    /// removes its worktree, and deletes its owned local (and, if enabled, remote)
    /// branches. A partial failure leaves the thread in `DeletionFailed`, ready to retry.
    pub async fn delete_thread(
        self: &Arc<Self>,
        thread: ThreadId,
        token: &str,
    ) -> ServiceResult<()> {
        let plan = self.plan_thread_deletion(thread).await?;
        if plan.token != token {
            return Err(ServiceError::Conflict(
                "the thread changed since the deletion warning was shown; review it again".into(),
            ));
        }
        let repo = self.read(|s| Ok(s.repo(s.thread(thread)?.thread.repo)?.repo_root.clone()))?;

        let panes = self.mutate(|state| {
            state
                .thread_mut(thread)?
                .thread
                .begin_deletion()
                .map_err(|e| ServiceError::Precondition(e.to_string()))?;
            let tabs: Vec<_> = state
                .tabs
                .iter()
                .filter(|t| t.thread() == thread)
                .map(urushi_core::Tab::id)
                .collect();
            Ok(tabs
                .into_iter()
                .flat_map(|t| state.remove_tab(t))
                .collect::<Vec<_>>())
        })?;
        self.kill_sessions(panes);

        let git = self.git.clone();
        let failures = blocking(move || Ok(execute_plan(&*git, &repo, &plan))).await?;

        self.mutate(|state| {
            if failures.is_empty() {
                let gone = state.thread(thread)?.thread.clone();
                state.threads.retain(|r| r.thread.id != thread);
                // Closing a parent-branch thread is switching it off.
                if gone.is_project_base()
                    && let Some(project) = gone.project
                    && let Ok(project) = state.project_mut(project)
                {
                    project.settings.base_thread = false;
                }
                // Tasks started as this thread can be started again.
                for board in &mut state.boards {
                    board.unlink_thread(thread);
                }
            } else {
                let reason = failures.join("; ");
                state
                    .thread_mut(thread)?
                    .thread
                    .deletion_failed(reason)
                    .map_err(|e| ServiceError::Internal(e.to_string()))?;
            }
            Ok(())
        })?;
        Ok(())
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| crate::platform::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    a == b || canon(a) == canon(b)
}

fn valid_branches(names: impl IntoIterator<Item = String>) -> BTreeSet<BranchName> {
    names
        .into_iter()
        .filter_map(|n| BranchName::new(n).ok())
        .collect()
}

fn compute_plan(git: &dyn Git, inputs: &PlanInputs) -> DeletionPlan {
    let thread = &inputs.record.thread;
    let worktree = &thread.worktree_path;
    let mut warnings = Vec::new();
    let keeps_worktree = thread.is_attached();
    // A kept checkout loses nothing, so its state doesn't matter.
    let exists = worktree.is_dir() && !keeps_worktree;
    if !exists && !keeps_worktree {
        warnings.push(format!(
            "worktree directory {} is already missing",
            worktree.display()
        ));
    }
    let mut warn = |what: &str, e: &dyn std::fmt::Display| warnings.push(format!("{what}: {e}"));
    let changes = if exists {
        git.status_counts(worktree).unwrap_or_else(|e| {
            warn("could not read worktree status", &e);
            StatusCounts::default()
        })
    } else {
        StatusCounts::default()
    };
    let checked_out_in_worktree: Vec<BranchName> = if exists {
        git.branches_checked_out_in(worktree)
            .map(|b| valid_branches(b).into_iter().collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let existing = valid_branches(git.local_branches(&inputs.repo).unwrap_or_else(|e| {
        warn("could not list branches", &e);
        Vec::new()
    }));
    let checked_out_elsewhere = valid_branches(
        git.worktrees(&inputs.repo)
            .unwrap_or_default()
            .into_iter()
            .filter(|w| !same_path(&w.path, worktree))
            .filter_map(|w| w.branch),
    );
    let remote_defaults = git
        .remote_default_branches(&inputs.repo)
        .unwrap_or_default();
    let mut protected = valid_branches(
        remote_defaults
            .into_iter()
            .chain(ALWAYS_PROTECTED.map(String::from)),
    );
    if let Ok(base) = BranchName::new(thread.base_ref.clone()) {
        protected.insert(base);
    }
    protected.extend(inputs.project_bases.iter().cloned());

    let local_branches = if thread.is_project_base() {
        Vec::new() // It works on a branch that was there before it.
    } else {
        owned_branches(&OwnershipFacts {
            primary: Some(thread.primary_branch.clone()),
            checked_out_in_worktree,
            preexisting: inputs.record.preexisting_branches.clone(),
            protected: protected.clone(),
            checked_out_elsewhere,
            existing,
        })
    };

    let unpushed_commits = git
        .unpushed_commits(&inputs.repo, &local_branches, &thread.base_commit)
        .unwrap_or_else(|e| {
            warn("could not count unpushed commits", &e);
            0
        });

    let remote_branches = if inputs.delete_remote {
        remote_branches_for(git, &inputs.repo, &local_branches, &protected)
    } else {
        Vec::new()
    };

    let mut plan = DeletionPlan {
        thread: thread.id,
        worktree_path: worktree.clone(),
        changes,
        unpushed_commits,
        local_branches,
        remote_branches,
        running_panes: inputs.running_panes,
        delete_remote_branches: inputs.delete_remote,
        warnings,
        keeps_worktree,
        token: String::new(),
    };
    plan.token = plan.fingerprint();
    plan
}

/// Remote branches that correspond to owned local branches: their configured upstream,
/// or a same-named branch on any remote. Only branches that actually exist on the remote
/// (as remote-tracking refs) count, and protected names are never included.
fn remote_branches_for(
    git: &dyn Git,
    repo: &Path,
    owned: &[BranchName],
    protected: &BTreeSet<BranchName>,
) -> Vec<(String, String)> {
    let known: BTreeSet<(String, String)> = git
        .remote_branches(repo)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let upstreams = git.upstreams(repo).unwrap_or_default();
    let is_protected = |name: &str| BranchName::new(name).is_ok_and(|b| protected.contains(&b));
    let mut out = BTreeSet::new();
    for branch in owned {
        for up in upstreams.iter().filter(|u| u.local == branch.as_str()) {
            out.insert((up.remote.clone(), up.remote_branch.clone()));
        }
        for (remote, name) in &known {
            if name == branch.as_str() {
                out.insert((remote.clone(), name.clone()));
            }
        }
    }
    out.into_iter()
        .filter(|pair| known.contains(pair) && !is_protected(&pair.1))
        .collect()
}

/// Carries out a plan. Returns a description of each step that failed.
fn execute_plan(git: &dyn Git, repo: &Path, plan: &DeletionPlan) -> Vec<String> {
    let mut failures = Vec::new();
    if !plan.keeps_worktree
        && plan.worktree_path.is_dir()
        && let Err(e) = git.worktree_remove(repo, &plan.worktree_path)
    {
        failures.push(format!("removing worktree: {e}"));
    }
    if let Err(e) = git.worktree_prune(repo) {
        failures.push(format!("pruning worktrees: {e}"));
    }
    for branch in &plan.local_branches {
        if let Err(e) = git.delete_branch(repo, branch) {
            failures.push(format!("deleting branch {branch}: {e}"));
        }
    }
    for (remote, branch) in &plan.remote_branches {
        if let Err(e) = git.delete_remote_branch(repo, remote, branch) {
            failures.push(format!("deleting {remote}/{branch}: {e}"));
        }
    }
    failures
}
