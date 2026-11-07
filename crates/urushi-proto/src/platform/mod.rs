//! Platform-specific locations. The only `cfg(target_os)` code in this crate (ADR 0007).

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{default_data_dir, local_endpoint};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{default_data_dir, local_endpoint};
