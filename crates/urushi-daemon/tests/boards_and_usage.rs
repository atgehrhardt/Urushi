//! End-to-end: task boards (kept and mirrored by the daemon) and agent usage reports.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::sync::Arc;

use common::*;
use urushi_client::{ClientError, v1};
use urushi_core::Agent;
use urushi_daemon::DaemonOptions;
use urushi_daemon::tailscale::StaticTailscale;
use urushi_daemon::usage::{Process, SystemUsage, Usage, UsageSource, Window};

fn titles(column: &v1::BoardColumn) -> Vec<&str> {
    column.tasks.iter().map(|t| t.title.as_str()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn boards_are_kept_mirrored_and_survive_a_restart() {
    let home = temp_home();
    let daemon_home = home.path().join("home");
    let daemon = Daemon::start(&daemon_home).await;
    let (client, mut events) = daemon.connect().await;
    client.list_workspace(true).await.unwrap();

    let board = client.create_board("Sprint").await.unwrap();
    let names: Vec<_> = board.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["To do", "In progress", "Done"]);
    let (todo, done) = (board.columns[0].id.clone(), board.columns[2].id.clone());
    let a = client.create_task(&board.id, &todo, "a", "").await.unwrap();
    let b = client
        .create_task(&board.id, &todo, "b", "notes")
        .await
        .unwrap();
    client.move_task(&a.id, &done, 0).await.unwrap();
    client.update_task(&b.id, Some("b2"), None).await.unwrap();

    // Subscribers see every change.
    let mirrored = next_event(&mut events, |e| match e {
        v1::event::Body::WorkspaceChanged(w)
            if w.boards
                .first()
                .is_some_and(|b| b.columns[0].tasks.first().is_some_and(|t| t.title == "b2")) =>
        {
            Some(w)
        }
        _ => None,
    })
    .await;
    let board_now = &mirrored.boards[0];
    assert_eq!(titles(&board_now.columns[0]), ["b2"]);
    assert_eq!(board_now.columns[0].tasks[0].notes, "notes");
    assert_eq!(titles(&board_now.columns[2]), ["a"]);

    // Bad input is refused and changes nothing.
    let err = client
        .create_task(&board.id, &todo, "  ", "")
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            ClientError::Remote {
                code: v1::ErrorCode::InvalidArgument,
                ..
            }
        ),
        "{err:?}"
    );
    let err = client.move_task(&a.id, &b.id, 0).await.unwrap_err();
    assert!(
        matches!(
            err,
            ClientError::Remote {
                code: v1::ErrorCode::NotFound,
                ..
            }
        ),
        "{err:?}"
    );
    let err = client.delete_task("not-an-id").await.unwrap_err();
    assert!(
        matches!(
            err,
            ClientError::Remote {
                code: v1::ErrorCode::InvalidArgument,
                ..
            }
        ),
        "{err:?}"
    );

    client.delete_task(&b.id).await.unwrap();
    client.rename_board(&board.id, "Sprint 2").await.unwrap();
    let other = client.create_board("Other").await.unwrap();
    client.delete_board(&other.id).await.unwrap();
    daemon.stop().await;

    // A new daemon on the same home has the same boards.
    let daemon = Daemon::start(&daemon_home).await;
    let (client, _events) = daemon.connect().await;
    let w = client.list_workspace(false).await.unwrap();
    assert_eq!(w.boards.len(), 1);
    assert_eq!(w.boards[0].name, "Sprint 2");
    assert!(w.boards[0].columns[0].tasks.is_empty());
    assert_eq!(titles(&w.boards[0].columns[2]), ["a"]);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn subtasks_are_created_checked_renamed_and_deleted() {
    let home = temp_home();
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let board = client.create_board("Work").await.unwrap();
    let todo = board.columns[0].id.clone();

    // A task created with its checklist, then edited one step at a time.
    let body = v1::request::Body::CreateTask(v1::CreateTaskRequest {
        board_id: board.id.clone(),
        column_id: todo.clone(),
        title: "Ship search".into(),
        notes: String::new(),
        subtasks: vec!["index".into(), "  query   page ".into()],
        ..Default::default()
    });
    let v1::response::Outcome::Task(task) = client.request(body).await.unwrap() else {
        panic!("CreateTask answers with the task");
    };
    let steps: Vec<_> = task.subtasks.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(steps, ["index", "query page"]);
    let extra = client.create_subtask(&task.id, "docs").await.unwrap();
    client
        .update_subtask(&task.subtasks[0].id, None, Some(true))
        .await
        .unwrap();
    client
        .update_subtask(&extra.id, Some("write docs"), None)
        .await
        .unwrap();
    client.delete_subtask(&task.subtasks[1].id).await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let now = &w.boards[0].columns[0].tasks[0];
    let steps: Vec<_> = now
        .subtasks
        .iter()
        .map(|s| (s.title.as_str(), s.done))
        .collect();
    assert_eq!(steps, [("index", true), ("write docs", false)]);
    let err = client.create_subtask(&task.id, " ").await.unwrap_err();
    assert!(
        matches!(
            err,
            ClientError::Remote {
                code: v1::ErrorCode::InvalidArgument,
                ..
            }
        ),
        "{err:?}"
    );
    daemon.stop().await;
}

