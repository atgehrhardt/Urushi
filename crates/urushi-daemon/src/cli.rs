//! The `urushi` CLI next to the daemon, and the environment every pane gets so programs
//! in it (agents) can run it against this daemon (ADR 0024).
//!
//! Panes get `URUSHI_HOME` (so the CLI reaches this daemon, whichever home it runs in:
//! the real one or a dev one) and, when the CLI is found, its directory first on `PATH`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The CLI's file name on this platform.
fn file_name() -> String {
    format!("urushi{}", std::env::consts::EXE_SUFFIX)
}

/// Where the CLI is, beside the daemon: in the same directory (a build, a Linux
/// install), or in the app bundle's `Resources/bin` (macOS). `None` if it isn't.
pub fn locate() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    locate_from(exe.parent()?)
}

/// [`locate`] for a daemon in `dir`.
pub fn locate_from(dir: &Path) -> Option<PathBuf> {
    [
        dir.join(file_name()),
        dir.join("..")
            .join("Resources")
            .join("bin")
            .join(file_name()),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// The variables a pane gets for the CLI: `URUSHI_HOME`, and `PATH` with the CLI's
/// directory first (when it is known and `path` can hold it).
pub fn pane_env(home: &Path, cli: Option<&Path>, path: Option<OsString>) -> Vec<(String, String)> {
    let mut env = vec![(
        "URUSHI_HOME".to_owned(),
        home.to_string_lossy().into_owned(),
    )];
    if let Some(dir) = cli.and_then(Path::parent) {
        let rest = path.map(|p| std::env::split_paths(&p).collect::<Vec<_>>());
        let all = std::iter::once(dir.to_path_buf()).chain(rest.into_iter().flatten());
        if let Some(joined) = std::env::join_paths(all)
            .ok()
            .and_then(|p| p.into_string().ok())
        {
            env.push(("PATH".to_owned(), joined));
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_is_found_beside_the_daemon_or_in_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let macos = dir.path().join("Urushi.app/Contents/MacOS");
        let bin = dir.path().join("Urushi.app/Contents/Resources/bin");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        assert_eq!(locate_from(&macos), None);
        std::fs::write(bin.join(file_name()), b"").unwrap();
        assert_eq!(
            locate_from(&macos).map(|p| p.ends_with(format!("bin/{}", file_name()))),
            Some(true)
        );
        std::fs::write(macos.join(file_name()), b"").unwrap();
        assert_eq!(locate_from(&macos), Some(macos.join(file_name())));
    }

    #[test]
    fn panes_reach_this_daemon_and_its_cli_first() {
        let home = Path::new("/home/a/Urushi Dev");
        let cli = Path::new("/opt/urushi/bin/urushi");
        let env = pane_env(home, Some(cli), Some(OsString::from("/usr/bin")));
        assert_eq!(env[0], ("URUSHI_HOME".into(), "/home/a/Urushi Dev".into()));
        let path = &env[1].1;
        let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
        assert_eq!(
            dirs,
            [PathBuf::from("/opt/urushi/bin"), PathBuf::from("/usr/bin")]
        );
        assert_eq!(
            pane_env(home, None, None).len(),
            1,
            "no CLI: no PATH change"
        );
    }
}
