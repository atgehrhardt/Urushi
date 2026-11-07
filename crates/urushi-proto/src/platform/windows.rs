use std::path::{Path, PathBuf};

use crate::endpoint::LocalEndpoint;

/// `%LOCALAPPDATA%\Urushi`.
pub(crate) fn default_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(d).join("Urushi"))
}

/// A pipe name unique to `home`, so separate homes (e.g. tests) never collide.
pub(crate) fn local_endpoint(home: &Path) -> LocalEndpoint {
    // FNV-1a: stable across runs and Rust versions (unlike `DefaultHasher`).
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in home.to_string_lossy().to_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    LocalEndpoint::NamedPipe(format!(r"\\.\pipe\urushi-{hash:016x}"))
}
