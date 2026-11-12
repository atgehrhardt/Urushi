//! Platform-specific daemon plumbing (ADR 0007): local IPC listener, directory
//! permissions, private files, canonical paths, shutdown signals, finding executables, listing processes, where
//! agents keep their sign-ins, and the open-file limit.

/// Whether this OS has app updates (ADR 0049): only macOS has a released app bundle.
pub(crate) const UPDATES_SUPPORTED: bool = cfg!(target_os = "macos");

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{
    canonicalize, claude_credentials, executable_names, is_executable, login_shell_path,
    process_table, secure_dir, serve_local, shutdown_signal, write_private,
};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{
    canonicalize, claude_credentials, executable_names, is_executable, login_shell_path,
    process_table, secure_dir, serve_local, shutdown_signal, write_private,
};

/// Raises the process's open-file limit as far as the user may, and logs the result.
/// Failing to raise it is logged, never fatal: the daemon still works, with fewer panes.
pub(crate) fn raise_open_file_limit() {
    #[cfg(unix)]
    match unix::raise_fd_limit() {
        Ok((before, after)) => tracing::info!(before, after, "open file limit"),
        Err(error) => tracing::warn!(%error, "could not raise the open file limit"),
    }
    // Windows has no comparable per-process limit on handles.
}
