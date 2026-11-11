//! Where a daemon can be reached.
//!
//! Local daemons live under the user's Urushi *home* directory (the app data dir). They
//! listen on a Unix socket there, or on a named pipe derived from it on Windows. Remote
//! daemons are reached over TCP on a tailnet address, or on a LAN address through an
//! encrypted channel (`lan`). `URUSHI_HOME` overrides the home
//! directory, which keeps tests and side-by-side installs isolated.

use std::fmt;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the home directory.
pub const HOME_ENV: &str = "URUSHI_HOME";

/// A daemon address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    /// A daemon on this machine.
    Local(LocalEndpoint),
    /// A daemon on another machine, typically a `MagicDNS` name or tailnet IP.
    Tcp {
        /// Host name or IP address.
        host: String,
        /// TCP port.
        port: u16,
    },
}

/// Local IPC address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalEndpoint {
    /// A Unix domain socket path (macOS, Linux).
    UnixSocket(PathBuf),
    /// A Windows named pipe name (`\\.\pipe\…`).
    NamedPipe(String),
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(LocalEndpoint::UnixSocket(path)) => write!(f, "unix:{}", path.display()),
            Self::Local(LocalEndpoint::NamedPipe(name)) => write!(f, "pipe:{name}"),
            Self::Tcp { host, port } => write!(f, "{host}:{port}"),
        }
    }
}

/// The platform's app data dir (see `platform/`). `None` where no local daemon runs.
pub fn default_data_dir() -> Option<PathBuf> {
    crate::platform::default_data_dir()
}

/// The home directory: `URUSHI_HOME` if set, otherwise the platform's app data dir.
/// `None` on platforms without a local daemon (iOS, Android) unless the variable is set.
///
/// **Debug builds use a separate `… Dev` home** (e.g. `~/Library/Application Support/Urushi
/// Dev`), so anything built with plain `cargo build`/`cargo run`, while developing Urushi
/// inside Urushi, can never touch the real workspace. Only release builds (what
/// `make install` ships) use the real home.
pub fn home_dir() -> Option<PathBuf> {
    match std::env::var_os(HOME_ENV) {
        Some(home) if !home.is_empty() => Some(PathBuf::from(home)),
        _ => default_data_dir().map(|dir| {
            if cfg!(debug_assertions) {
                dev_variant(&dir)
            } else {
                dir
            }
        }),
    }
}

/// `…/Urushi` → `…/Urushi Dev`, `…/urushi` → `…/urushi-dev`.
pub fn dev_variant(dir: &Path) -> PathBuf {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dev = if name.chars().next().is_some_and(char::is_uppercase) {
        format!("{name} Dev")
    } else {
        format!("{name}-dev")
    };
    dir.with_file_name(dev)
}

/// The local endpoint of the daemon whose home is `home`.
pub fn local_endpoint(home: &Path) -> LocalEndpoint {
    crate::platform::local_endpoint(home)
}

/// Parses `host:port` (or `[v6]:port`). A missing port uses `default_port`.
pub fn parse_tcp(spec: &str, default_port: u16) -> Option<Endpoint> {
    let (host, port) = split_host_port(spec)?;
    Some(Endpoint::Tcp {
        host,
        port: port.unwrap_or(default_port),
    })
}

/// Splits `host`, `host:port`, `[v6]:port` or a bare IPv6 address into the host and the
/// port, if one was given. A trailing dot on the host is dropped. `None` if `spec` is
/// empty or its port isn't a number.
pub fn split_host_port(spec: &str) -> Option<(String, Option<u16>)> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    let (host, port) = if let Some(rest) = spec.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => Some(p.parse().ok()?),
            None => None,
        };
        (host, port)
    } else if spec.matches(':').count() == 1 {
        let (host, port) = spec.split_once(':')?;
        (host, Some(port.parse().ok()?))
    } else {
        (spec, None)
    };
    Some((host.trim_end_matches('.').to_owned(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tcp_specs() {
        let tcp = |h: &str, p| {
            Some(Endpoint::Tcp {
                host: h.into(),
                port: p,
            })
        };
        assert_eq!(
            parse_tcp("box.tail.ts.net", 7420),
            tcp("box.tail.ts.net", 7420)
        );
        assert_eq!(
            parse_tcp("box.tail.ts.net.:9000", 7420),
            tcp("box.tail.ts.net", 9000)
        );
        assert_eq!(parse_tcp("100.1.2.3:81", 7420), tcp("100.1.2.3", 81));
        assert_eq!(parse_tcp("[fd7a::1]:82", 7420), tcp("fd7a::1", 82));
        assert_eq!(parse_tcp("fd7a::1", 7420), tcp("fd7a::1", 7420));
        assert_eq!(parse_tcp("", 7420), None);
        assert_eq!(parse_tcp("host:notaport", 7420), None);
    }

    #[test]
    fn splits_host_and_optional_port() {
        assert_eq!(
            split_host_port("mac.local"),
            Some(("mac.local".into(), None))
        );
        assert_eq!(
            split_host_port(" 192.168.1.10:7421 "),
            Some(("192.168.1.10".into(), Some(7421)))
        );
        assert_eq!(split_host_port("[fd12::1]"), Some(("fd12::1".into(), None)));
        assert_eq!(split_host_port("fd12::1"), Some(("fd12::1".into(), None)));
        assert_eq!(
            split_host_port("[fd12::1]:5"),
            Some(("fd12::1".into(), Some(5)))
        );
        assert_eq!(split_host_port("[fd12::1]:x"), None);
    }

    #[test]
    fn dev_homes_are_separate() {
        assert_eq!(
            dev_variant(Path::new("/a/Urushi")),
            PathBuf::from("/a/Urushi Dev")
        );
        assert_eq!(
            dev_variant(Path::new("/a/urushi")),
            PathBuf::from("/a/urushi-dev")
        );
    }

    #[test]
    fn local_endpoint_lives_under_home() {
        let ep = local_endpoint(Path::new("/tmp/urushi-home"));
        match ep {
            LocalEndpoint::UnixSocket(path) => assert!(path.starts_with("/tmp/urushi-home")),
            LocalEndpoint::NamedPipe(name) => assert!(name.starts_with(r"\\.\pipe\urushi-")),
        }
    }
}
