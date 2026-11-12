//! End-to-end: named thread branches, threads without a first tab, and tabs that start
//! an agent CLI. A fake agent script stands in for the real CLIs.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::*;
use urushi_client::v1;

/// Shell lines that drop Urushi's hook options (ADR 0017), which come first, setting
/// `$hooks` to whether there were any.
const STRIP_HOOKS: &str =
    "hooks=no; while [ \"$1\" = --settings ] || [ \"$1\" = -c ]; do hooks=yes; shift 2; done";

/// Writes an executable `name` into `dir` that says whether Urushi's hooks were passed,
/// then prints its other arguments.
fn fake_agent(dir: &Path, name: &str) {
    let path = dir.join(name);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n{STRIP_HOOKS}\necho \"FAKE {name} HOOKS:$hooks\"\necho \"FAKE {name} ARGS:$*:END\"\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_thread_can_have_a_named_branch_and_no_first_tab() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let project = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();

    let thread = client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: project.id.clone(),
            title: "Primary".into(),
            branch: "Primary".into(),
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(thread.primary_branch, "Primary");
    assert!(branches(&fixture.repo).contains(&"Primary".to_owned()));
    let workspace = client.list_workspace(false).await.unwrap();
    assert!(workspace.tabs.iter().all(|t| t.thread_id != thread.id));

    // A tab can still be opened afterwards, and closing it leaves a usable thread.
    let tab = client
        .create_tab(&thread.id, "", v1::Agent::Unspecified)
        .await
        .unwrap();
    assert_eq!(tab.title, "Terminal");
    client.close_tab(&tab.id).await.unwrap();
    client
        .create_tab(&thread.id, "", v1::Agent::Unspecified)
        .await
        .unwrap();

    // The same branch can't be created twice, and nothing is left behind.
    let before = branches(&fixture.repo);
    let err = client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: project.id.clone(),
            title: "again".into(),
            branch: "Primary".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::Conflict), "{err}");
    assert_eq!(branches(&fixture.repo), before);

    // Invalid names are rejected before git runs.
    let err = client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: project.id.clone(),
            branch: "has space".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::InvalidArgument), "{err}");

    // A base that doesn't exist says so in plain words.
    let err = client
        .create_thread(&project.id, "Primary", "Primary-base")
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::InvalidArgument));
    assert!(
        err.to_string()
            .contains("no branch, tag or commit has that name"),
        "{err}"
    );
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_tabs_type_the_agent_command_into_a_shell() {
    let home = temp_home();
    let fixture = repo(home.path());
    let agents = home.path().join("agents bin");
    std::fs::create_dir_all(&agents).unwrap();
    fake_agent(&agents, "claude");
    let daemon = Daemon::start_with_agents(&home.path().join("home"), vec![agents.clone()]).await;
    let (client, _events) = daemon.connect().await;

    // Only installed agents are listed.
    let installed = client.list_agents().await.unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].agent(), v1::Agent::Claude);

    let project = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: project.id,
            title: "agents".into(),
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap();

    // Normal mode by default: no skip-permissions flag.
    let tab = client
        .create_tab(&thread.id, "", v1::Agent::Claude)
        .await
        .unwrap();
    assert_eq!(tab.title, "Claude");
    let pane = panes_of(&tab)[0].clone();
    wait_for_text(&client, &pane, "FAKE claude ARGS::END").await;
    wait_for_text(&client, &pane, "FAKE claude HOOKS:yes").await;
    // The shell is still there after the agent exits.
    type_line(&client, &pane, "echo still-here").await;
    wait_for_text(&client, &pane, "still-here").await;

    // Opted in through settings: the flag is passed.
    client
        .update_settings(v1::SettingsPatch {
            claude_skip_permissions: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    let tab = client
        .create_tab(&thread.id, "", v1::Agent::Claude)
        .await
        .unwrap();
    let pane = panes_of(&tab)[0].clone();
    wait_for_text(
        &client,
        &pane,
        "FAKE claude ARGS:--dangerously-skip-permissions:END",
    )
    .await;

    // An agent that isn't installed fails cleanly and opens nothing.
    let tabs_before = client.list_workspace(false).await.unwrap().tabs.len();
    let err = client
        .create_tab(&thread.id, "", v1::Agent::Codex)
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::FailedPrecondition));
    assert!(err.to_string().contains("Codex is not installed"), "{err}");
    assert_eq!(
        client.list_workspace(false).await.unwrap().tabs.len(),
        tabs_before
    );
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn threads_are_organized_into_projects_with_pins_and_statuses() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();

    let billing = client.create_project("  Billing   launch ").await.unwrap();
    assert_eq!(billing.name, "Billing launch");
    let thread = client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: repo.id.clone(),
            title: "api".into(),
            skip_initial_tab: true,
            folder_id: billing.id.clone(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(thread.folder_id, billing.id);

    // Rename, pin and set a status in one update.
    client
        .update_thread(v1::UpdateThreadRequest {
            thread_id: thread.id.clone(),
            title: Some("API work".into()),
            pinned: Some(true),
            status: Some(" In   review ".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let t = w.threads.iter().find(|t| t.id == thread.id).unwrap();
    assert_eq!(
        (
            t.title.as_str(),
            t.pinned,
            t.status.as_str(),
            t.folder_id.as_str()
        ),
        ("API work", true, "In review", billing.id.as_str())
    );
    assert_eq!(w.folders.len(), 1);

    // Unknown projects and bad statuses are refused, changing nothing.
    for bad in [
        v1::UpdateThreadRequest {
            thread_id: thread.id.clone(),
            folder_id: Some(urushi_core::ProjectId::from_u128(9).to_string()),
            pinned: Some(false),
            ..Default::default()
        },
        v1::UpdateThreadRequest {
            thread_id: thread.id.clone(),
            status: Some("x".repeat(100)),
            pinned: Some(false),
            ..Default::default()
        },
    ] {
        assert!(client.update_thread(bad).await.is_err());
    }
    let w = client.list_workspace(false).await.unwrap();
    assert!(w.threads.iter().find(|t| t.id == thread.id).unwrap().pinned);

    // Clearing the status and deleting the project ungroups the thread, and touches
    // nothing on disk.
    client
        .update_thread(v1::UpdateThreadRequest {
            thread_id: thread.id.clone(),
            status: Some(String::new()),
            ..Default::default()
        })
        .await
        .unwrap();
    client.delete_project(&billing.id).await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let t = w.threads.iter().find(|t| t.id == thread.id).unwrap();
    assert!(t.status.is_empty() && t.folder_id.is_empty());
    assert!(w.folders.is_empty());
    assert!(std::path::Path::new(&t.worktree_path).is_dir());

    // Organization survives a daemon restart.
    client
        .update_thread(v1::UpdateThreadRequest {
            thread_id: thread.id.clone(),
            status: Some("Blocked".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    drop(client);
    let home_dir = daemon.home.clone();
    daemon.stop().await;
    let daemon = Daemon::start(&home_dir).await;
    let (client, _events) = daemon.connect().await;
    let w = client.list_workspace(false).await.unwrap();
    let t = w.threads.iter().find(|t| t.id == thread.id).unwrap();
    assert_eq!((t.status.as_str(), t.pinned), ("Blocked", true));
    daemon.stop().await;
}
