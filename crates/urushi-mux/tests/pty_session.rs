//! Integration tests against real PTYs and real processes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

use std::time::Duration;

use urushi_mux::{AlacrittyEmulator, NativePty, SessionHandle, SpawnSpec, TermSize};
#[cfg(unix)]
use urushi_mux::{Input, Key, KeyEvent, Modifiers};

const TIMEOUT: Duration = Duration::from_secs(10);

fn spawn(program: &str, args: &[&str], size: TermSize) -> SessionHandle {
    let spec = SpawnSpec {
        program: Some(program.into()),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        size: Some(size),
        ..SpawnSpec::default()
    };
    SessionHandle::spawn(&NativePty, &spec, Box::new(AlacrittyEmulator::new(size))).expect("spawn")
}

/// Waits until some visible line contains `needle`, returning all lines.
async fn wait_for_text(session: &SessionHandle, needle: &str) -> Vec<String> {
    let mut status = session.status();
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let lines = session.text().await.expect("text");
        if lines.iter().any(|l| l.contains(needle)) {
            return lines;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out waiting for {needle:?}; screen: {lines:#?}"
        );
        let _ =
            tokio::time::timeout(remaining.min(Duration::from_millis(200)), status.changed()).await;
    }
}

#[cfg(unix)]
async fn wait_for_exit(session: &SessionHandle) -> i32 {
    let mut status = session.status();
    tokio::time::timeout(TIMEOUT, async {
        loop {
            if let Some(code) = status.borrow().exit_code {
                return code;
            }
            status.changed().await.expect("session alive");
        }
    })
    .await
    .expect("process should exit")
}

#[cfg(unix)]
mod unix {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn scrolling_back_produces_a_frame_showing_history() {
        let s = spawn(
            "/bin/sh",
            &[
                "-c",
                "i=1; while [ $i -le 40 ]; do echo line$i; i=$((i+1)); done; sleep 5",
            ],
            TermSize::new(20, 5),
        );
        wait_for_text(&s, "line40").await;
        let before = s.frame_since(0).await.unwrap();
        assert_eq!(before.display_offset, 0);

