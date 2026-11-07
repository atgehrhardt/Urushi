//! Finding, and if necessary starting, the local daemon.
//!
//! The daemon outlives every client (that is what makes sessions recoverable), so the
//! first client to need it starts it, detached, and later clients just connect.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use urushi_proto::endpoint::{self, Endpoint};

use crate::{Client, ClientError, Events, platform};

/// Environment variable naming the `urushid` binary to start.
pub const DAEMON_BIN_ENV: &str = "URUSHID";

const START_ATTEMPTS: u32 = 100;
const START_POLL: Duration = Duration::from_millis(50);

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
    if let Ok(connected) = Client::connect(&endpoint, client_name).await {
        return Ok(connected);
    }
    start_daemon(&home)?;
    let mut last_error = None;
    for _ in 0..START_ATTEMPTS {
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

fn daemon_binary() -> PathBuf {
    if let Some(explicit) = std::env::var_os(DAEMON_BIN_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(explicit);
    }
    let name = if cfg!(windows) {
        "urushid.exe"
    } else {
        "urushid"
    };
    // Next to the running executable (app bundles and cargo target dirs ship it there).
    if let Some(sibling) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(name)))
        && sibling.is_file()
    {
        return sibling;
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
