//! End-to-end: a task's agent is told to review the task with this daemon's `urushi`
//! CLI, and that command reaches it (ADR 0024, ADR 0033), even when the pane's shell
//! puts another `urushi` first on `PATH`, as login shells that rebuild `PATH` do (UR-32).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::*;
use urushi_client::{Client, v1};

fn executable(path: &Path, script: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A daemon in `home` whose CLI (`<home>/cli bin/urushi`, printing `REAL-CLI <args>`)
/// is beside it, and whose `claude` is `agent_script`. Returns the CLI's path.
async fn daemon_with_cli(home: &Path, agent_script: &str) -> (Daemon, PathBuf) {
    let agents = home.join("agents bin");
    executable(&agents.join("claude"), agent_script);
    let cli = home.join("cli bin").join("urushi");
    executable(&cli, "#!/bin/sh\necho \"REAL-CLI $*\"\n");
    let mut options = urushi_daemon::DaemonOptions::new(home.join("home"));
    options.tailscale =
        std::sync::Arc::new(urushi_daemon::tailscale::StaticTailscale::not_installed());
    options.agents =
        std::sync::Arc::new(urushi_daemon::agents::AgentLocator::in_dirs(vec![agents]));
    options.cli = Some(cli.clone());
    (Daemon::start_options(options).await, cli)
}

/// Starts a thread, with Claude as its agent, from a new task "Ship search" with notes
/// "Index titles.", and returns the task's id and the screen once the agent printed
/// `AGENT-DONE`.
async fn start_task(client: &Client, repo: &Path) -> (String, Vec<String>) {
    let repo_id = client
        .add_repo(repo.to_str().unwrap(), "")
        .await
        .unwrap()
        .id;
    let board = client.create_board("Work").await.unwrap();
    let task = client
        .create_task(
            &board.id,
            &board.columns[0].id,
            "Ship search",
            "Index titles.",
        )
        .await
        .unwrap();
    client
        .update_settings(v1::SettingsPatch {
            task_agent: Some(v1::Agent::Claude.into()),
            task_mode: Some(v1::AgentMode::Normal.into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let thread = client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: repo_id,
            title: "from a task".into(),
            task_id: task.id.clone(),
            start_task_agent: true,
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let tab = w
        .tabs
        .iter()
        .find(|t| t.thread_id == thread.id)
        .unwrap()
        .clone();
    let pane = panes_of(&tab)[0].clone();
    (task.id, wait_for_text(client, &pane, "AGENT-DONE").await)
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_cli_beside_it_a_task_starts_by_reference() {
    let home = temp_home();
    let fixture = repo(home.path());
    // An agent that also says where the CLI is and which daemon it would reach.
    let script = "#!/bin/sh\nwhile [ \"$1\" = --settings ] || [ \"$1\" = -c ]; do shift 2; done\nfor a in \"$@\"; do printf 'ARG[%s]\\n' \"$a\"; done\nprintf 'CLI[%s]\\n' \"$URUSHI_CLI\"\nprintf 'HOME[%s]\\n' \"$URUSHI_HOME\"\necho AGENT-DONE\n";
    let (daemon, cli) = daemon_with_cli(home.path(), script).await;
    let (client, _events) = daemon.connect().await;
    let (task, screen) = start_task(&client, &fixture.repo).await;
    let text = screen.join("\n");
    assert!(
        text.contains("ARG[Work on the task \"Ship search\" from my Urushi board."),
        "{text}"
    );
    // The terminal wraps long lines; read them as one. The CLI's directory has a space,
    // so its path is quoted.
    let unwrapped = screen.concat();
    assert!(
        unwrapped.contains(&format!("run `'{}' board show {task}`", cli.display())),
        "{text}"
    );
    assert!(
        !text.contains("Index titles."),
        "a reference, not a copy: {text}"
    );
    assert!(
        unwrapped.contains(&format!("CLI[{}]", cli.display())),
        "{text}"
    );
    let daemon_home = std::fs::canonicalize(home.path().join("home")).unwrap();
    assert!(
        unwrapped.contains(&format!("HOME[{}]", daemon_home.display())),
        "{text}"
    );
    daemon.stop().await;
}

/// UR-32: a login shell rebuilds `PATH` (macOS `path_helper`, the user's rc files), so
/// the CLI's directory the daemon put first can end up behind another `urushi` (on a
/// case-insensitive disk, even the app's own GUI). The command in the prompt still runs
/// this daemon's CLI.
#[tokio::test(flavor = "multi_thread")]
async fn the_prompts_command_runs_this_cli_whatever_comes_first_on_path() {
    let home = temp_home();
    let fixture = repo(home.path());
    // An agent that runs the command its prompt says to review the task with.
    let script = "#!/bin/sh\nwhile [ \"$1\" = --settings ] || [ \"$1\" = -c ]; do shift 2; done\ncmd=$(printf '%s\\n' \"$1\" | sed -n 's/.*run `\\([^`]*\\)`.*/\\1/p')\nprintf 'RAN[%s]\\n' \"$cmd\"\nsh -c \"$cmd\"\necho AGENT-DONE\n";
    let (daemon, _cli) = daemon_with_cli(home.path(), script).await;
    let (client, _events) = daemon.connect().await;
    // The pane's shell, like a login shell, puts its own `urushi` first on PATH.
    let decoy = home.path().join("decoy bin");
    executable(&decoy.join("urushi"), "#!/bin/sh\necho DECOY\n");
    let shell = home.path().join("login-sh");
    executable(
        &shell,
        &format!(
            "#!/bin/sh\nPATH='{}':\"$PATH\"\nexport PATH\nexec /bin/sh \"$@\"\n",
            decoy.display()
        ),
    );
    client
        .update_settings(v1::SettingsPatch {
            default_shell: Some(shell.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    let (task, screen) = start_task(&client, &fixture.repo).await;
    let text = screen.join("\n");
    assert!(
        text.contains(&format!("REAL-CLI board show {task}")),
        "{text}"
    );
    assert!(!text.contains("DECOY"), "{text}");
    daemon.stop().await;
}
