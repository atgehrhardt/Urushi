//! End-to-end: a thread started from a task runs the user's task agent, in its task
//! mode, with the task as its first prompt (ADR 0014). A fake agent script stands in for
//! the real CLIs and prints each argument it gets.
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
use urushi_client::{Client, v1};

/// Writes an executable `name` into `dir` that prints each argument as `ARG[<arg>]`,
/// after Urushi's hook options.
fn fake_agent(dir: &Path, name: &str) {
    let path = dir.join(name);
    // Urushi's hook options come first (ADR 0017); they're dropped here.
    let script = "#!/bin/sh\nwhile [ \"$1\" = --settings ] || [ \"$1\" = -c ]; do shift 2; done\nfor a in \"$@\"; do printf 'ARG[%s]\\n' \"$a\"; done\necho AGENT-DONE\n";
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A board with a task that has notes and two subtasks, one checked off.
async fn task(client: &Client) -> (v1::Task, v1::Subtask) {
    let board = client.create_board("Work").await.unwrap();
    let task = client
        .create_task(
            &board.id,
            &board.columns[0].id,
            "Ship search",
            "Index titles.\nKeep it fast.",
        )
        .await
        .unwrap();
    let done = client.create_subtask(&task.id, "schema").await.unwrap();
    client
        .update_subtask(&done.id, None, Some(true))
        .await
        .unwrap();
    let docs = client.create_subtask(&task.id, "docs").await.unwrap();
    (task, docs)
}

fn start(repo: &str, task: &str, subtask: &str) -> v1::CreateThreadRequest {
    v1::CreateThreadRequest {
        project_id: repo.into(),
        title: "from a task".into(),
        task_id: task.into(),
        subtask_id: subtask.into(),
        start_task_agent: true,
        skip_initial_tab: true,
        ..Default::default()
    }
}

async fn choose(client: &Client, agent: v1::Agent, mode: v1::AgentMode) {
    client
        .update_settings(v1::SettingsPatch {
            task_agent: Some(agent.into()),
            task_mode: Some(mode.into()),
            ..Default::default()
        })
        .await
        .unwrap();
}

/// The only tab of `thread`.
async fn first_tab(client: &Client, thread: &str) -> v1::Tab {
    let w = client.list_workspace(false).await.unwrap();
    let tabs: Vec<_> = w
        .tabs
        .into_iter()
        .filter(|t| t.thread_id == thread)
        .collect();
    assert_eq!(tabs.len(), 1, "{tabs:#?}");
    tabs.into_iter().next().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_task_starts_the_task_agent_in_its_mode_with_the_task_as_prompt() {
    let home = temp_home();
    let fixture = repo(home.path());
    let agents = home.path().join("agents bin");
    std::fs::create_dir_all(&agents).unwrap();
    fake_agent(&agents, "claude");
    let daemon = Daemon::start_with_agents(&home.path().join("home"), vec![agents]).await;
    let (client, _events) = daemon.connect().await;
    let repo_id = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap()
        .id;
    let (task, docs) = task(&client).await;

    choose(&client, v1::Agent::Claude, v1::AgentMode::Plan).await;
    let thread = client
        .create_thread_with(start(&repo_id, &task.id, ""))
        .await
        .unwrap();
    let tab = first_tab(&client, &thread.id).await;
    assert_eq!(tab.title, "Claude");
    let pane = panes_of(&tab)[0].clone();
    let screen = wait_for_text(&client, &pane, "AGENT-DONE").await;
    let text = screen.join("\n");
    for expected in [
        "ARG[--permission-mode]",
        "ARG[plan]",
        "ARG[Work on this task from my Urushi board.",
        "Task: Ship search",
        "Index titles.",
        "- [x] schema",
        "- [ ] docs]",
    ] {
        assert!(text.contains(expected), "{expected:?} missing from {text}");
    }
    assert_eq!(
        screen.iter().filter(|l| l.starts_with("ARG[")).count(),
        3,
        "the whole prompt is one argument: {text}"
    );
    let w = client.list_workspace(false).await.unwrap();
    assert_eq!(w.boards[0].columns[0].tasks[0].thread_id, thread.id);

    // A subtask gets its step, with the task as context; normal mode has no flags.
    choose(&client, v1::Agent::Claude, v1::AgentMode::Normal).await;
    let thread = client
        .create_thread_with(start(&repo_id, &task.id, &docs.id))
        .await
        .unwrap();
    let pane = panes_of(&first_tab(&client, &thread.id).await)[0].clone();
    let screen = wait_for_text(&client, &pane, "AGENT-DONE").await;
    let text = screen.join("\n");
    assert!(text.contains("ARG[Work on this step of a task"), "{text}");
    assert!(text.contains("Step: docs"), "{text}");
    assert!(text.contains("- [ ] docs  <- this step]"), "{text}");
    assert!(!text.contains("--permission-mode"), "{text}");

    // A deeper subtask names every task above it, and gets the link (ADR 0015).
    let deep = client
        .create_subtask(&docs.id, "API reference")
        .await
        .unwrap();
    let thread = client
        .create_thread_with(start(&repo_id, &task.id, &deep.id))
        .await
        .unwrap();
    let pane = panes_of(&first_tab(&client, &thread.id).await)[0].clone();
    let screen = wait_for_text(&client, &pane, "AGENT-DONE").await;
    let text = screen.join("\n");
    assert!(text.contains("Step: API reference"), "{text}");
    assert!(
        text.contains("It is part of the task: Ship search › docs"),
        "{text}"
    );
    let w = urushi_client::WorkspaceMirror::new(client.list_workspace(false).await.unwrap());
    assert_eq!(w.find_task(&deep.id).unwrap().task().thread_id(), thread.id);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_task_agent_that_cant_start_leaves_nothing_behind() {
    let home = temp_home();
    let fixture = repo(home.path());
    let agents = home.path().join("agents bin");
    std::fs::create_dir_all(&agents).unwrap();
    fake_agent(&agents, "claude");
    let daemon = Daemon::start_with_agents(&home.path().join("home"), vec![agents]).await;
    let (client, _events) = daemon.connect().await;
    let repo_id = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap()
        .id;
    let (task, _) = task(&client).await;
    let branches_before = branches(&fixture.repo);

    // No task agent chosen yet.
    let err = client
        .create_thread_with(start(&repo_id, &task.id, ""))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::FailedPrecondition), "{err}");
    assert!(err.to_string().contains("no agent is chosen"), "{err}");

    // The chosen agent isn't installed.
    choose(&client, v1::Agent::Codex, v1::AgentMode::Normal).await;
    let err = client
        .create_thread_with(start(&repo_id, &task.id, ""))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::FailedPrecondition), "{err}");
    assert!(err.to_string().contains("Codex is not installed"), "{err}");

    // Asking for the task agent without a task.
    choose(&client, v1::Agent::Claude, v1::AgentMode::Plan).await;
    let err = client
        .create_thread_with(start(&repo_id, "", ""))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Some(v1::ErrorCode::InvalidArgument), "{err}");

    let w = client.list_workspace(false).await.unwrap();
    assert!(w.threads.is_empty());
    assert!(w.tabs.is_empty());
    assert_eq!(w.boards[0].columns[0].tasks[0].thread_id, "");
    assert_eq!(branches(&fixture.repo), branches_before);
    daemon.stop().await;
}
