//! Git operations the daemon needs, behind the [`Git`] trait (ADR 0004).
//!
//! The only implementation, [`CliGit`], runs the user's `git` binary. That way their
//! config, hooks and credential helpers apply, and we link no git library. Methods are
//! synchronous. Call them from `spawn_blocking`.

mod cli;

use std::path::{Path, PathBuf};

pub use cli::CliGit;
use thiserror::Error;
use urushi_core::BranchName;

/// Git failures.
#[derive(Debug, Clone, Error)]
pub enum GitError {
    /// `git` isn't installed or isn't on `PATH`.
    #[error("git is not installed or not on PATH")]
    NotInstalled,
    /// The path isn't inside a git repository.
    #[error("{0} is not inside a git repository")]
    NotARepository(PathBuf),
    /// A git command failed.
    #[error("`git {command}` failed: {message}")]
    Failed {
        /// The arguments, for diagnostics.
        command: String,
        /// Git's error output.
        message: String,
    },
    /// A git command took too long (e.g. a push to an unreachable remote).
    #[error("`git {0}` timed out")]
    TimedOut(String),
}

/// A worktree of a repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeInfo {
    /// Its directory.
    pub path: PathBuf,
    /// The branch checked out there, if any (not detached).
    pub branch: Option<String>,
}

/// Uncommitted changes in a worktree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatusCounts {
    /// Tracked files with changes (staged or not).
    pub modified: u32,
    /// Untracked files.
    pub untracked: u32,
}

/// A local branch's configured upstream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upstream {
    /// Local branch.
    pub local: String,
    /// Remote name.
    pub remote: String,
    /// Branch name on the remote.
    pub remote_branch: String,
}

/// Everything the daemon asks of git.
pub trait Git: Send + Sync {
    /// The top-level directory of the repository containing `path`.
    fn repo_root(&self, path: &Path) -> Result<PathBuf, GitError>;
    /// The branch checked out in `repo`, or `None` if HEAD is detached.
    fn current_branch(&self, repo: &Path) -> Result<Option<String>, GitError>;
    /// Resolves `rev` to a full commit id.
    fn resolve_commit(&self, repo: &Path, rev: &str) -> Result<String, GitError>;
    /// Local branch names.
    fn local_branches(&self, repo: &Path) -> Result<Vec<String>, GitError>;
    /// Creates `path` as a new worktree on a new branch `branch` starting at `base_commit`.
    fn worktree_add(
        &self,
        repo: &Path,
        path: &Path,
        branch: &BranchName,
        base_commit: &str,
    ) -> Result<(), GitError>;
    /// Removes a worktree, discarding uncommitted changes.
    fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), GitError>;
    /// Forgets worktrees whose directories no longer exist.
    fn worktree_prune(&self, repo: &Path) -> Result<(), GitError>;
    /// All worktrees, including the main one.
    fn worktrees(&self, repo: &Path) -> Result<Vec<WorktreeInfo>, GitError>;
    /// Branches checked out in `worktree` over its lifetime, from its own HEAD reflog,
    /// plus the current one.
    fn branches_checked_out_in(&self, worktree: &Path) -> Result<Vec<String>, GitError>;
    /// Uncommitted changes in `worktree`.
    fn status_counts(&self, worktree: &Path) -> Result<StatusCounts, GitError>;
    /// Commits on `branches` that aren't on any remote and aren't reachable from `base_commit`.
    fn unpushed_commits(
        &self,
        repo: &Path,
        branches: &[BranchName],
        base_commit: &str,
    ) -> Result<u32, GitError>;
    /// Remote-tracking branches as `(remote, branch)`, excluding `HEAD`.
    fn remote_branches(&self, repo: &Path) -> Result<Vec<(String, String)>, GitError>;
    /// Default branches of remotes (from `refs/remotes/*/HEAD`), e.g. `main`.
    fn remote_default_branches(&self, repo: &Path) -> Result<Vec<String>, GitError>;
    /// Configured upstreams of local branches.
    fn upstreams(&self, repo: &Path) -> Result<Vec<Upstream>, GitError>;
    /// Force-deletes a local branch.
    fn delete_branch(&self, repo: &Path, branch: &BranchName) -> Result<(), GitError>;
    /// Deletes a branch on a remote (`git push <remote> --delete <branch>`).
    fn delete_remote_branch(&self, repo: &Path, remote: &str, branch: &str)
    -> Result<(), GitError>;
    /// Fetches one branch of a remote into its remote-tracking ref
    /// (`refs/remotes/<remote>/<branch>`), and nothing else: no other branches, no tags.
    /// Local branches and checkouts are untouched.
    fn fetch_branch(&self, repo: &Path, remote: &str, branch: &str) -> Result<(), GitError>;
    /// Whether `ancestor` is reachable from `descendant` (a commit is its own ancestor).
    fn is_ancestor(&self, repo: &Path, ancestor: &str, descendant: &str) -> Result<bool, GitError>;
    /// Moves local `branch` from `old` to `new`, refusing if it no longer points at `old`.
    /// The caller checks that this is a fast-forward and that no worktree has the branch
    /// checked out: the ref moves, no files do.
    fn fast_forward_branch(
        &self,
        repo: &Path,
        branch: &BranchName,
        new: &str,
        old: &str,
    ) -> Result<(), GitError>;
}
