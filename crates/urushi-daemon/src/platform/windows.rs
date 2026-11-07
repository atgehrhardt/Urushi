use std::path::Path;
use std::sync::Arc;

use tokio::net::windows::named_pipe::ServerOptions;
use urushi_proto::endpoint::LocalEndpoint;

use crate::server::connection::{self, Peer};
use crate::service::Service;

/// Creates `dir`. The default ACL of a user's `%LOCALAPPDATA%` already limits access to
/// that user.
pub(crate) fn secure_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// Accepts local connections on a named pipe, forever. Remote (SMB) clients are rejected.
pub(crate) async fn serve_local(
    service: Arc<Service>,
    endpoint: &LocalEndpoint,
) -> std::io::Result<()> {
    let LocalEndpoint::NamedPipe(name) = endpoint else {
        return Err(std::io::Error::other(
            "windows daemons listen on a named pipe",
        ));
    };
    tracing::info!(pipe = %name, "listening");
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(name)?;
    loop {
        server.connect().await?;
        let connected = server;
        server = ServerOptions::new()
            .reject_remote_clients(true)
            .create(name)?;
        let service = service.clone();
        tokio::spawn(connection::serve(
            service,
            connected,
            Peer {
                description: "local".into(),
            },
        ));
    }
}

/// Resolves on Ctrl-C.
pub(crate) async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
