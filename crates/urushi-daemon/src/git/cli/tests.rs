//! Tests against real git repositories in temp dirs, with a local bare repo as "origin".

use std::path::{Path, PathBuf};
use std::process::Command;

use urushi_core::BranchName;

use super::super::Git;
use super::CliGit;

pub(crate) struct Fixture {
    _dir: tempfile::TempDir,
    pub(crate) repo: PathBuf,
    pub(crate) remote: PathBuf,
    pub(crate) root: PathBuf,
}

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repo on `main` with one commit, pushed to a bare `origin` whose HEAD is `main`.
pub(crate) fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let remote = root.join("origin.git");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    sh(
        &root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    sh(&repo, &["init", "-q", "-b", "main"]);
    sh(&repo, &["config", "user.email", "test@example.com"]);
    sh(&repo, &["config", "user.name", "Test"]);
    sh(&repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("README"), "hello\n").unwrap();
    sh(&repo, &["add", "README"]);
    sh(&repo, &["commit", "-q", "-m", "initial"]);
    sh(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    sh(&repo, &["push", "-q", "-u", "origin", "main"]);
    sh(&repo, &["remote", "set-head", "origin", "main"]);
    Fixture {
        _dir: dir,
        repo,
        remote,
        root,
    }
}

pub(crate) fn commit_file(dir: &Path, name: &str) {
    std::fs::write(dir.join(name), name).unwrap();
    sh(dir, &["add", name]);
    sh(
        dir,
        &[
            "-c",
            "user.email=t@e.com",
            "-c",
            "user.name=T",
            "commit",
            "-q",
            "-m",
            name,
        ],
    );
}

pub(crate) fn git_in(dir: &Path, args: &[&str]) -> String {
    sh(dir, args)
}

fn branch(name: &str) -> BranchName {
    BranchName::new(name).unwrap()
}

#[test]
fn repo_basics() {
    let f = fixture();
    let git = CliGit::default();
    let nested = f.repo.join("sub");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(git.repo_root(&nested).unwrap(), f.repo);
    assert_eq!(
        git.current_branch(&f.repo).unwrap().as_deref(),
        Some("main")
    );
    assert_eq!(git.local_branches(&f.repo).unwrap(), vec!["main"]);
    assert_eq!(git.resolve_commit(&f.repo, "main").unwrap().len(), 40);
    assert!(git.resolve_commit(&f.repo, "nope").is_err());
    assert!(matches!(
        git.repo_root(&f.root),
        Err(super::GitError::NotARepository(_))
    ));
}

#[test]
fn missing_git_binary_is_reported() {
    let git = CliGit::with_program("/definitely/not/git");
    assert!(matches!(
        git.local_branches(Path::new(".")),
        Err(super::GitError::NotInstalled)
    ));
}

#[test]
fn worktree_lifecycle_and_facts() {
    let f = fixture();
    let git = CliGit::default();
    let base = git.resolve_commit(&f.repo, "main").unwrap();
    let wt = f.root.join("wt");
    git.worktree_add(&f.repo, &wt, &branch("urushi/t-1"), &base)
        .unwrap();

    let worktrees = git.worktrees(&f.repo).unwrap();
    assert_eq!(worktrees.len(), 2);
    assert!(
        worktrees
            .iter()
            .any(|w| w.path == wt && w.branch.as_deref() == Some("urushi/t-1"))
    );

    // Work inside the worktree: a commit, a new branch checked out, dirty files.
    commit_file(&wt, "a.txt");
    git_in(&wt, &["checkout", "-q", "-b", "feature/x"]);
    commit_file(&wt, "b.txt");
    std::fs::write(wt.join("README"), "changed").unwrap();
    std::fs::write(wt.join("new.txt"), "new").unwrap();

    let checked_out = git.branches_checked_out_in(&wt).unwrap();
    assert!(checked_out.contains(&"feature/x".to_owned()));
    assert!(
        !checked_out.contains(&"main".to_owned()),
        "main was never checked out here"
    );

    let counts = git.status_counts(&wt).unwrap();
    assert_eq!((counts.modified, counts.untracked), (1, 1));

    let unpushed = git
        .unpushed_commits(&f.repo, &[branch("urushi/t-1"), branch("feature/x")], &base)
        .unwrap();
    assert_eq!(unpushed, 2);

    git.worktree_remove(&f.repo, &wt).unwrap();
    assert!(!wt.exists());
    git.worktree_prune(&f.repo).unwrap();
    assert_eq!(git.worktrees(&f.repo).unwrap().len(), 1);
}

#[test]
fn remote_facts_and_deletion() {
    let f = fixture();
    let git = CliGit::default();
    git_in(&f.repo, &["checkout", "-q", "-b", "urushi/pushed"]);
    commit_file(&f.repo, "p.txt");
    git_in(&f.repo, &["push", "-q", "-u", "origin", "urushi/pushed"]);
    git_in(&f.repo, &["checkout", "-q", "main"]);

    assert_eq!(git.remote_default_branches(&f.repo).unwrap(), vec!["main"]);
    let remote = git.remote_branches(&f.repo).unwrap();
    assert!(remote.contains(&("origin".into(), "urushi/pushed".into())));
    assert!(!remote.iter().any(|(_, b)| b == "HEAD"));
    let ups = git.upstreams(&f.repo).unwrap();
    assert!(ups.iter().any(|u| u.local == "urushi/pushed"
        && u.remote == "origin"
        && u.remote_branch == "urushi/pushed"));

    git.delete_remote_branch(&f.repo, "origin", "urushi/pushed")
        .unwrap();
    let on_remote = git_in(
        &f.remote,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    );
    assert!(!on_remote.contains("urushi/pushed"));
    assert!(on_remote.contains("main"));

    git.delete_branch(&f.repo, &branch("urushi/pushed"))
        .unwrap();
    assert_eq!(git.local_branches(&f.repo).unwrap(), vec!["main"]);
}

/// Clones `origin` into `<root>/<name>`, commits `file` on `branch` there and pushes it.
/// Returns the pushed commit.
fn push_from_another_clone(f: &Fixture, name: &str, branch: &str, file: &str) -> String {
    let clone = f.root.join(name);
    if !clone.exists() {
        sh(
            &f.root,
            &[
                "clone",
                "-q",
                f.remote.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
    }
    sh(&clone, &["fetch", "-q", "origin"]);
    sh(
        &clone,
        &["checkout", "-q", "-B", branch, &format!("origin/{branch}")],
    );
    commit_file(&clone, file);
    sh(&clone, &["push", "-q", "origin", branch]);
    sh(&clone, &["rev-parse", "HEAD"]).trim().to_owned()
}

#[test]
fn fetching_one_branch_updates_only_its_remote_tracking_ref() {
    let f = fixture();
    let git = CliGit::default();
    let before = git.resolve_commit(&f.repo, "main").unwrap();
    let pushed = push_from_another_clone(&f, "other", "main", "new.txt");

    git.fetch_branch(&f.repo, "origin", "main").unwrap();
    assert_eq!(git.resolve_commit(&f.repo, "origin/main").unwrap(), pushed);
    assert_eq!(
        git.resolve_commit(&f.repo, "main").unwrap(),
        before,
        "the local branch doesn't move"
    );
    assert!(
        !f.repo.join("new.txt").exists(),
        "the checkout isn't touched"
    );

    assert!(
        git.fetch_branch(&f.repo, "origin", "no-such-branch")
            .is_err()
    );
    assert!(git.fetch_branch(&f.repo, "nowhere", "main").is_err());
}

#[test]
fn ancestry_and_fast_forwards() {
    let f = fixture();
    let git = CliGit::default();
    let old = git.resolve_commit(&f.repo, "main").unwrap();
    let new = push_from_another_clone(&f, "other", "main", "new.txt");
    git.fetch_branch(&f.repo, "origin", "main").unwrap();

    assert!(git.is_ancestor(&f.repo, &old, &new).unwrap());
    assert!(
        git.is_ancestor(&f.repo, &old, &old).unwrap(),
        "a commit is its own"
    );
    assert!(!git.is_ancestor(&f.repo, &new, &old).unwrap());

    git_in(&f.repo, &["branch", "release", &old]);
    let release = branch("release");
    assert!(
        git.fast_forward_branch(&f.repo, &release, &new, &new)
            .is_err(),
        "refused: the branch isn't where the caller thinks"
    );
    assert_eq!(git.resolve_commit(&f.repo, "release").unwrap(), old);
    git.fast_forward_branch(&f.repo, &release, &new, &old)
        .unwrap();
    assert_eq!(git.resolve_commit(&f.repo, "release").unwrap(), new);
}

#[test]
fn existing_branches_get_worktrees_and_remote_only_ones_are_tracked() {
    let f = fixture();
    let git = CliGit::default();
    // A local branch that isn't checked out anywhere.
    git_in(&f.repo, &["branch", "develop"]);
    let wt = f.root.join("develop");
    git.worktree_add_existing(&f.repo, &wt, &branch("develop"))
        .unwrap();
    assert_eq!(git.current_branch(&wt).unwrap().as_deref(), Some("develop"));

    // A branch only the remote has becomes a local branch tracking it.
    git_in(&f.repo, &["push", "-q", "origin", "main:release"]);
    git_in(&f.repo, &["fetch", "-q", "origin"]);
    assert!(
        !git.local_branches(&f.repo)
            .unwrap()
            .contains(&"release".into())
    );
    let wt2 = f.root.join("release");
    git.worktree_add_existing(&f.repo, &wt2, &branch("release"))
        .unwrap();
    assert_eq!(
        git.current_branch(&wt2).unwrap().as_deref(),
        Some("release")
    );
    assert!(
        git.upstreams(&f.repo)
            .unwrap()
            .iter()
            .any(|u| u.local == "release" && u.remote == "origin" && u.remote_branch == "release")
    );

    // A tracking branch can also be made without a worktree.
    git_in(&f.repo, &["push", "-q", "origin", "main:hotfix"]);
    git_in(&f.repo, &["fetch", "-q", "origin"]);
    git.create_tracking_branch(&f.repo, &branch("hotfix"), "origin/hotfix")
        .unwrap();
    assert!(
        git.upstreams(&f.repo)
            .unwrap()
            .iter()
            .any(|u| u.local == "hotfix" && u.remote_branch == "hotfix")
    );

    // Git refuses a branch checked out elsewhere (main is, in the repo itself).
    let wt3 = f.root.join("main-again");
    assert!(
        git.worktree_add_existing(&f.repo, &wt3, &branch("main"))
            .is_err()
    );
    assert!(!wt3.exists());
}
