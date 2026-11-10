//! End-to-end: a project's parent branch and its parent-branch thread (ADR 0029,
//! protocol 1.18), against a real repo with a bare `origin`.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::path::PathBuf;

use common::*;
use urushi_client::{Client, ClientError, v1};

fn settings(repo: &str, branch: &str, on: Option<bool>) -> v1::FolderSettings {
    v1::FolderSettings {
        base_repo_id: repo.into(),
        base_branch: branch.into(),
        base_thread: on,
    }
}

async fn base_thread(client: &Client, project: &str) -> Option<v1::Thread> {
    let w = client.list_workspace(false).await.unwrap();
    w.threads
        .into_iter()
        .find(|t| t.project_base && t.folder_id == project)
}

async fn folder(client: &Client, project: &str) -> v1::Folder {
    let w = client.list_workspace(false).await.unwrap();
    w.folders.into_iter().find(|f| f.id == project).unwrap()
}

fn code(err: &ClientError) -> v1::ErrorCode {
    match err {
        ClientError::Remote { code, .. } => *code,
        other => panic!("not a remote error: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_parent_branch_thread_attaches_to_the_repo_and_never_deletes_anything() {
    let home = temp_home();
    let f = repo(home.path());
    git(&f.repo, &["branch", "develop"]);
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let repo_id = client
        .add_repo(f.repo.to_str().unwrap(), "")
        .await
        .unwrap()
        .id;
    let project = client.create_project("Billing").await.unwrap().id;
    let defaults = folder(&client, &project).await.settings.unwrap();
    assert_eq!(defaults.base_thread, Some(true), "on by default");

    // `main` is checked out in the repo itself: the thread runs there.
    let saved = client
        .update_project_settings(&project, settings(&repo_id, "main", None))
        .await
        .unwrap();
    assert_eq!(saved.settings.unwrap().base_branch, "main");
    let main = base_thread(&client, &project).await.expect("opened");
    assert!(main.attached_checkout);
    assert_eq!(main.primary_branch, "main");
    assert_eq!(PathBuf::from(&main.worktree_path), f.repo);
    assert_eq!(main.tab_ids.len(), 1, "with a terminal");

    // A thread of the project started without a base starts from the parent branch.
    let child = client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: repo_id.clone(),
            title: "Invoices".into(),
            folder_id: project.clone(),
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(child.base_ref, "main");

    // The parent-branch thread stays in its project.
    let err = client
        .update_thread(v1::UpdateThreadRequest {
            thread_id: main.id.clone(),
            folder_id: Some(String::new()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(code(&err), v1::ErrorCode::FailedPrecondition);

    // Changing the parent branch while the thread is open needs it closed first.
    let err = client
        .update_project_settings(&project, settings(&repo_id, "develop", None))
        .await
        .unwrap_err();
    assert_eq!(code(&err), v1::ErrorCode::FailedPrecondition);

    // Closing it: nothing on disk goes, no branch is deleted, and it is switched off.
    std::fs::write(f.repo.join("dirty.txt"), "mine").unwrap();
    let plan = client.plan_thread_deletion(&main.id).await.unwrap();
    assert!(plan.keeps_worktree);
    assert!(plan.local_branches.is_empty() && plan.remote_branches.is_empty());
    assert_eq!((plan.modified_files, plan.untracked_files), (0, 0));
    client.delete_thread(&main.id, &plan.token).await.unwrap();
    assert!(
        f.repo.join("dirty.txt").exists(),
        "the user's checkout is untouched"
    );
    assert!(branches(&f.repo).contains(&"main".into()));
    assert!(base_thread(&client, &project).await.is_none());
    let off = folder(&client, &project).await.settings.unwrap();
    assert_eq!(off.base_thread, Some(false));
    assert_eq!(off.base_branch, "main", "the parent branch stays");

    // `develop` isn't checked out anywhere: the thread gets a worktree of its own.
    client
        .update_project_settings(&project, settings(&repo_id, "develop", Some(true)))
        .await
        .unwrap();
    let develop = base_thread(&client, &project).await.expect("opened");
    assert!(!develop.attached_checkout);
    let path = PathBuf::from(&develop.worktree_path);
    assert!(path.is_dir() && path != f.repo);
    assert_eq!(git(&path, &["branch", "--show-current"]).trim(), "develop");

    // Survives a restart.
    drop(client);
    let home_dir = daemon.home.clone();
    daemon.stop().await;
    let daemon = Daemon::start(&home_dir).await;
    let (client, _events) = daemon.connect().await;
    let again = base_thread(&client, &project).await.unwrap();
    assert_eq!(again.id, develop.id);
    assert!(!again.attached_checkout);

    // Closing it removes the worktree Urushi made, but keeps the branch.
    let plan = client.plan_thread_deletion(&develop.id).await.unwrap();
    assert!(!plan.keeps_worktree && plan.local_branches.is_empty());
    client
        .delete_thread(&develop.id, &plan.token)
        .await
        .unwrap();
    assert!(!path.exists());
    assert!(branches(&f.repo).contains(&"develop".into()));

    // Deleting an ordinary thread never deletes a project's parent branch either.
    let plan = client.plan_thread_deletion(&child.id).await.unwrap();
    assert!(
        !plan
            .local_branches
            .iter()
            .any(|b| b == "develop" || b == "main")
    );
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_branches_are_listed_and_can_be_the_parent_branch() {
    let home = temp_home();
    let f = repo(home.path());
    git(&f.repo, &["push", "-q", "origin", "main:release"]);
    git(&f.repo, &["fetch", "-q", "origin"]);
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let repo_id = client
        .add_repo(f.repo.to_str().unwrap(), "")
        .await
        .unwrap()
        .id;
    let listed = client.list_branches(&repo_id).await.unwrap();
    assert!(listed.remote_branches.contains(&"origin/release".into()));
    assert!(!listed.remote_branches.iter().any(|b| b.ends_with("/HEAD")));

    // Picking `origin/release` stores `release`, made to track it.
    let project = client.create_project("Release").await.unwrap().id;
    let saved = client
        .update_project_settings(&project, settings(&repo_id, "origin/release", None))
        .await
        .unwrap();
    assert_eq!(saved.settings.unwrap().base_branch, "release");
    assert!(branches(&f.repo).contains(&"release".into()));
    let release = base_thread(&client, &project).await.unwrap();
    assert!(!release.attached_checkout);

    // Something that isn't a branch is refused, and nothing changes.
    let err = client
        .update_project_settings(&project, settings(&repo_id, "nope", None))
        .await
        .unwrap_err();
    assert_eq!(code(&err), v1::ErrorCode::InvalidArgument);
    assert_eq!(
        folder(&client, &project)
            .await
            .settings
            .unwrap()
            .base_branch,
        "release"
    );
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_projects_first_thread_makes_its_base_the_parent_branch() {
    let home = temp_home();
    let f = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let repo_id = client
        .add_repo(f.repo.to_str().unwrap(), "")
        .await
        .unwrap()
        .id;
    let project = client.create_project("Search").await.unwrap().id;
    client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: repo_id.clone(),
            title: "Ranking".into(),
            folder_id: project.clone(),
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap();
    let adopted = folder(&client, &project).await.settings.unwrap();
    assert_eq!(
        (adopted.base_repo_id.as_str(), adopted.base_branch.as_str()),
        (repo_id.as_str(), "main")
    );
    let main = base_thread(&client, &project).await.expect("on by default");
    assert!(main.attached_checkout);

    // A project whose thread is switched off adopts the branch but opens nothing.
    let quiet = client.create_project("Quiet").await.unwrap().id;
    client
        .update_project_settings(&quiet, settings("", "", Some(false)))
        .await
        .unwrap();
    client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: repo_id.clone(),
            title: "Other".into(),
            folder_id: quiet.clone(),
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        folder(&client, &quiet).await.settings.unwrap().base_branch,
        "main"
    );
    assert!(base_thread(&client, &quiet).await.is_none());

    // Deleting the project ungroups its parent-branch thread; nothing on disk changes.
    client.delete_project(&project).await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let orphan = w.threads.iter().find(|t| t.id == main.id).unwrap();
    assert!(orphan.folder_id.is_empty() && orphan.project_base);
    daemon.stop().await;
}
