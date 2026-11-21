//! End-to-end: Claude's `Stop` hook reports the shells and monitors it keeps open in the
//! background, clients see them on the pane's tab, and they stay through the agent's end
//! and interrupt keys until a new report, the session's end or the pane closing
//! (ADR 0059).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::time::Duration;

use common::*;
use urushi_client::v1;
use urushi_core::agent::hooks::{claude_background_command, idle_command, signal_command};
use urushi_core::{AttentionKind, BackgroundTasks};

/// `pane`'s background tasks in `tabs`.
fn background_of(tabs: &[v1::Tab], pane: &str) -> BackgroundTasks {
    tabs.iter()
        .flat_map(|t| &t.background)
        .find(|b| b.pane_id == pane)
        .map(|b| BackgroundTasks {
            shells: b.shells,
            monitors: b.monitors,
        })
        .unwrap_or_default()
}

/// `pane`'s background tasks in the next workspace where they differ from `previous`.
async fn background_change(
    events: &mut urushi_client::Events,
    pane: &str,
    previous: BackgroundTasks,
) -> BackgroundTasks {
    next_event(events, |event| {
        let v1::event::Body::WorkspaceChanged(w) = event else {
            return None;
        };
        let now = background_of(&w.tabs, pane);
        (now != previous).then_some(now)
    })
    .await
}

/// Waits until the shell in `pane` is idle at its prompt: a marker command is typed
/// until its output shows with a new prompt after it. Ctrl+C reaching the shell while
/// it still runs a line, or a line typed while it handles an interrupt, leaves it in a
/// state where the next command can be lost, so the test waits on both sides of one.
async fn settle(client: &urushi_client::Client, pane: &str) {
    static MARKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    for _ in 0..50 {
        let n = MARKERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let marker = format!("settled-{n}");
        type_line(client, pane, &format!("echo {marker}")).await;
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(30)).await;
            let lines = client.read_screen_text(pane).await.unwrap();
            let shown = lines.iter().position(|l| l.trim_end() == marker);
            // The prompt after it means the shell reads input again.
            if shown.is_some_and(|i| lines[i + 1..].iter().any(|l| !l.trim().is_empty())) {
                return;
            }
        }
    }
    panic!("the shell in {pane} never came back to its prompt");
}

/// `pane`'s background tasks, as the daemon lists them now.
async fn background_now(client: &urushi_client::Client, pane: &str) -> BackgroundTasks {
    background_of(&client.list_workspace(false).await.unwrap().tabs, pane)
}

/// A `Stop` hook input as Claude Code 2.1 sends it, with these background tasks.
fn stop_input(background_tasks: &str) -> String {
    format!(
        r#"{{"session_id":"s","transcript_path":"/t.jsonl","cwd":"/w","permission_mode":"default","hook_event_name":"Stop","stop_hook_active":false,"last_assistant_message":"Started the server.","background_tasks":[{background_tasks}],"session_crons":[]}}"#
    )
}

const SHELL: &str = r#"{"id":"b1","type":"shell","status":"running","description":"Dev server","command":"make dev"}"#;
const MONITOR: &str = r#"{"id":"m1","type":"monitor","status":"running","description":"CI"}"#;
const SUBAGENT: &str = r#"{"id":"a1","type":"subagent","status":"running","description":"Explore the code","agent_type":"Explore"}"#;

/// Runs Claude's background `Stop` hook command with `input` on stdin, as Claude does,
/// and returns what it reported to the pane's terminal.
fn claude_background_report(input: &str) -> BackgroundTasks {
    use std::io::Write as _;
    let dir = temp_home();
    let tty = dir.path().join("tty");
    std::fs::write(&tty, "").unwrap();
    let mut child = std::process::Command::new("sh")
        .arg("-c")
        .arg(claude_background_command())
        .env("URUSHI_TTY", &tty)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success(), "a hook never fails");
    let written = std::fs::read_to_string(&tty).unwrap();
    let params = written
        .strip_prefix("\u{1b}]777;urushi;background;")
        .and_then(|rest| rest.strip_suffix('\u{7}'))
        .unwrap_or_else(|| panic!("not a background report: {written:?}"));
    let params: Vec<&[u8]> = params.split(';').map(str::as_bytes).collect();
    BackgroundTasks::from_params(&params)
}

