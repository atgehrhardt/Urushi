//! End-to-end: after the daemon restarts, panes come back as they were: a shell in the
//! same directory under its old output, an agent resuming its conversation, and a
//! program that was running started again with the same arguments.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use common::*;
use urushi_client::v1;
use urushi_core::{Agent, AgentSessionId};
use urushi_daemon::DaemonOptions;
use urushi_daemon::restore::Conversation;
use urushi_daemon::tailscale::StaticTailscale;
use urushi_daemon::usage::process::Details;
use urushi_daemon::usage::{Process, SystemUsage, Usage, UsageSource};

/// The real machine, except that every Claude has session `restored-session-1` open.
struct Machine;

impl UsageSource for Machine {
    fn processes(&self) -> Vec<Process> {
        SystemUsage.processes()
    }
    fn details(&self, pids: &[u32]) -> HashMap<u32, Details> {
        SystemUsage.details(pids)
    }
    fn agent_conversation(&self, agent: Agent, _pid: u32) -> Conversation {
        match agent {
            Agent::Claude => {
                Conversation::Saved(AgentSessionId::new("restored-session-1").unwrap())
            }
            _ => Conversation::Unknown,
        }
    }
    fn usage(&self, _agent: Agent) -> Result<Usage, String> {
        Err("not here".into())
    }
}

/// A stand-in `claude` that prints its arguments and keeps running, like the real one.
fn fake_claude(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("claude");
    std::fs::write(
        &path,
        "#!/bin/sh\necho \"FAKE claude ARGS:$*:END\"\nsleep 60\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A stand-in long-running program: counts its runs in `<dir>/runs`, prints its
/// arguments, and keeps running.
fn fake_app(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("fake-app");
    std::fs::write(
        &path,
        "#!/bin/sh\necho run >> \"$(dirname \"$0\")/runs\"\n\
         echo \"APP RUN $(wc -l < \"$(dirname \"$0\")/runs\" | tr -d ' ') ARGS:[$1][$2]\"\n\
         sleep 60\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn options(home: &Path, bin: &Path) -> DaemonOptions {
    let mut options = DaemonOptions::new(home);
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.agents = Arc::new(urushi_daemon::agents::AgentLocator::in_dirs(vec![
        bin.to_path_buf(),
    ]));
    options.usage = Arc::new(Machine);
    options
}

#[tokio::test(flavor = "multi_thread")]
async fn restarted_panes_come_back_with_their_output_directory_agent_and_program() {
    let home = temp_home();
    let fixture = repo(home.path());
    let bin = home.path().join("bin");
    fake_claude(&bin);
    let daemon_home = home.path().join("home");
    let daemon = Daemon::start_options(options(&daemon_home, &bin)).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&repo.id, "t", "").await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let shell = panes_of(w.tabs.iter().find(|t| t.thread_id == thread.id).unwrap())[0].clone();
    type_line(
        &client,
        &shell,
        "mkdir -p sub && cd sub && printf 'marker-%s\\n' before",
    )
    .await;
    wait_for_text(&client, &shell, "marker-before").await;
    let agent_tab = client
        .create_tab(&thread.id, "", v1::Agent::Claude)
        .await
        .unwrap();
    let agent = panes_of(&agent_tab)[0].clone();
    wait_for_text(&client, &agent, "FAKE claude ARGS::END").await;
    // A program left running in the foreground of a third pane.
    let app = fake_app(&bin);
    let app_tab = client
        .create_tab(&thread.id, "", v1::Agent::Unspecified)
        .await
        .unwrap();
    let app_pane = panes_of(&app_tab)[0].clone();
    type_line(
        &client,
        &app_pane,
        &format!("'{}' 'two words' x", app.display()),
    )
    .await;
    wait_for_text(&client, &app_pane, "APP RUN 1 ARGS:[two words][x]").await;
    drop(client);
    daemon.stop().await;

    let daemon = Daemon::start_options(options(&daemon_home, &bin)).await;
    let (client, _events) = daemon.connect().await;
    // The shell's old output is shown above the new shell, which starts where it was.
    let screen = wait_for_text(&client, &shell, "restored after the daemon restarted").await;
    assert!(
        screen.iter().any(|l| l == "marker-before"),
        "old output replayed: {screen:?}"
    );
    type_line(&client, &shell, "printf 'now-in:%s\\n' \"$(pwd -P)\"").await;
    let sub = std::fs::canonicalize(&thread.worktree_path)
        .unwrap()
        .join("sub");
    wait_for_text(&client, &shell, &format!("now-in:{}", sub.display())).await;
    // The agent resumes the conversation it had open.
    wait_for_text(
        &client,
        &agent,
        "FAKE claude ARGS:--resume restored-session-1:END",
    )
    .await;
    // The program runs again, with exactly the same arguments.
    wait_for_text(&client, &app_pane, "APP RUN 2 ARGS:[two words][x]").await;
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_panes_leave_no_restore_point_behind() {
    let home = temp_home();
    let fixture = repo(home.path());
    let bin = home.path().join("bin");
    fake_claude(&bin);
    let daemon_home = home.path().join("home");
    let daemon = Daemon::start_options(options(&daemon_home, &bin)).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&repo.id, "t", "").await.unwrap();
    let tab = client
        .create_tab(&thread.id, "", v1::Agent::Unspecified)
        .await
        .unwrap();
    let pane = panes_of(&tab)[0].clone();
    type_line(&client, &pane, "echo secret-output").await;
    wait_for_text(&client, &pane, "secret-output").await;
    drop(client);
    daemon.stop().await;
    let restore = std::fs::canonicalize(&daemon_home).unwrap().join("restore");
    let file = restore.join(format!("{pane}.json"));
    assert!(file.exists(), "saved on shutdown");

    let daemon = Daemon::start_options(options(&daemon_home, &bin)).await;
    let (client, _events) = daemon.connect().await;
    client.close_tab(&tab.id).await.unwrap();
    drop(client);
    daemon.stop().await;
    assert!(!file.exists(), "a closed pane's output is deleted");
}
