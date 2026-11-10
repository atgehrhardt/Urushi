//! Finding installed agent CLIs (Claude, Codex, Cursor) on the daemon's machine.
//!
//! Pickers only offer agents that are installed, and a tab starts one by typing its
//! command into the user's shell (ADR 0009). The search covers the `PATH` of the user's
//! login shell (what terminals see), the daemon's own `PATH`, and common install
//! directories that a profile may not have added yet. An agent found only in one of
//! those extra directories is started by its full path, since its name wouldn't resolve.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use urushi_core::Agent;

use crate::platform;
use crate::service::lock;

/// How long the login-shell probe may take before it's abandoned.
const SHELL_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a probed search path is reused, so a newly installed agent shows up soon
/// without spawning a login shell for every picker.
const SEARCH_PATH_TTL: Duration = Duration::from_secs(60);

/// Directories, relative to the home directory, where agent installers put binaries.
const HOME_DIRS: &[&str] = &[
    ".local/bin",
    ".claude/local",
    ".npm-global/bin",
    ".bun/bin",
    ".volta/bin",
    ".cargo/bin",
];
/// Absolute directories that package managers use.
const SYSTEM_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];

/// An installed agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundAgent {
    /// Which agent.
    pub agent: Agent,
    /// The executable.
    pub path: PathBuf,
    /// What to type to run it: the bare name if the shell will find it, else the path.
    pub program: String,
}

/// One directory to search, and whether the user's shell has it on its `PATH`.
#[derive(Clone, Debug)]
struct SearchDir {
    dir: PathBuf,
    on_shell_path: bool,
}

enum Source {
    /// The real machine: probe the login shell, plus well-known directories.
    System {
        cache: Mutex<Option<(Instant, Arc<Vec<SearchDir>>)>>,
    },
    /// Fixed directories (tests). None of them is assumed to be on the shell's `PATH`.
    Fixed(Arc<Vec<SearchDir>>),
}

/// Finds installed agent CLIs. Its methods block (they read the filesystem and may run a
/// shell), so call them off the async runtime.
pub struct AgentLocator {
    source: Source,
}

impl AgentLocator {
    /// Searches this machine the way the user's terminals would.
    pub fn system() -> Self {
        Self {
            source: Source::System {
                cache: Mutex::new(None),
            },
        }
    }

    /// Searches only `dirs`. Agents found there are started by their full path.
    pub fn in_dirs(dirs: impl IntoIterator<Item = PathBuf>) -> Self {
        let dirs = dirs
            .into_iter()
            .map(|dir| SearchDir {
                dir,
                on_shell_path: false,
            })
            .collect();
        Self {
            source: Source::Fixed(Arc::new(dirs)),
        }
    }

    /// Every installed agent, in display order. `shell` is the shell new panes run
    /// (`None` = the user's login shell); its profile decides the `PATH` searched.
    pub fn installed(&self, shell: Option<&str>) -> Vec<FoundAgent> {
        let dirs = self.search_dirs(shell);
        Agent::ALL
            .into_iter()
            .filter_map(|agent| find(agent, &dirs))
            .collect()
    }

    /// `agent`, if it is installed.
    pub fn find(&self, agent: Agent, shell: Option<&str>) -> Option<FoundAgent> {
        find(agent, &self.search_dirs(shell))
    }

    fn search_dirs(&self, shell: Option<&str>) -> Arc<Vec<SearchDir>> {
        match &self.source {
            Source::Fixed(dirs) => dirs.clone(),
            Source::System { cache } => {
                if let Some((at, dirs)) = lock(cache).as_ref()
                    && at.elapsed() < SEARCH_PATH_TTL
                {
                    return dirs.clone();
                }
                // Probe without holding the lock: it can take a while.
                let dirs = Arc::new(system_dirs(shell));
                *lock(cache) = Some((Instant::now(), dirs.clone()));
                dirs
            }
        }
    }
}

fn system_dirs(shell: Option<&str>) -> Vec<SearchDir> {
    let shell = shell
        .map(str::to_owned)
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/sh".to_owned());
    let shell_path = platform::login_shell_path(&shell, SHELL_PROBE_TIMEOUT);
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    search_order(
        shell_path.as_ref(),
        std::env::var_os("PATH").as_ref(),
        home.as_deref().map(Path::new),
    )
}

/// The login shell's `PATH`, then the daemon's, then the extra install directories.
fn search_order(
    shell_path: Option<&OsString>,
    daemon_path: Option<&OsString>,
    home: Option<&Path>,
) -> Vec<SearchDir> {
    let on_path = [shell_path, daemon_path]
        .into_iter()
        .flatten()
        .flat_map(std::env::split_paths)
        .map(|dir| (dir, true));
    let extra = home
        .into_iter()
        .flat_map(|home| HOME_DIRS.iter().map(move |d| home.join(d)))
        .chain(SYSTEM_DIRS.iter().map(PathBuf::from))
        .map(|dir| (dir, false));
    let mut dirs: Vec<SearchDir> = Vec::new();
    for (dir, on_shell_path) in on_path.chain(extra) {
        if dir.as_os_str().is_empty() || dirs.iter().any(|d| d.dir == dir) {
            continue;
        }
        dirs.push(SearchDir { dir, on_shell_path });
    }
    dirs
}

fn find(agent: Agent, dirs: &[SearchDir]) -> Option<FoundAgent> {
    let names = platform::executable_names(agent.program());
    dirs.iter().find_map(|search| {
        names
            .iter()
            .map(|name| search.dir.join(name))
            .find(|path| platform::is_executable(path))
            .map(|path| FoundAgent {
                agent,
                program: if search.on_shell_path {
                    agent.program().to_owned()
                } else {
                    path.to_string_lossy().into_owned()
                },
                path,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_path_comes_first_and_duplicates_are_dropped() {
        let shell = std::env::join_paths(["/a", "/b"]).unwrap();
        let daemon = std::env::join_paths(["/b", "/usr/bin"]).unwrap();
        let dirs = search_order(Some(&shell), Some(&daemon), Some(Path::new("/home/me")));
        let listed: Vec<_> = dirs
            .iter()
            .map(|d| (d.dir.to_string_lossy().into_owned(), d.on_shell_path))
            .collect();
        assert_eq!(listed[0], ("/a".into(), true));
        assert_eq!(listed[1], ("/b".into(), true));
        assert_eq!(listed[2], ("/usr/bin".into(), true));
        let local_bin = Path::new("/home/me").join(".local/bin");
        assert!(listed.contains(&(local_bin.to_string_lossy().into_owned(), false)));
        assert_eq!(listed.iter().filter(|(d, _)| d == "/b").count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn finds_only_installed_executables_and_types_full_paths_off_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        std::fs::write(&claude, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Not executable, so not installed.
        std::fs::write(dir.path().join("codex"), "").unwrap();

        let locator = AgentLocator::in_dirs([dir.path().to_path_buf()]);
        let found = locator.installed(None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].agent, Agent::Claude);
        assert_eq!(found[0].program, claude.to_string_lossy());
        assert!(locator.find(Agent::Codex, None).is_none());

        let on_path = [SearchDir {
            dir: dir.path().to_path_buf(),
            on_shell_path: true,
        }];
        assert_eq!(find(Agent::Claude, &on_path).unwrap().program, "claude");
    }
}
