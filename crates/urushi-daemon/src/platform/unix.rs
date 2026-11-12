use std::ops::ControlFlow;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::net::UnixListener;
use urushi_proto::endpoint::LocalEndpoint;

use crate::server::accept::accept_forever;
use crate::server::connection::{self, Peer};
use crate::service::Service;

/// The absolute path of `path`, with symlinks resolved.
pub(crate) fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// Creates `dir` readable only by the current user (it holds the socket and state).
pub(crate) fn secure_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// Replaces the file at `path` with `bytes`, readable and writable only by this user
/// (0600): a temp file made with that mode, fsynced, then renamed over it.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Accepts local connections from the same OS user, forever.
pub(crate) async fn serve_local(
    service: Arc<Service>,
    endpoint: &LocalEndpoint,
) -> std::io::Result<()> {
    let LocalEndpoint::UnixSocket(path) = endpoint else {
        return Err(std::io::Error::other("unix daemons listen on a socket"));
    };
    if let Some(parent) = path.parent() {
        secure_dir(parent)?;
    }
    // We hold the instance lock, so any socket file left behind is stale.
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let owner = std::fs::metadata(service.home())?.uid();
    tracing::info!(socket = %path.display(), "listening");
    accept_forever(
        "local socket",
        || listener.accept(),
        |(stream, _)| {
            match stream.peer_cred().map(|c| c.uid()) {
                Ok(uid) if uid == owner => {}
                Ok(uid) => {
                    tracing::warn!(uid, "refused local connection from another user");
                    return ControlFlow::Continue(());
                }
                // The peer hung up before we could ask who it is (a client probing
                // whether the daemon is up does this).
                Err(error) => {
                    tracing::debug!(%error, "dropped a local connection that closed at once");
                    return ControlFlow::Continue(());
                }
            }
            tokio::spawn(connection::serve(
                service.clone(),
                stream,
                Peer {
                    description: "local".into(),
                    local: true,
                    close: None,
                },
            ));
            ControlFlow::Continue(())
        },
    )
    .await;
    Ok(())
}

/// Raises this process's soft limit on open files to its hard limit (at most
/// [`MAX_OPEN_FILES`]), and returns the `(before, after)` soft limits.
///
/// Every pane holds a few descriptors (its PTY, and a reader and writer onto it), and
/// so does every connection and every `git` or `curl` the daemon runs. A daemon started
/// from the macOS GUI inherits a soft limit of 256, which tens of panes use up; past it,
/// new panes, connections and commands all fail (UR-69). The hard limit is the user's
/// own ceiling, so raising to it needs no privilege.
pub(crate) fn raise_fd_limit() -> std::io::Result<(u64, u64)> {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
    let limit = getrlimit(Resource::Nofile);
    let before = limit.current.unwrap_or(u64::MAX);
    let target = limit.maximum.unwrap_or(u64::MAX).min(MAX_OPEN_FILES);
    if target <= before {
        return Ok((before, before));
    }
    setrlimit(
        Resource::Nofile,
        Rlimit {
            current: Some(target),
            maximum: limit.maximum,
        },
    )?;
    Ok((before, target))
}

/// The most open files [`raise_fd_limit`] asks for. macOS refuses a soft limit above
/// `OPEN_MAX` (10 240) even when the hard limit is unlimited, and it is plenty: hundreds
/// of panes need a few thousand.
pub(crate) const MAX_OPEN_FILES: u64 = 10_240;

/// Resolves on Ctrl-C or SIGTERM.
pub(crate) async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

/// Whether `path` is a file the current user may execute.
pub(crate) fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// File names under which `program` may be installed.
pub(crate) fn executable_names(program: &str) -> Vec<String> {
    vec![program.to_owned()]
}

/// The `PATH` an interactive login `shell` ends up with.
///
/// A daemon started by the GUI inherits launchd's minimal `PATH`, but agent CLIs are
/// usually installed in directories that the user's shell profile adds (`~/.local/bin`,
/// Homebrew, nvm). Terminals run that profile, so this is the `PATH` they see. Returns
/// `None` if the shell fails or takes longer than `timeout`.
pub(crate) fn login_shell_path(
    shell: &str,
    timeout: std::time::Duration,
) -> Option<std::ffi::OsString> {
    use std::io::Read;
    use std::os::unix::ffi::OsStringExt;
    use std::process::{Command, Stdio};

    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", "env"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = reader.join().ok()?;
    // Profiles may print their own text, so take the last line that looks like `PATH=`.
    out.split(|b| *b == b'\n')
        .filter_map(|line| line.strip_prefix(b"PATH="))
        .next_back()
        .map(|p| std::ffi::OsString::from_vec(p.to_vec()))
}

/// Every process on this machine, one line each, from `ps` (see `process::PS_COLUMNS`).
pub(crate) fn process_table() -> Option<String> {
    crate::exec::run(
        "ps",
        &["-A", "-o", crate::usage::process::PS_COLUMNS],
        None,
        std::time::Duration::from_secs(5),
    )
    .ok()
    .map(|out| String::from_utf8_lossy(&out).into_owned())
}

/// Claude Code's stored sign-in (JSON). On macOS it's normally in the login keychain,
/// read with `security` (as Claude Code itself does, so the system doesn't ask for
/// permission); otherwise, and as Claude's own fallback, `~/.claude/.credentials.json`.
pub(crate) fn claude_credentials() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let keychain = crate::exec::run(
            "security",
            &[
                "find-generic-password",
                "-s",
                "Claude Code-credentials",
                "-w",
            ],
            None,
            std::time::Duration::from_secs(5),
        );
        if let Ok(out) = keychain {
            return Ok(String::from_utf8_lossy(&out).trim().to_owned());
        }
    }
    crate::usage::claude_credentials_file()
}

#[cfg(test)]
mod tests {
    use rustix::process::{Resource, getrlimit};

    use super::*;

    #[test]
    fn the_open_file_limit_is_raised_within_the_hard_limit() {
        let (before, after) = raise_fd_limit().unwrap();
        assert!(after >= before);
        let limit = getrlimit(Resource::Nofile);
        assert_eq!(limit.current, Some(after));
        let hard = limit.maximum.unwrap_or(u64::MAX);
        assert!(after <= hard.min(MAX_OPEN_FILES).max(before));
        // Raising again changes nothing.
        assert_eq!(raise_fd_limit().unwrap(), (after, after));
    }
}
