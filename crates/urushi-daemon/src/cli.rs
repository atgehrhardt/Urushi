//! The `urushi` CLI next to the daemon, and the environment every pane gets so programs
//! in it (agents) can run it against this daemon (ADR 0024).
//!
//! Panes get `URUSHI_HOME` (so the CLI reaches this daemon, whichever home it runs in:
//! the real one or a dev one) and, when the CLI is found, its absolute path in
//! [`CLI_ENV`] and its directory first on `PATH`. A login shell may rebuild `PATH` and
//! put another `urushi` ahead of it, so prompts name the CLI by its path (ADR 0033).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The pane variable holding the CLI's absolute path, for programs that must reach
/// this daemon's CLI whatever comes first on `PATH`.
pub const CLI_ENV: &str = "URUSHI_CLI";

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

/// [`locate`] for a daemon in `dir`. The bundle's directory is named without `..`, as
/// the path shows up in agents' prompts.
pub fn locate_from(dir: &Path) -> Option<PathBuf> {
    let name = file_name();
    let bundle = dir.parent().map(|c| c.join("Resources").join("bin"));
    std::iter::once(dir.to_path_buf())
        .chain(bundle)
        .find_map(|d| exact_file(&d, &name))
}

/// `dir/name` if a file of exactly that name is there. On a case-insensitive disk
/// (macOS) `dir/urushi` also opens the app's `Urushi` binary, so asking the path alone
/// would take the GUI for the CLI; the directory listing has the real name.
fn exact_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let listed = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .any(|e| e.file_name() == name);
    let path = dir.join(name);
    (listed && path.is_file()).then_some(path)
}

/// The variables a pane gets for the CLI: `URUSHI_HOME`, and when the CLI is known,
/// [`CLI_ENV`] and `PATH` with the CLI's directory first (when `path` can hold it).
pub fn pane_env(home: &Path, cli: Option<&Path>, path: Option<OsString>) -> Vec<(String, String)> {
    let mut env = vec![(
        "URUSHI_HOME".to_owned(),
        home.to_string_lossy().into_owned(),
    )];
    if let Some(cli) = cli {
        env.push((CLI_ENV.to_owned(), cli.to_string_lossy().into_owned()));
    }
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
            locate_from(&macos),
            Some(
                dir.path()
                    .join("Urushi.app/Contents/Resources/bin")
                    .join(file_name())
            ),
            "named without `..`"
        );
        std::fs::write(macos.join(file_name()), b"").unwrap();
        assert_eq!(locate_from(&macos), Some(macos.join(file_name())));
    }

    /// On a case-insensitive disk (macOS), `Contents/MacOS/urushi` opens the app's own
    /// `Urushi` binary. That GUI must never be put on panes' `PATH` as the CLI.
    #[test]
    fn the_app_binary_is_not_taken_for_the_cli() {
        let dir = tempfile::tempdir().unwrap();
        let macos = dir.path().join("Urushi.app/Contents/MacOS");
        let bin = dir.path().join("Urushi.app/Contents/Resources/bin");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let app = format!("Urushi{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(macos.join(&app), b"gui").unwrap();
        assert_eq!(locate_from(&macos), None, "the GUI alone is no CLI");
        std::fs::write(bin.join(file_name()), b"cli").unwrap();
        let found = locate_from(&macos).unwrap();
        assert_eq!(std::fs::read(&found).unwrap(), b"cli");
        assert!(found.ends_with(format!("bin/{}", file_name())));
    }

    #[test]
    fn panes_reach_this_daemon_and_its_cli_first() {
        let home = Path::new("/home/a/Urushi Dev");
        let cli = Path::new("/opt/urushi/bin/urushi");
        let env = pane_env(home, Some(cli), Some(OsString::from("/usr/bin")));
        assert_eq!(env[0], ("URUSHI_HOME".into(), "/home/a/Urushi Dev".into()));
        assert_eq!(env[1], (CLI_ENV.into(), "/opt/urushi/bin/urushi".into()));
        assert_eq!(env[2].0, "PATH");
        let path = &env[2].1;
        let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
        assert_eq!(
            dirs,
            [PathBuf::from("/opt/urushi/bin"), PathBuf::from("/usr/bin")]
        );
        assert_eq!(
            pane_env(home, None, None).len(),
            1,
            "no CLI: no PATH change, no URUSHI_CLI"
        );
    }
}
