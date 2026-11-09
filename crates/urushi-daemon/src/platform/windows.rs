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
                local: true,
            },
        ));
    }
}

/// Resolves on Ctrl-C.
pub(crate) async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// Whether `path` is a file (Windows decides executability by extension).
pub(crate) fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// File names under which `program` may be installed (npm installs `.cmd` shims).
pub(crate) fn executable_names(program: &str) -> Vec<String> {
    ["exe", "cmd", "bat"]
        .iter()
        .map(|ext| format!("{program}.{ext}"))
        .collect()
}

/// Windows shells don't have login profiles that change `PATH`; the daemon's own
/// environment already has the user's `PATH`.
pub(crate) fn login_shell_path(
    _shell: &str,
    _timeout: std::time::Duration,
) -> Option<std::ffi::OsString> {
    None
}

/// Listing processes isn't supported on Windows yet: no agent is ever detected.
pub(crate) fn process_table() -> Option<String> {
    None
}

/// Claude Code's stored sign-in (JSON), from `%USERPROFILE%\.claude\.credentials.json`.
pub(crate) fn claude_credentials() -> Result<String, String> {
    crate::usage::claude_credentials_file()
}
