//! End-to-end: the user's own task message, and task messages drafted in the agent
//! instead of sent (UR-30, ADR 0030). Fake agents stand in for the real CLIs: they
//! say they're ready the way the real ones do, then print what they read from the
//! terminal.
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
use std::time::Duration;

use common::*;
use urushi_client::{Client, v1};

/// How a fake agent says it reads input.
#[derive(Clone, Copy)]
enum Ready {
    /// Urushi's `ready` signal, as Claude's `SessionStart` hook writes it, a second
    /// after it starts.
    Signal,
    /// First a startup dialog (`TRUST?`) that waits for a key, like Claude's folder
    /// trust, then the signal.
    DialogThenSignal,
    /// The signal (`READY-SENT`), then two more seconds of drawing.
    SignalThenBusy,
    /// Nothing but bracketed paste, as Codex does.
    PasteMode,
}

/// Writes an executable `name` into `dir`. It prints its argument count, its
/// arguments, and then, unless it got a prompt, everything it reads from the terminal
/// within 10 s, in `od -c` notation without spaces, as `GOT[…]`.
fn fake_agent(dir: &Path, name: &str, ready: Ready) {
    let ready_signal = "printf '\\033]777;urushi;ready\\007'\n";
    let signal = match ready {
        Ready::Signal => format!("sleep 1\n{ready_signal}"),
        Ready::DialogThenSignal => format!(
            "echo 'TRUST?'\nstty raw -echo min 1 time 0\ndd bs=1 count=1 2>/dev/null >/dev/null\n\
             stty sane\necho TRUSTED\n{ready_signal}"
        ),
        Ready::SignalThenBusy => format!(
            "{ready_signal}echo READY-SENT\n\
             i=0; while [ $i -lt 10 ]; do printf .; sleep 0.2; i=$((i+1)); done\n"
        ),
        Ready::PasteMode => String::new(),
    };
    let script = format!(
        "#!/bin/sh\n\
         while [ \"$1\" = --settings ] || [ \"$1\" = -c ]; do shift 2; done\n\
         printf 'ARGS[%s]\\n' \"$#\"\n\
         prompt=no\n\
         for a in \"$@\"; do printf 'ARG[%s]\\n' \"$a\"; case \"$a\" in -*|plan) ;; *) prompt=yes;; esac; done\n\
         if [ $prompt = no ]; then\n\
         {signal}\
         printf '\\033[?2004h'\n\
         stty raw -echo min 0 time 100\n\
         got=$(dd bs=4096 count=1 2>/dev/null | od -An -c | tr -d ' \\n')\n\
         stty sane\n\
         printf '\\033[?2004l'\n\
         printf 'GOT[%s]\\n' \"$got\"\n\
         fi\n\
         echo AGENT-DONE\n"
    );
    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A daemon finding agents only in `dir`, a client, a repo and a board task.
async fn setup(home: &Path, agents: &Path) -> (Daemon, Client, String, v1::Task) {
    let fixture = repo(home);
    let daemon = Daemon::start_with_agents(&home.join("home"), vec![agents.to_path_buf()]).await;
    let (client, _events) = daemon.connect().await;
    let repo_id = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
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
    (daemon, client, repo_id, task)
}

async fn set(client: &Client, patch: v1::SettingsPatch) {
    client.update_settings(patch).await.unwrap();
}

/// Starts a thread for `task` and returns its first pane.
async fn start(client: &Client, repo: &str, task: &str) -> String {
    let thread = client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: repo.into(),
            title: "from a task".into(),
            task_id: task.into(),
            start_task_agent: true,
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let tab = w.tabs.iter().find(|t| t.thread_id == thread.id).unwrap();
    panes_of(tab)[0].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_custom_message_is_filled_in_and_sent() {
    let home = temp_home();
    let agents = home.path().join("agents bin");
    std::fs::create_dir_all(&agents).unwrap();
    fake_agent(&agents, "claude", Ready::Signal);
    let (daemon, client, repo, task) = setup(home.path(), &agents).await;
    set(
        &client,
        v1::SettingsPatch {
            task_agent: Some(v1::Agent::Claude.into()),
            task_message_custom: Some(true),
            task_message: Some("Pick up {key} \"{title}\"{url}; read `{show}`.".into()),
            ..Default::default()
        },
    )
    .await;
    let pane = start(&client, &repo, &task.id).await;
    let screen = wait_for_text(&client, &pane, "AGENT-DONE").await;
    let text = screen.concat();
    assert!(
        text.contains(&format!(
            "ARG[Pick up {} \"Ship search\"; read `urushi board show {}`.]",
            task.id, task.id
        )),
        "{screen:#?}"
    );
    assert!(text.contains("ARGS[1]"), "{screen:#?}");

    // Switched off, the default message comes back; the text is kept.
    set(
        &client,
        v1::SettingsPatch {
            task_message_custom: Some(false),
            ..Default::default()
        },
    )
    .await;
    let settings = client.settings().await.unwrap();
    let agents = settings.agents.unwrap();
    assert!(!agents.task_message_custom);
    assert!(agents.task_message.starts_with("Pick up {key}"));
    let pane = start(&client, &repo, &task.id).await;
    let text = wait_for_text(&client, &pane, "AGENT-DONE").await.concat();
    assert!(text.contains("ARG[Work on this task"), "{text}");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_drafted_message_is_pasted_once_claude_says_it_is_ready_and_not_sent() {
    let home = temp_home();
    let agents = home.path().join("agents bin");
    std::fs::create_dir_all(&agents).unwrap();
    fake_agent(&agents, "claude", Ready::Signal);
    let (daemon, client, repo, task) = setup(home.path(), &agents).await;
    set(
        &client,
        v1::SettingsPatch {
            task_agent: Some(v1::Agent::Claude.into()),
            task_mode: Some(v1::AgentMode::Plan.into()),
            task_draft: Some(true),
            task_message_custom: Some(true),
            task_message: Some("Draft {title}".into()),
            ..Default::default()
        },
    )
    .await;
    let pane = start(&client, &repo, &task.id).await;
    let screen = wait_for_text(&client, &pane, "AGENT-DONE").await;
    let text = screen.concat();
    // Plan mode still applies, but no prompt is on the command line.
    assert!(text.contains("ARGS[2]"), "{screen:#?}");
    assert!(text.contains("ARG[plan]"), "{screen:#?}");
    // Bracketed, and nothing (no Enter) after it.
    assert!(
        text.contains("GOT[033[200~DraftShipsearch033[201~]"),
        "{screen:#?}"
    );
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_ready_signal_the_agent_must_hold_the_terminal() {
    let home = temp_home();
    // No space in the path: `ps` shows a script's path unquoted, and Urushi finds the
    // agent by its name there.
    let agents = home.path().join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    fake_agent(&agents, "codex", Ready::PasteMode);
    let (daemon, client, repo, task) = setup(home.path(), &agents).await;
    set(
        &client,
        v1::SettingsPatch {
            task_agent: Some(v1::Agent::Codex.into()),
            task_draft: Some(true),
            ..Default::default()
        },
    )
    .await;
    let pane = start(&client, &repo, &task.id).await;
    let screen = wait_for_text(&client, &pane, "AGENT-DONE").await;
    let text = screen.concat();
    assert!(text.contains("ARGS[0]"), "{screen:#?}");
    // The default message (a copy: this daemon has no CLI beside it), bracketed and unsent.
    assert!(
        text.contains("GOT[033[200~Workonthistaskfrommy"),
        "{screen:#?}"
    );
    assert!(text.contains("033[201~]"), "{screen:#?}");
    daemon.stop().await;
}

/// Claude with drafting on and the message "Draft {title}"; returns its pane.
async fn drafting_claude(home: &Path, ready: Ready) -> (Daemon, Client, String) {
    let agents = home.join("agents bin");
    std::fs::create_dir_all(&agents).unwrap();
    fake_agent(&agents, "claude", ready);
    let (daemon, client, repo, task) = setup(home, &agents).await;
    set(
        &client,
        v1::SettingsPatch {
            task_agent: Some(v1::Agent::Claude.into()),
            task_draft: Some(true),
            task_message_custom: Some(true),
            task_message: Some("Draft {title}".into()),
            ..Default::default()
        },
    )
    .await;
    let pane = start(&client, &repo, &task.id).await;
    (daemon, client, pane)
}

async fn press(client: &Client, pane: &str, text: &str) {
    client
        .send_input(pane, v1::input::Kind::Text(text.into()))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn answering_a_startup_dialog_keeps_the_draft() {
    let home = temp_home();
    let (daemon, client, pane) = drafting_claude(home.path(), Ready::DialogThenSignal).await;
    wait_for_text(&client, &pane, "TRUST?").await;
    press(&client, &pane, "y").await;
    let screen = wait_for_text(&client, &pane, "AGENT-DONE").await;
    assert!(
        screen
            .concat()
            .contains("GOT[033[200~DraftShipsearch033[201~]"),
        "{screen:#?}"
    );
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn typing_once_the_agent_is_ready_drops_the_draft() {
    let home = temp_home();
    let (daemon, client, pane) = drafting_claude(home.path(), Ready::SignalThenBusy).await;
    wait_for_text(&client, &pane, "READY-SENT").await;
    // The user starts typing while the agent is still drawing.
    press(&client, &pane, "q").await;
    let screen = wait_for_text(&client, &pane, "AGENT-DONE").await;
    assert!(screen.concat().contains("GOT[q]"), "{screen:#?}");
    // Nothing is pasted later either, into the agent or the shell after it.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let screen = client.read_screen_text(&pane).await.unwrap();
    assert!(!screen.concat().contains("Draft"), "{screen:#?}");
    daemon.stop().await;
}
