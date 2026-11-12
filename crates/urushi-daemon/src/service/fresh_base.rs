//! Starting a new thread from its base's state on the remote (ADR 0028).
//!
//! Before a thread's worktree is made, the base's branch is fetched from the remote it
//! tracks, and the thread starts from the remote's commit when the local branch is
//! merely behind it. The user's checkouts never change: a local branch is only
//! fast-forwarded when no worktree has it checked out. This is best effort. When there
//! is nothing to fetch, or the fetch fails, the thread starts from the local commit.

use std::path::Path;

use urushi_core::BranchName;

use super::base_fetch::{BaseFetches, FetchKey, FetchOrigin};
use crate::git::{Git, GitError};

/// The commit a thread on `base` should start from: `local_commit` (what `base` resolves
/// to locally), or the remote's newer commit for it. The fetch is shared with a recent or
/// running one of the same branch (`base_fetch`). Also says how the fetch went, for logs.
pub(super) fn fresh_base(
    git: &dyn Git,
    fetches: &BaseFetches,
    repo: &Path,
    base: &str,
    local_commit: String,
) -> (String, &'static str) {
    let mut outcome = "skipped";
    match refresh(git, fetches, repo, base, &local_commit, &mut outcome) {
        Ok(Some(commit)) => (commit, outcome),
        Ok(None) => (local_commit, outcome),
        Err(e) => {
            tracing::warn!(base, error = %e, "couldn't update the base from its remote; starting from the local commit");
            (local_commit, "failed")
        }
    }
}

/// Fetches `base`'s branch ahead of a new thread on it, so the thread can reuse the
/// fetch (ADR 0055). `None` when the base doesn't track a remote.
pub(super) fn prefetch(
    git: &dyn Git,
    fetches: &BaseFetches,
    repo: &Path,
    base: &str,
) -> Result<Option<FetchOrigin>, GitError> {
    let Some(tracked) = tracking(git, repo, base)? else {
        return Ok(None);
    };
    fetch(git, fetches, repo, &tracked).map(Some)
}

fn fetch(
    git: &dyn Git,
    fetches: &BaseFetches,
    repo: &Path,
    tracked: &Tracked,
) -> Result<FetchOrigin, GitError> {
    let key = FetchKey {
        repo: repo.to_path_buf(),
        remote: tracked.remote.clone(),
        branch: tracked.branch.clone(),
    };
    fetches.fetch(&key, || {
        git.fetch_branch(repo, &tracked.remote, &tracked.branch)
    })
}

/// Where a base is kept on a remote.
struct Tracked {
    remote: String,
    branch: String,
    /// The local branch tracking it; `None` when the base is a remote-tracking ref.
    local: Option<String>,
}

/// Fetches the base and returns the remote's commit when the thread should start there.
fn refresh(
    git: &dyn Git,
    fetches: &BaseFetches,
    repo: &Path,
    base: &str,
    local_commit: &str,
    outcome: &mut &'static str,
) -> Result<Option<String>, GitError> {
    let Some(tracked) = tracking(git, repo, base)? else {
        return Ok(None);
    };
    *outcome = fetch(git, fetches, repo, &tracked)?.as_str();
    let remote_ref = format!("refs/remotes/{}/{}", tracked.remote, tracked.branch);
    let remote_commit = git.resolve_commit(repo, &remote_ref)?;
    let Some(local) = tracked.local else {
        return Ok(Some(remote_commit));
    };
    if git.is_ancestor(repo, &remote_commit, local_commit)? {
        return Ok(None); // up to date, or ahead with commits of the user's own
    }
    if !git.is_ancestor(repo, local_commit, &remote_commit)? {
        tracing::warn!(
            base,
            "the base has diverged from its remote; starting from the local commit"
        );
        return Ok(None);
    }
    fast_forward_if_free(git, repo, &local, &remote_commit, local_commit);
    Ok(Some(remote_commit))
}

/// What to fetch for `base`: the upstream of a local branch, or the remote branch a
/// remote-tracking name like `origin/main` stands for. `None` for anything else (a tag,
/// a commit, `HEAD`, a branch without an upstream).
fn tracking(git: &dyn Git, repo: &Path, base: &str) -> Result<Option<Tracked>, GitError> {
    if let Some(up) = git.upstreams(repo)?.into_iter().find(|u| u.local == base) {
        return Ok(Some(Tracked {
            remote: up.remote,
            branch: up.remote_branch,
            local: Some(up.local),
        }));
    }
    // A local branch shadows a remote-tracking ref of the same name.
    if git.local_branches(repo)?.iter().any(|b| b == base) {
        return Ok(None);
    }
    Ok(git
        .remote_branches(repo)?
        .into_iter()
        .find(|(remote, branch)| base.strip_prefix(remote.as_str()) == Some(&format!("/{branch}")))
        .map(|(remote, branch)| Tracked {
            remote,
            branch,
            local: None,
        }))
}

/// Moves `branch` up to `new` unless a worktree has it checked out: moving a checked-out
/// branch would leave its files behind its ref. Failing is harmless, so it's only logged.
fn fast_forward_if_free(git: &dyn Git, repo: &Path, branch: &str, new: &str, old: &str) {
    let result = git.worktrees(repo).and_then(|worktrees| {
        if worktrees
            .iter()
            .any(|w| w.branch.as_deref() == Some(branch))
        {
            return Ok(());
        }
        match BranchName::new(branch) {
            Ok(name) => git.fast_forward_branch(repo, &name, new, old),
            Err(_) => Ok(()),
        }
    });
    if let Err(e) = result {
        tracing::warn!(branch, error = %e, "couldn't fast-forward the base branch");
    }
}
