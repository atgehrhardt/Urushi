//! Platform-specific daemon plumbing (ADR 0007): local IPC listener, directory
//! permissions, shutdown signals, finding executables, listing processes, and where
//! agents keep their sign-ins.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{
    claude_credentials, executable_names, is_executable, login_shell_path, process_table,
    secure_dir, serve_local, shutdown_signal,
};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{
    claude_credentials, executable_names, is_executable, login_shell_path, process_table,
    secure_dir, serve_local, shutdown_signal,
};
