use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;

use tokio::net::UnixListener;
use urushi_proto::endpoint::LocalEndpoint;

use crate::server::connection::{self, Peer};
use crate::service::Service;

/// Creates `dir` readable only by the current user (it holds the socket and state).
pub(crate) fn secure_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
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
    loop {
        let (stream, _) = listener.accept().await?;
        let uid = stream.peer_cred().map(|c| c.uid()).ok();
        if uid != Some(owner) {
            tracing::warn!(?uid, "refused local connection from another user");
            continue;
        }
        let service = service.clone();
        tokio::spawn(connection::serve(
            service,
            stream,
            Peer {
                description: "local".into(),
            },
        ));
    }
}

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