        s.scroll(urushi_mux::Scroll::Lines(10)).await.unwrap();
        let after = s.frame_since(before.version).await.unwrap();
        assert_eq!(after.display_offset, 10);
        assert!(
            after.version > before.version,
            "scrolling must bump the version"
        );
        let text: Vec<String> = after.lines.iter().map(|(_, l)| l.text()).collect();
        assert!(
            text.iter().any(|l| l.contains("line30")),
            "scrolled-back lines are sent: {text:#?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn output_reaches_the_screen() {
        let s = spawn(
            "/bin/sh",
            &["-c", "printf 'hello from pty'; sleep 5"],
            TermSize::new(40, 5),
        );
        wait_for_text(&s, "hello from pty").await;
        s.kill().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn input_is_echoed_and_keys_are_encoded() {
        let s = spawn("/bin/cat", &[], TermSize::new(40, 5));
        s.input(Input::Text("ping".into())).await.unwrap();
        s.input(Input::Key(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::NONE,
        }))
        .await
        .unwrap();
        wait_for_text(&s, "ping").await;
        // Ctrl+D ends cat.
        s.input(Input::Key(KeyEvent {
            key: Key::Character("d".into()),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        }))
        .await
        .unwrap();
        assert_eq!(wait_for_exit(&s).await, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn exit_code_is_reported() {
        let s = spawn("/bin/sh", &["-c", "exit 3"], TermSize::DEFAULT);
        assert_eq!(wait_for_exit(&s).await, 3);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resize_reaches_the_process() {
        let s = spawn("/bin/sh", &[], TermSize::new(80, 24));
        s.resize(TermSize::new(100, 30)).await.unwrap();
        s.input(Input::Text("stty size\r".into())).await.unwrap();
        wait_for_text(&s, "30 100").await;
        s.kill().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn frames_are_incremental_after_the_first() {
        let s = spawn("/bin/cat", &[], TermSize::new(20, 6));
        let first = s.frame_since(0).await.unwrap();
        assert!(first.full);
        assert_eq!(first.lines.len(), 6);

        s.input(Input::Text("abc".into())).await.unwrap();
        wait_for_text(&s, "abc").await;
        let next = s.frame_since(first.version).await.unwrap();
        assert!(!next.full);
        assert!(next.version > first.version);
        assert!(next.lines.iter().any(|(_, l)| l.text().contains("abc")));
        assert!(next.lines.len() < 6, "only changed lines are sent");

        let idle = s.frame_since(next.version).await.unwrap();
        assert!(
            idle.lines.is_empty(),
            "nothing changed since the last frame"
        );

        s.resize(TermSize::new(30, 6)).await.unwrap();
        assert!(
            s.frame_since(idle.version).await.unwrap().full,
            "resizes force a full frame"
        );
        s.kill().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn environment_and_cwd_are_applied() {
        let dir = std::env::temp_dir();
        let spec = SpawnSpec {
            program: Some("/bin/sh".into()),
            args: vec![
                "-c".into(),
                "printf \"[${HOME-unset}]$URUSHI_TEST:$(pwd -P)\"; sleep 5".into(),
            ],
            cwd: Some(dir.clone()),
            env_remove: vec!["HOME".into()],
            env: vec![("URUSHI_TEST".into(), "marker".into())],
            size: Some(TermSize::new(200, 3)),
            tty_env: None,
        };
        let s = SessionHandle::spawn(
            &NativePty,
            &spec,
            Box::new(AlacrittyEmulator::new(TermSize::new(200, 3))),
        )
        .unwrap();
        let canonical = std::fs::canonicalize(&dir).unwrap();
        wait_for_text(&s, &format!("[unset]marker:{}", canonical.display())).await;
        s.kill().await.unwrap();
    }
}

/// An agent hook, which has no controlling terminal, raises attention (and says it is
/// ready) by writing Urushi's signal to the terminal device named in the pane's
/// environment.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn hooks_signal_through_the_terminal_device() {
    use urushi_core::AttentionKind;
    use urushi_core::agent::hooks::{
        TTY_ENV, idle_command, ready_command, running_command, signal_command,
    };

    let size = TermSize::new(80, 4);
    let script = format!(
        "{} ; {} ; {} ; {} ; {} ; {} ; printf '\\a'; sleep 5",
        ready_command(),
        running_command(),
        idle_command(),
        running_command(),
        signal_command(AttentionKind::Done),
        signal_command(AttentionKind::Waiting)
    );
    let spec = SpawnSpec {
        program: Some("/bin/sh".into()),
        args: vec!["-c".into(), script],
        size: Some(size),
        tty_env: Some(TTY_ENV.into()),
        ..SpawnSpec::default()
    };
    let s = SessionHandle::spawn(&NativePty, &spec, Box::new(AlacrittyEmulator::new(size)))
        .expect("spawn");
    let mut status = s.status();
    let seen = tokio::time::timeout(TIMEOUT, async {
        loop {
            let now = *status.borrow_and_update();
            if now.alerts >= 3 {
                return now;
            }
            status.changed().await.expect("session alive");
        }
    })
    .await
    .expect("signals arrive");
    assert_eq!(seen.alerts, 3, "done, waiting, then a bell");
    assert_eq!(seen.bells, 1);
    assert_eq!(seen.alert, Some(AttentionKind::Waiting));
    assert_eq!(seen.ready, 1, "ready is counted, and is not an alert");
    assert_eq!(seen.activity, 3, "running, idle, running: not alerts");
    assert!(
        !seen.running,
        "the alerts after the last running stopped it"
    );
    s.kill().await.unwrap();
}

/// The `running` a session reports once `script` has printed its signals in one write.
#[cfg(unix)]
async fn running_after(signals: &str) -> bool {
    let size = TermSize::new(80, 4);
    let script = format!("printf '{signals}'; sleep 5");
    let spec = SpawnSpec {
        program: Some("/bin/sh".into()),
        args: vec!["-c".into(), script],
        size: Some(size),
        ..SpawnSpec::default()
    };
    let s = SessionHandle::spawn(&NativePty, &spec, Box::new(AlacrittyEmulator::new(size)))
        .expect("spawn");
    let mut status = s.status();
    let seen = tokio::time::timeout(TIMEOUT, async {
        loop {
            let now = *status.borrow_and_update();
            if now.activity >= 1 && now.alerts >= 1 {
                return now;
            }
            status.changed().await.expect("session alive");
        }
    })
    .await
    .expect("signals arrive");
    s.kill().await.unwrap();
    seen.running
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn running_follows_the_order_of_signals_that_arrive_together() {
    let running = "\\033]777;urushi;running\\007";
    let done = "\\033]777;urushi;done\\007";
    // A turn's last tool, then its end: stopped.
    assert!(!running_after(&format!("{running}{done}")).await);
    // A turn's end, then the next prompt: running.
    assert!(running_after(&format!("{done}{running}")).await);
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn output_reaches_the_screen_windows() {
    let s = spawn(
        "cmd.exe",
        &["/c", "echo hello from pty"],
        TermSize::new(40, 5),
    );
    wait_for_text(&s, "hello from pty").await;
}

/// The state `ps` reports for `pid` (`Z` for a zombie), or `None` once it is gone.
#[cfg(unix)]
fn process_state(pid: u32) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    let state = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!state.is_empty()).then_some(state)
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_killed_process_is_reaped_not_left_a_zombie() {
    // Ignores the hang-up, so ending it takes the escalation to SIGKILL.
    let session = spawn(
        "/bin/sh",
        &["-c", "trap '' HUP; echo ready; while :; do sleep 1; done"],
        TermSize::new(80, 24),
    );
    wait_for_text(&session, "ready").await;
    let pid = session.pid().expect("pid");
    session.kill().await.unwrap();
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        match process_state(pid) {
            None => break,
            Some(state) => assert!(
                tokio::time::Instant::now() < deadline,
                "process {pid} still there after kill (state {state})"
            ),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
