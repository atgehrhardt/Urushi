//! End-to-end: a new thread starts from its base's state on the remote, without touching
//! the user's checkout (ADR 0028).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::path::{Path, PathBuf};

use common::*;
use urushi_client::{Client, v1};

fn rev(dir: &Path, rev: &str) -> String {
    git(dir, &["rev-parse", rev]).trim().to_owned()
}

/// Commits `file` on `branch` in a second clone of `origin` and pushes it, as a teammate
/// would. Returns the pushed commit.
fn push_from_teammate(f: &Fixture, branch: &str, file: &str) -> String {
    let clone = f.root.join("teammate");
    if !clone.exists() {
        git(
            &f.root,
            &[
                "clone",
                "-q",
                f.remote.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        git(&clone, &["config", "user.email", "mate@example.com"]);
        git(&clone, &["config", "user.name", "Mate"]);
        git(&clone, &["config", "commit.gpgsign", "false"]);
    }
    git(&clone, &["fetch", "-q", "origin"]);
    git(
        &clone,
        &["checkout", "-q", "-B", branch, &format!("origin/{branch}")],
    );
    commit(&clone, file);
    git(&clone, &["push", "-q", "origin", branch]);
    rev(&clone, "HEAD")
}

fn commit(dir: &Path, file: &str) {
    std::fs::write(dir.join(file), file).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
}

async fn thread_from(client: &Client, project: &str, base: &str) -> v1::Thread {
    client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: project.into(),
            base_ref: base.into(),
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap()
}

async fn setup() -> (tempfile::TempDir, Fixture, Daemon, Client, String) {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let project = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    (home, fixture, daemon, client, project.id)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_checked_out_base_is_fetched_and_the_checkout_left_alone() {
    let (_home, f, daemon, client, project) = setup().await;
    assert!(
        client.settings().await.unwrap().update_base_from_remote,
        "on by default"
    );
    let stale = rev(&f.repo, "main");
    let pushed = push_from_teammate(&f, "main", "fresh.txt");

    // No base given: the repo's current branch, `main`.
    let thread = thread_from(&client, &project, "").await;
    assert_eq!(thread.base_ref, "main");
    assert_eq!(thread.base_commit, pushed);
    assert!(
        PathBuf::from(&thread.worktree_path)
            .join("fresh.txt")
            .exists()
    );
    assert_eq!(rev(&f.repo, "main"), stale, "main is checked out: it stays");
    assert!(!f.repo.join("fresh.txt").exists(), "no files change there");

    // A remote-tracking base is fetched too.
    let pushed = push_from_teammate(&f, "main", "fresher.txt");
    let thread = thread_from(&client, &project, "origin/main").await;
    assert_eq!(thread.base_commit, pushed);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_base_checked_out_nowhere_is_fast_forwarded_and_local_commits_win() {
    let (_home, f, daemon, client, project) = setup().await;
    git(&f.repo, &["branch", "release"]);
    git(&f.repo, &["push", "-q", "-u", "origin", "release"]);

    // Behind: the thread and the local branch both move to the remote's commit.
    let pushed = push_from_teammate(&f, "release", "r1.txt");
    let thread = thread_from(&client, &project, "release").await;
    assert_eq!(thread.base_commit, pushed);
    assert_eq!(rev(&f.repo, "release"), pushed, "fast-forwarded");

    // Ahead: the local commit is used and nothing moves.
    git(&f.repo, &["checkout", "-q", "release"]);
    commit(&f.repo, "mine.txt");
    git(&f.repo, &["checkout", "-q", "main"]);
    let mine = rev(&f.repo, "release");
    let thread = thread_from(&client, &project, "release").await;
    assert_eq!(thread.base_commit, mine);

    // Diverged: still the local commit; the local branch keeps the user's work.
    push_from_teammate(&f, "release", "r2.txt");
    let thread = thread_from(&client, &project, "release").await;
    assert_eq!(thread.base_commit, mine);
    assert_eq!(rev(&f.repo, "release"), mine);

    // A tag, or a branch without an upstream, starts where it is.
    let stale = rev(&f.repo, "main");
    git(&f.repo, &["tag", "v1"]);
    git(&f.repo, &["branch", "local-only"]);
    push_from_teammate(&f, "main", "later.txt");
    assert_eq!(
        thread_from(&client, &project, "v1").await.base_commit,
        stale
    );
    assert_eq!(
        thread_from(&client, &project, "local-only")
            .await
            .base_commit,
        stale
    );
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_remote_falls_back_to_the_local_commit() {
    let (_home, f, daemon, client, project) = setup().await;
    let local = rev(&f.repo, "main");
    let gone = f.root.join("gone.git");
    git(
        &f.repo,
        &["remote", "set-url", "origin", gone.to_str().unwrap()],
    );
    let thread = thread_from(&client, &project, "main").await;
    assert_eq!(thread.base_commit, local);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_setting_off_threads_start_from_the_local_commit() {
    let (_home, f, daemon, client, project) = setup().await;
    client
        .update_settings(v1::SettingsPatch {
            update_base_from_remote: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!client.settings().await.unwrap().update_base_from_remote);
    let stale = rev(&f.repo, "main");
    push_from_teammate(&f, "main", "fresh.txt");
    let thread = thread_from(&client, &project, "main").await;
    assert_eq!(thread.base_commit, stale);
    assert_eq!(
        rev(&f.repo, "origin/main"),
        stale,
        "nothing was fetched either"
    );
    daemon.stop().await;
}
