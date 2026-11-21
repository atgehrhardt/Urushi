//! End-to-end: shelving a thread stops its terminals but keeps its worktree, tabs and
//! layout; unshelving brings each pane back from its restore point, as after a restart:
//! a shell in the same directory under its old output, and an agent resuming its
//! conversation (ADR 0060).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use std::path::Path;
use std::time::Duration;

use common::agents::{fake_claude, options};
use common::*;
use urushi_client::{Client, ClientError, v1};

fn is_code(err: &ClientError, wanted: v1::ErrorCode) -> bool {
    matches!(err, ClientError::Remote { code, .. } if *code == wanted)
}

async fn thread(client: &Client, id: &str) -> v1::Thread {
    let w = client.list_workspace(false).await.unwrap();
    w.threads.into_iter().find(|t| t.id == id).unwrap()
}

/// The pid the pane's shell wrote to `file`.
async fn shell_pid(client: &Client, pane: &str, file: &Path) -> i32 {
    type_line(client, pane, &format!("echo $$ > '{}'", file.display())).await;
    wait_until("the shell's pid", || async {
        std::fs::read_to_string(file).ok()?.trim().parse().ok()
    })
    .await
}

fn alive(pid: i32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shelved_thread_keeps_its_tabs_and_comes_back_as_it_was() {
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
    let t = client.create_thread(&repo.id, "t", "").await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let shell_tab = w.tabs.iter().find(|tab| tab.thread_id == t.id).unwrap();
    let shell = panes_of(shell_tab)[0].clone();
    type_line(
        &client,
        &shell,
        "mkdir -p sub && cd sub && printf 'mark-%s\\n' one",
    )
    .await;
    wait_for_text(&client, &shell, "mark-one").await;
    let pid = shell_pid(&client, &shell, &home.path().join("pid")).await;
    let agent_tab = client
        .create_tab(&t.id, "", v1::Agent::Claude)
        .await
        .unwrap();
    let agent = panes_of(&agent_tab)[0].clone();
    wait_for_text(&client, &agent, "FAKE claude ARGS::END").await;
    let tabs_before = thread(&client, &t.id).await.tab_ids;

    client.set_thread_shelved(&t.id, true).await.unwrap();
    let shelved = thread(&client, &t.id).await;
    assert!(shelved.shelved);
    assert_eq!(
        shelved.state,
        v1::ThreadState::Ready as i32,
        "older clients see it ready"
    );
    assert_eq!(shelved.tab_ids, tabs_before, "the tabs are kept");
    wait_until("the shell to end", || async { (!alive(pid)).then_some(()) }).await;
    // The layout is kept, even once the processes are gone.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let w = client.list_workspace(false).await.unwrap();
    let kept = w.tabs.iter().find(|tab| tab.id == shell_tab.id).unwrap();
    assert_eq!(panes_of(kept), vec![shell.clone()]);
    assert!(Path::new(&t.worktree_path).is_dir(), "the worktree is kept");
    // Nothing runs in it until it is unshelved.
    let err = client.read_screen_text(&shell).await.unwrap_err();
    assert!(is_code(&err, v1::ErrorCode::FailedPrecondition), "{err:?}");
    let err = client
        .create_tab(&t.id, "", v1::Agent::Unspecified)
        .await
        .unwrap_err();
    assert!(is_code(&err, v1::ErrorCode::FailedPrecondition), "{err:?}");
    let err = client
        .split_pane(&shell_tab.id, &shell, v1::Direction::Right)
        .await
        .unwrap_err();
    assert!(is_code(&err, v1::ErrorCode::FailedPrecondition), "{err:?}");
    let err = client.set_thread_shelved(&t.id, true).await.unwrap_err();
    assert!(is_code(&err, v1::ErrorCode::FailedPrecondition), "{err:?}");

    // It stays shelved across a daemon restart.
    drop(client);
    daemon.stop().await;
    let daemon = Daemon::start_options(options(&daemon_home, &bin)).await;
    let (client, _events) = daemon.connect().await;
    assert!(thread(&client, &t.id).await.shelved);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        client.read_screen_text(&shell).await.is_err(),
        "not restored"
    );

    client.set_thread_shelved(&t.id, false).await.unwrap();
    assert!(!thread(&client, &t.id).await.shelved);
    let screen = wait_for_text(&client, &shell, "── unshelved ──").await;
    assert!(
        screen.iter().any(|l| l == "mark-one"),
        "old output replayed: {screen:?}"
    );
    type_line(&client, &shell, "printf 'now-in:%s\\n' \"$(pwd -P)\"").await;
    let sub = std::fs::canonicalize(&t.worktree_path).unwrap().join("sub");
    wait_for_text(&client, &shell, &format!("now-in:{}", sub.display())).await;
    wait_for_text(
        &client,
        &agent,
        "FAKE claude ARGS:--resume restored-session-1:END",
    )
    .await;
    let err = client.set_thread_shelved(&t.id, false).await.unwrap_err();
    assert!(is_code(&err, v1::ErrorCode::FailedPrecondition), "{err:?}");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unshelving_at_once_keeps_the_new_terminals() {
    let home = temp_home();
    let fixture = repo(home.path());
    let bin = home.path().join("bin");
    fake_claude(&bin);
    let daemon = Daemon::start_options(options(&home.path().join("home"), &bin)).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let t = client.create_thread(&repo.id, "t", "").await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let pane = panes_of(w.tabs.iter().find(|tab| tab.thread_id == t.id).unwrap())[0].clone();
    type_line(&client, &pane, "echo first-life").await;
    wait_for_text(&client, &pane, "first-life").await;
    // The old shell may only end after the new one started.
    client.set_thread_shelved(&t.id, true).await.unwrap();
    client.set_thread_shelved(&t.id, false).await.unwrap();
    wait_for_text(&client, &pane, "── unshelved ──").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    type_line(&client, &pane, "echo second-life").await;
    wait_for_text(&client, &pane, "second-life").await;
    let w = client.list_workspace(false).await.unwrap();
    assert!(
        w.tabs.iter().any(|tab| panes_of(tab) == vec![pane.clone()]),
        "the pane is still in its tab"
    );
    daemon.stop().await;
}

/// The GUI attaches a thread's panes as soon as the workspace shows it unshelved, so
/// their terminals must already run by then (it showed "pane … not found").
#[tokio::test(flavor = "multi_thread")]
async fn panes_can_be_attached_as_soon_as_the_thread_shows_unshelved() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, mut events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let t = client.create_thread(&repo.id, "t", "").await.unwrap();
    let w = client.list_workspace(true).await.unwrap();
    let tab = w.tabs.iter().find(|tab| tab.thread_id == t.id).unwrap();
    let mut panes = panes_of(tab);
    for _ in 0..7 {
        let tab = client
            .create_tab(&t.id, "", v1::Agent::Unspecified)
            .await
            .unwrap();
        panes.extend(panes_of(&tab));
    }
    client.set_thread_shelved(&t.id, true).await.unwrap();
    // Past the events from before the shelve.
    next_event(&mut events, |e| match e {
        v1::event::Body::WorkspaceChanged(ws)
            if ws.threads.iter().any(|th| th.id == t.id && th.shelved) =>
        {
            Some(())
        }
        _ => None,
    })
    .await;
    // Workspace events are spaced out; the user unshelves well after the last one.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let attach_on_unshelved = async {
        next_event(&mut events, |e| match e {
            v1::event::Body::WorkspaceChanged(ws)
                if ws.threads.iter().any(|th| th.id == t.id && !th.shelved) =>
            {
                Some(())
            }
            _ => None,
        })
        .await;
        // The last one first: it is the last to start.
        for pane in panes.iter().rev() {
            client.attach_pane(pane, 80, 24).await.unwrap();
        }
    };
    let (unshelved, ()) =
        tokio::join!(client.set_thread_shelved(&t.id, false), attach_on_unshelved);
    unshelved.unwrap();
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shelved_thread_can_be_deleted() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let t = client.create_thread(&repo.id, "t", "").await.unwrap();
    client.set_thread_shelved(&t.id, true).await.unwrap();
    let plan = client.plan_thread_deletion(&t.id).await.unwrap();
    client.delete_thread(&t.id, &plan.token).await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    assert!(w.threads.iter().all(|th| th.id != t.id));
    assert!(
        !Path::new(&t.worktree_path).exists(),
        "the worktree is removed"
    );
    let err = client.set_thread_shelved(&t.id, true).await.unwrap_err();
    assert!(is_code(&err, v1::ErrorCode::NotFound), "{err:?}");
    daemon.stop().await;
}
