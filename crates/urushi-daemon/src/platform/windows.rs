use std::path::{Path, PathBuf};
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

/// Replaces the file at `path` with `bytes`: a temp file, flushed, then renamed over it.
/// Like the home it lives in, it inherits the user-only ACL of `%LOCALAPPDATA%`.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
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
                close: None,
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

/// The absolute path of `path`, with symlinks resolved, in the plain `C:\…` form.
/// `std::fs::canonicalize` returns verbatim `\\?\C:\…` paths, which git and most
/// programs can't use, so the verbatim prefix is removed where the plain form means the
/// same file.
pub(crate) fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(strip_verbatim)
}

/// `\\?\C:\x` → `C:\x` and `\\?\UNC\server\share` → `\\server\share`; anything else as is.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_are_removed() {
        let strip = |p: &str| strip_verbatim(PathBuf::from(p));
        assert_eq!(strip(r"\\?\C:\Users\me"), PathBuf::from(r"C:\Users\me"));
        assert_eq!(
            strip(r"\\?\UNC\host\share\x"),
            PathBuf::from(r"\\host\share\x")
        );
        assert_eq!(
            strip(r"\\?\GLOBALROOT\x"),
            PathBuf::from(r"\\?\GLOBALROOT\x")
        );
        assert_eq!(strip(r"C:\plain"), PathBuf::from(r"C:\plain"));
    }

    #[test]
    fn canonical_paths_are_plain() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = canonicalize(dir.path()).unwrap();
        assert!(
            !canonical.to_str().unwrap().starts_with(r"\\?\"),
            "{canonical:?}"
        );
        assert!(canonical.is_dir());
    }
}
