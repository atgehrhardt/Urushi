//! Shared helpers: an in-process daemon in a temp home, and git fixtures.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers"
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use urushi_client::{Client, Endpoint, Events, v1};
use urushi_daemon::tailscale::{StaticTailscale, TailscaleProvider};
use urushi_daemon::{DaemonError, DaemonOptions};
use urushi_proto::endpoint::local_endpoint;

pub const TIMEOUT: Duration = Duration::from_secs(15);

/// A running daemon. Dropping it does not stop it; call [`Daemon::stop`].
pub struct Daemon {
    pub home: PathBuf,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<(), DaemonError>>,
}

impl Daemon {
    pub async fn start(home: &Path) -> Self {
        Self::start_with(home, Arc::new(StaticTailscale::not_installed())).await
    }

    pub async fn start_with(home: &Path, tailscale: Arc<dyn TailscaleProvider>) -> Self {
        let mut options = DaemonOptions::new(home);
        options.tailscale = tailscale;
        Self::start_options(options).await
    }

    /// A daemon that finds agent CLIs only in `agent_dirs`.
    pub async fn start_with_agents(home: &Path, agent_dirs: Vec<PathBuf>) -> Self {
        let mut options = DaemonOptions::new(home);
        options.tailscale = Arc::new(StaticTailscale::not_installed());
        options.agents = Arc::new(urushi_daemon::agents::AgentLocator::in_dirs(agent_dirs));
        Self::start_options(options).await
    }

    pub async fn start_options(options: DaemonOptions) -> Self {
        let home = options.home.clone();
        let home = home.as_path();
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(urushi_daemon::run(options, async move {
            let _ = stopped.await;
        }));
        let mut daemon = Self {
            home: home.to_path_buf(),
            stop: Some(stop),
            task,
        };
        // Wait until it accepts connections.
        for _ in 0..200 {
            if Client::connect(&daemon.endpoint(), "probe").await.is_ok() {
                return daemon;
            }
            if daemon.task.is_finished() {
                let result = (&mut daemon.task).await;
                panic!("daemon did not start: {result:?}");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("daemon did not start within 5s");
    }

    pub fn endpoint(&self) -> Endpoint {
        Endpoint::Local(local_endpoint(&self.home))
    }

    pub async fn connect(&self) -> (Client, Events) {
        let (client, events) = Client::connect(&self.endpoint(), "test").await.unwrap();
        // Tests use a plain shell, so the user's shell config can't interfere.
        client
            .update_settings(v1::SettingsPatch {
                default_shell: Some("/bin/sh".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        (client, events)
    }

    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = tokio::time::timeout(TIMEOUT, self.task).await;
    }
}

/// A short temp dir (Unix socket paths are limited to ~100 bytes).
pub fn temp_home() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("ur")
        .tempdir_in(std::env::temp_dir())
        .unwrap()
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub struct Fixture {
    pub root: PathBuf,
    pub repo: PathBuf,
    pub remote: PathBuf,
}

/// A repo on `main`, pushed to a bare `origin` whose HEAD is `main`.
pub fn repo(dir: &Path) -> Fixture {
    let root = std::fs::canonicalize(dir).unwrap();
    // Git can't use Windows' verbatim form (`\\?\C:\…`); the daemon strips it the same way.
    #[cfg(windows)]
    let root = PathBuf::from(root.to_str().unwrap().trim_start_matches(r"\\?\"));
    let remote = root.join("origin.git");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(
        &root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("README"), "hello\n").unwrap();
    git(&repo, &["add", "README"]);
    git(&repo, &["commit", "-q", "-m", "initial"]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    git(&repo, &["remote", "set-head", "origin", "main"]);
    Fixture { root, repo, remote }
}

pub fn branches(dir: &Path) -> Vec<String> {
    git(
        dir,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .lines()
    .map(str::to_owned)
    .collect()
}

/// Polls `read_screen_text` until the screen contains `needle` (also across wrapped lines).
pub async fn wait_for_text(client: &Client, pane: &str, needle: &str) -> Vec<String> {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let lines = client.read_screen_text(pane).await.unwrap();
        if lines.iter().any(|l| l.contains(needle)) || lines.concat().contains(needle) {
            return lines;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {needle:?}; screen: {lines:#?}"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

pub async fn type_line(client: &Client, pane: &str, line: &str) {
    client
        .send_input(pane, v1::input::Kind::Text(format!("{line}\r")))
        .await
        .unwrap();
}

/// Panes of a tab in reading order.
pub fn panes_of(tab: &v1::Tab) -> Vec<String> {
    let layout = urushi_proto::convert::layout_from_proto(tab.layout.as_ref().unwrap()).unwrap();
    layout.panes().iter().map(ToString::to_string).collect()
}

/// Waits for the next event matching `pick`.
pub async fn next_event<T>(
    events: &mut Events,
    mut pick: impl FnMut(v1::event::Body) -> Option<T>,
) -> T {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let event = events.recv().await.expect("connection open");
            if let Some(found) = pick(event) {
                return found;
            }
        }
    })
    .await
    .expect("event arrived in time")
}
