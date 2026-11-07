use std::path::{Path, PathBuf};

use crate::endpoint::LocalEndpoint;

/// Unix socket paths must fit in `sockaddr_un.sun_path` (104 bytes on macOS, 108 on
/// Linux, including the terminating NUL).
const MAX_SOCKET_PATH: usize = 100;

/// The app data dir: `~/Library/Application Support/Urushi` on macOS, and
/// `$XDG_DATA_HOME/urushi` (or `~/.local/share/urushi`) on Linux and other Unixes.
/// `None` on iOS and Android, which never run a local daemon.
pub(crate) fn default_data_dir() -> Option<PathBuf> {
    if cfg!(any(target_os = "ios", target_os = "android")) {
        return None;
    }
    let home = user_home()?;
    if cfg!(target_os = "macos") {
        return Some(home.join("Library/Application Support/Urushi"));
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map_or_else(|| home.join(".local/share"), PathBuf::from);
    Some(data.join("urushi"))
}

fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// `<home>/urushid.sock`, or, when that path is too long for a socket, a short path
/// unique to `home` under `~/.urushi/sockets/` (a directory only the user can access).
pub(crate) fn local_endpoint(home: &Path) -> LocalEndpoint {
    let preferred = home.join("urushid.sock");
    if preferred.as_os_str().len() <= MAX_SOCKET_PATH {
        return LocalEndpoint::UnixSocket(preferred);
    }
    let base = user_home().unwrap_or_else(|| PathBuf::from("/tmp"));
    LocalEndpoint::UnixSocket(
        base.join(".urushi/sockets")
            .join(format!("{:016x}.sock", fnv1a(home))),
    )
}

fn fnv1a(path: &Path) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_homes_get_short_socket_paths() {
        let long = PathBuf::from("/").join("x".repeat(120));
        let LocalEndpoint::UnixSocket(path) = local_endpoint(&long) else {
            panic!()
        };
        assert!(path.as_os_str().len() <= MAX_SOCKET_PATH || !path.starts_with(&long));
        assert!(path.to_string_lossy().contains(".urushi/sockets/"));
        let LocalEndpoint::UnixSocket(again) = local_endpoint(&long) else {
            panic!()
        };
        assert_eq!(path, again, "stable for the same home");
    }
}