/// A board with one task that has one subtask.
async fn task_with_a_subtask(client: &urushi_client::Client) -> (v1::Task, v1::Subtask) {
    let board = client.create_board("Work").await.unwrap();
    let task = client
        .create_task(&board.id, &board.columns[0].id, "Ship search", "")
        .await
        .unwrap();
    let subtask = client.create_subtask(&task.id, "docs").await.unwrap();
    (task, subtask)
}

#[tokio::test(flavor = "multi_thread")]
async fn threads_started_from_tasks_are_linked_until_deleted() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let (task, extra) = task_with_a_subtask(&client).await;

    // Threads started from the task and from a subtask are linked to them.
    let repo_id = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap()
        .id;
    let start = |subtask: &str| v1::CreateThreadRequest {
        project_id: repo_id.clone(),
        title: "from a task".into(),
        task_id: task.id.clone(),
        subtask_id: subtask.into(),
        skip_initial_tab: true,
        ..Default::default()
    };
    let for_task = client.create_thread_with(start("")).await.unwrap();
    let for_step = client.create_thread_with(start(&extra.id)).await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let now = &w.boards[0].columns[0].tasks[0];
    assert_eq!(now.thread_id, for_task.id);
    assert_eq!(now.subtasks[0].thread_id, for_step.id);

    // A missing task is refused before any worktree is made.
    let threads_before = w.threads.len();
    let mut bad = start("");
    bad.task_id = "00000000-0000-0000-0000-000000000001".into();
    let err = client.create_thread_with(bad).await.unwrap_err();
    assert!(
        matches!(
            err,
            ClientError::Remote {
                code: v1::ErrorCode::NotFound,
                ..
            }
        ),
        "{err:?}"
    );
    let err = client
        .create_thread_with(start(&task.id))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            ClientError::Remote {
                code: v1::ErrorCode::NotFound,
                ..
            }
        ),
        "a task id is not a subtask id: {err:?}"
    );
    let w = client.list_workspace(false).await.unwrap();
    assert_eq!(w.threads.len(), threads_before);

    // Deleting a thread unlinks it, so the subtask can be started again.
    let plan = client.plan_thread_deletion(&for_step.id).await.unwrap();
    client
        .delete_thread(&for_step.id, &plan.token)
        .await
        .unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let now = &w.boards[0].columns[0].tasks[0];
    assert_eq!(now.subtasks[0].thread_id, "");
    assert_eq!(now.thread_id, for_task.id, "other links stay");
    daemon.stop().await;
}

/// The real process table, and a fixed plan for Claude.
struct FakeUsage;

impl UsageSource for FakeUsage {
    fn processes(&self) -> Vec<Process> {
        SystemUsage.processes()
    }

    fn usage(&self, agent: Agent) -> Result<Usage, String> {
        match agent {
            Agent::Claude => Ok(Usage {
                plan: "Max 5x".into(),
                windows: vec![Window {
                    label: "5h".into(),
                    used_percent: 37.0,
                    resets_at_ms: Some(1_000),
                }],
            }),
            _ => Err("not here".into()),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn usage_follows_the_agent_running_in_the_pane() {
    let home = temp_home();
    let fixture = repo(home.path());
    // A stand-in `claude` that just waits.
    let bin = home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("claude");
    std::fs::write(&fake, "#!/bin/sh\nsleep 30\n").unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

    let mut options = DaemonOptions::new(home.path().join("home"));
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.usage = Arc::new(FakeUsage);
    let daemon = Daemon::start_options(options).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&repo.id, "t", "").await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let pane = panes_of(w.tabs.iter().find(|t| t.thread_id == thread.id).unwrap())[0].clone();

    let usage = client.agent_usage(&pane).await.unwrap();
    assert_eq!(usage.agent(), v1::Agent::Unspecified, "a plain shell");
    assert!(usage.windows.is_empty());

    type_line(&client, &pane, fake.to_str().unwrap()).await;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let usage = loop {
        let usage = client.agent_usage(&pane).await.unwrap();
        if usage.agent() == v1::Agent::Claude {
            break usage;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "claude never detected"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    };
    assert_eq!(usage.plan, "Max 5x");
    assert_eq!(usage.windows[0].label, "5h");
    assert!((usage.windows[0].used_percent - 37.0).abs() < f64::EPSILON);
    assert!(usage.unavailable.is_empty());

    let err = client.agent_usage(&thread.id).await.unwrap_err();
    assert!(
        matches!(
            err,
            ClientError::Remote {
                code: v1::ErrorCode::NotFound,
                ..
            }
        ),
        "{err:?}"
    );
    daemon.stop().await;
}
