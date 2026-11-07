//! Integration tests against real PTYs and real processes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "test code")]

use std::time::Duration;

use urushi_mux::{
    AlacrittyEmulator, Input, Key, KeyEvent, Modifiers, NativePty, SessionHandle, SpawnSpec,
    TermSize,
};

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
                "printf \"$URUSHI_TEST:$(pwd -P)\"; sleep 5".into(),
            ],
            cwd: Some(dir.clone()),
            env: vec![("URUSHI_TEST".into(), "marker".into())],
            size: Some(TermSize::new(200, 3)),
        };
        let s = SessionHandle::spawn(
            &NativePty,
            &spec,
            Box::new(AlacrittyEmulator::new(TermSize::new(200, 3))),
        )
        .unwrap();
        let canonical = std::fs::canonicalize(&dir).unwrap();
        wait_for_text(&s, &format!("marker:{}", canonical.display())).await;
        s.kill().await.unwrap();
    }
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
