//! Finding, starting, stopping and restarting the local daemon.
//!
//! The daemon outlives every client (that is what makes sessions recoverable), so the
//! first client to need it starts it, detached, and later clients just connect. That
//! also means installing a new version doesn't replace a running daemon: the user
//! restarts it on purpose ([`restart_local_daemon`]), which closes every terminal.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use urushi_proto::endpoint::{self, Endpoint};

use crate::{Client, ClientError, Events, platform};

/// Environment variable naming the `urushid` binary to start.
pub const DAEMON_BIN_ENV: &str = "URUSHID";

/// How long a just-started daemon has to accept connections. It restores every pane
/// before it listens, which takes a while with many of them (UR-69).
const START_TIMEOUT: Duration = Duration::from_secs(30);
const START_POLL: Duration = Duration::from_millis(50);
/// How long a stopping daemon gets to close its terminals and exit.
const STOP_TIMEOUT: Duration = Duration::from_secs(15);
/// First protocol minor version that accepts `ShutdownDaemon`.
const SHUTDOWN_SINCE_MINOR: u32 = 4;

/// How [`stop_local_daemon`] asks the daemon to stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopMethod {
    /// `ShutdownDaemon` if the daemon knows it, otherwise a signal (older daemons).
    Auto,
    /// Always a signal (SIGTERM), as for daemons before protocol 1.4. Unix only.
    Signal,
}

/// The daemon that was stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoppedDaemon {
    /// Its version.
    pub version: String,
    /// Its protocol minor version.
    pub protocol_minor: u32,
}

/// The home directory and endpoint of the local daemon. `home` overrides the default
/// (`URUSHI_HOME`, then the platform data dir).
pub fn local_daemon_endpoint(home: Option<&Path>) -> Result<(PathBuf, Endpoint), ClientError> {
    let home = home
        .map(Path::to_path_buf)
        .or_else(endpoint::home_dir)
        .ok_or_else(|| {
            ClientError::Launch(
                "this platform has no local daemon; connect to a remote host".into(),
            )
        })?;
    let local = Endpoint::Local(endpoint::local_endpoint(&home));
    Ok((home, local))
}

/// Connects to the local daemon, starting it first if it isn't running.
pub async fn ensure_local_daemon(
    home: Option<&Path>,
    client_name: &str,
) -> Result<(Client, Events), ClientError> {
    let (home, endpoint) = local_daemon_endpoint(home)?;
    match Client::connect(&endpoint, client_name).await {
        Ok(connected) => return Ok(connected),
        // A daemon is there but doesn't answer: starting another can't help (it would
        // find this one's lock and exit). Report it; the caller retries.
        Err(e @ ClientError::TimedOut(_)) => return Err(e),
        Err(_) => {}
    }
    start_daemon(&home)?;
    let mut last_error = None;
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(START_POLL).await;
        match Client::connect(&endpoint, client_name).await {
            Ok(connected) => return Ok(connected),
            Err(e) => last_error = Some(e),
        }
    }
    Err(ClientError::Launch(format!(
        "started urushid but could not connect ({}); see {}",
        last_error.map(|e| e.to_string()).unwrap_or_default(),
        home.join("urushid.log").display()
    )))
}

/// Stops the local daemon and waits until it has exited. `Ok(None)` if none was running.
///
/// Every terminal it runs closes. The next daemon brings the panes back from their
/// restore points (ADR 0013).
pub async fn stop_local_daemon(
    home: Option<&Path>,
    method: StopMethod,
) -> Result<Option<StoppedDaemon>, ClientError> {
    let (_, endpoint) = local_daemon_endpoint(home)?;
    let Endpoint::Local(local) = &endpoint else {
        return Ok(None);
    };
    let Ok((client, _events)) = Client::connect(&endpoint, "urushi-stop").await else {
        return Ok(None);
    };
    let welcome = client.welcome().clone();
    let stopped = StoppedDaemon {
        version: welcome.daemon_version,
        protocol_minor: welcome.protocol_minor,
    };
    let pid = platform::daemon_pid(local).await;
    if method == StopMethod::Auto && stopped.protocol_minor >= SHUTDOWN_SINCE_MINOR {
        client.shutdown_daemon().await?;
    } else {
        let pid = pid.ok_or_else(|| {
            ClientError::Launch(
                "this daemon is too old to be stopped from here; quit it or restart the computer"
                    .into(),
            )
        })?;
        platform::terminate(pid).map_err(|e| ClientError::Launch(e.to_string()))?;
    }
    drop(client);
    let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
    loop {
        let gone = match pid {
            Some(pid) => !platform::is_running(pid),
            None => Client::connect(&endpoint, "urushi-stop").await.is_err(),
        };
        if gone {
            return Ok(Some(stopped));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ClientError::Launch(
                "the daemon did not stop in time; its terminals may still be closing".into(),
            ));
        }
        tokio::time::sleep(START_POLL).await;
    }
}

/// Stops the local daemon (if any) and starts this installation's daemon in its place.
/// Returns what was stopped, and a connection to the new daemon.
pub async fn restart_local_daemon(
    home: Option<&Path>,
    client_name: &str,
) -> Result<(Option<StoppedDaemon>, Client, Events), ClientError> {
    let stopped = stop_local_daemon(home, StopMethod::Auto).await?;
    let (client, events) = ensure_local_daemon(home, client_name).await?;
    Ok((stopped, client, events))
}

fn daemon_binary() -> PathBuf {
    if let Some(explicit) = std::env::var_os(DAEMON_BIN_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(explicit);
    }
    let name = if cfg!(windows) {
        "urushid.exe"
    } else {
        "urushid"
    };
    // Next to the running executable (cargo target dirs, and the app's own binary), or
    // for the CLI inside a macOS bundle (`Contents/Resources/bin`), in `Contents/MacOS`.
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        for candidate in [dir.join(name), dir.join("../../MacOS").join(name)] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(name)
}

fn start_daemon(home: &Path) -> Result<(), ClientError> {
    std::fs::create_dir_all(home).map_err(|e| ClientError::Launch(e.to_string()))?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("urushid.log"))
        .map_err(|e| ClientError::Launch(e.to_string()))?;
    let log_err = log
        .try_clone()
        .map_err(|e| ClientError::Launch(e.to_string()))?;
    let binary = daemon_binary();
    let mut command = Command::new(&binary);
    command
        .arg("--home")
        .arg(home)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err);
    platform::detach(&mut command);
    command
        .spawn()
        .map(drop)
        .map_err(|e| ClientError::Launch(format!("{}: {e}", binary.display())))
}
