//! Platform-specific PTY details (ADR 0007), behind a platform-neutral API.

use std::path::PathBuf;

use portable_pty::MasterPty;

/// The path of the terminal device behind `master` (e.g. `/dev/ttys004`), so detached
/// helpers can write to it. `None` where the device has no path (Windows `ConPTY`).
#[cfg(unix)]
pub(crate) fn tty_path(master: &dyn MasterPty) -> Option<PathBuf> {
    master.tty_name()
}

/// The path of the terminal device behind `master`. `ConPTY` devices have none.
#[cfg(not(unix))]
pub(crate) fn tty_path(_master: &dyn MasterPty) -> Option<PathBuf> {
    None
}