#[test]
fn claudes_stop_hook_counts_its_background_shells_and_monitors() {
    let tasks = |shells, monitors| BackgroundTasks { shells, monitors };
    assert_eq!(
        claude_background_report(&stop_input(&format!("{SHELL},{MONITOR},{SUBAGENT}"))),
        tasks(1, 1)
    );
    assert_eq!(
        claude_background_report(&stop_input(&format!("{SHELL},{SHELL}"))),
        tasks(2, 0)
    );
    // Text in a description is JSON-escaped, so it can't pass for a task's type.
    let tricky = r#"{"id":"b2","type":"subagent","status":"running","description":"echo \"type\":\"shell\""}"#;
    assert_eq!(claude_background_report(&stop_input(tricky)), tasks(0, 0));
    assert_eq!(claude_background_report(&stop_input("")), tasks(0, 0));
    // Claude versions that don't list background tasks, or send nothing at all.
    assert_eq!(
        claude_background_report(r#"{"hook_event_name":"Stop"}"#),
        tasks(0, 0)
    );
    assert_eq!(claude_background_report(""), tasks(0, 0));
}

/// The command line that runs Claude's background `Stop` hook on `input`, typed into a
/// pane's shell.
fn report_line(input: &str) -> String {
    format!("echo '{input}' | {{ {} ; }}", claude_background_command())
}

#[tokio::test(flavor = "multi_thread")]
async fn background_tasks_outlive_the_turn_until_reported_closed_or_the_session_ends() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, mut events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let thread = client.create_thread(&repo.id, "t", "").await.unwrap();
    let w = client.list_workspace(true).await.unwrap();
    let tab = w.tabs.iter().find(|t| t.thread_id == thread.id).unwrap();
    assert!(tab.background.is_empty());
    let pane = panes_of(tab)[0].clone();
    let revision = client.list_workspace(false).await.unwrap().revision;
    let none = BackgroundTasks::default();
    let two_and_one = BackgroundTasks {
        shells: 2,
        monitors: 1,
    };

    // Claude's Stop hook reports two shells and a monitor. Not stored.
    let input = stop_input(&format!("{SHELL},{SHELL},{MONITOR}"));
    type_line(&client, &pane, &report_line(&input)).await;
    assert_eq!(
        background_change(&mut events, &pane, none).await,
        two_and_one
    );
    assert_eq!(
        client.list_workspace(false).await.unwrap().revision,
        revision
    );

    // The agent finished (its other Stop hook), and the user pressed Ctrl+C (an
    // interrupt, which stops running): still open. (Not Esc: the shell would read it
    // with the next key typed as Meta.)
    type_line(&client, &pane, &signal_command(AttentionKind::Done)).await;
    settle(&client, &pane).await;
    client
        .send_input(&pane, v1::input::Kind::Text("\u{3}".into()))
        .await
        .unwrap();
    settle(&client, &pane).await;
    assert_eq!(background_now(&client, &pane).await, two_and_one);

    // A shell ended: Claude's next turn reports the rest.
    let input = stop_input(&format!("{SHELL},{MONITOR}"));
    type_line(&client, &pane, &report_line(&input)).await;
    let one_and_one = BackgroundTasks {
        shells: 1,
        monitors: 1,
    };
    assert_eq!(
        background_change(&mut events, &pane, two_and_one).await,
        one_and_one
    );

    // Claude exits, and its background tasks with it.
    type_line(&client, &pane, &idle_command()).await;
    assert_eq!(
        background_change(&mut events, &pane, one_and_one).await,
        none
    );

    // Reported again, then the pane's shell exits: the pane goes, and its tasks with it.
    let input = stop_input(SHELL);
    type_line(&client, &pane, &report_line(&input)).await;
    let one = BackgroundTasks {
        shells: 1,
        monitors: 0,
    };
    assert_eq!(background_change(&mut events, &pane, none).await, one);
    type_line(&client, &pane, "exit").await;
    assert_eq!(background_change(&mut events, &pane, one).await, none);
    assert_eq!(background_now(&client, &pane).await, none);
}
