//! Platform-specific daemon plumbing (ADR 0007): local IPC listener, directory
//! permissions, private files, canonical paths, shutdown signals, finding executables, listing processes, and where
//! agents keep their sign-ins.

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
