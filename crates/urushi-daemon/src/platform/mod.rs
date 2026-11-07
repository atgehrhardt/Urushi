//! Platform-specific daemon plumbing (ADR 0007): local IPC listener, directory
//! permissions, and shutdown signals.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{secure_dir, serve_local, shutdown_signal};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{secure_dir, serve_local, shutdown_signal};
