use std::process::Command;

use tokio::net::UnixStream;
use urushi_proto::endpoint::LocalEndpoint;

use super::Stream;

pub(crate) async fn connect_local(local: &LocalEndpoint) -> std::io::Result<Box<dyn Stream>> {
    match local {
        LocalEndpoint::UnixSocket(path) => Ok(Box::new(UnixStream::connect(path).await?)),
        LocalEndpoint::NamedPipe(_) => Err(super::unsupported(local)),
    }
}

/// Puts the child in its own process group, so closing the launching terminal (or
/// quitting the app) doesn't take the daemon down with it.
pub(crate) fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

/// The process id of whoever listens on `local`: the daemon. Asked from the OS
/// (the socket's peer credentials), so it is exactly the daemon for that home.
pub(crate) async fn daemon_pid(local: &LocalEndpoint) -> Option<u32> {
    let LocalEndpoint::UnixSocket(path) = local else {
        return None;
    };
    let stream = UnixStream::connect(path).await.ok()?;
    let pid = stream.peer_cred().ok()?.pid()?;
    u32::try_from(pid).ok().filter(|p| *p > 1)
}

/// Asks process `pid` to stop (SIGTERM), as the OS does at logout.
pub(crate) fn terminate(pid: u32) -> std::io::Result<()> {
    let status = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("kill -TERM {pid} failed")))
    }
}

/// Whether process `pid` is still running. A zombie (exited, but not yet collected by
/// its parent, e.g. a daemon this very app started) counts as stopped.
pub(crate) fn is_running(pid: u32) -> bool {
    let Ok(out) = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return false;
    };
    let state = String::from_utf8_lossy(&out.stdout);
    let state = state.trim();
    !state.is_empty() && !state.starts_with('Z')
}
