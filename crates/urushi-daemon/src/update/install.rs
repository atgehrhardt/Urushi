//! Putting a downloaded release in place of the installed app (ADR 0049).
//!
//! The steps, each of which leaves the installed app untouched if it fails:
//! 1. download the zip and its `.sha256` into the daemon's private `updates` folder;
//! 2. check the zip's SHA-256 (`shasum`);
//! 3. unpack it (`ditto`) into a staging folder beside the app, on the same volume;
//! 4. check the new bundle: Urushi's identifier, the expected version, all three
//!    executables, and an intact signature (`codesign --verify`);
//! 5. swap it in with two renames, as `scripts/install-macos.sh` does. Running
//!    processes (the app, this daemon, the panes' CLIs) keep their unlinked binaries
//!    and carry on until they're restarted.
//!
//! curl doesn't mark what it downloads as quarantined, so Gatekeeper doesn't stop the
//! relaunch. Blocking: call it off the async runtime.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::bundle::{self, BUNDLE_ID, EXECUTABLES};
use super::github::{self, Release};
use crate::exec;
use crate::http::HttpClient;

/// How long unpacking, hashing or verifying the app may take.
const TOOL_TIMEOUT: Duration = Duration::from_secs(120);

/// Downloads `release` into `work`, checks it, and puts it in place of `bundle`.
pub fn install(
    http: &dyn HttpClient,
    release: &Release,
    bundle: &Path,
    work: &Path,
) -> Result<(), String> {
    let zip = download(http, release, work);
    let result = zip.and_then(|zip| replace(bundle, &zip, release));
    let _ = std::fs::remove_dir_all(work);
    result
}

/// Downloads the zip and checks its SHA-256. Returns where it is.
fn download(http: &dyn HttpClient, release: &Release, work: &Path) -> Result<PathBuf, String> {
    let _ = std::fs::remove_dir_all(work);
    crate::platform::secure_dir(work)
        .map_err(|e| format!("couldn't make {}: {e}", work.display()))?;
    let sha_file = work.join("release.sha256");
    http.download(&release.sha256_url, &sha_file)
        .map_err(|e| format!("couldn't download the checksum: {e}"))?;
    let expected = github::parse_sha256(&read(&sha_file)?)?;
    let zip = work.join("Urushi.zip");
    http.download(&release.zip_url, &zip)
        .map_err(|e| format!("couldn't download the update: {e}"))?;
    let actual = sha256(&zip)?;
    if actual != expected {
        return Err("the download is damaged (its checksum doesn't match); try again".into());
    }
    Ok(zip)
}

/// Unpacks `zip` beside `bundle`, checks it, and swaps it in.
pub fn replace(bundle: &Path, zip: &Path, release: &Release) -> Result<(), String> {
    let parent = bundle
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", bundle.display()))?;
    let name = bundle
        .file_name()
        .ok_or_else(|| format!("{} has no name", bundle.display()))?
        .to_string_lossy()
        .into_owned();
    let stage = parent.join(format!(".{name}.update"));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir(&stage).map_err(|e| {
        format!(
            "Urushi can't write to {} ({e}); install the update by hand",
            parent.display()
        )
    })?;
    let result = unpack(zip, &stage)
        .and_then(|new| check(&new, release).map(|()| new))
        .and_then(|new| swap(bundle, &new, &parent.join(format!(".{name}.old"))));
    let _ = std::fs::remove_dir_all(&stage);
    result
}

/// Unpacks `zip` into `stage` and finds the one app bundle inside.
fn unpack(zip: &Path, stage: &Path) -> Result<PathBuf, String> {
    tool("ditto", &["-x", "-k", &arg(zip), &arg(stage)])
        .map_err(|e| format!("couldn't unpack the update: {e}"))?;
    let mut apps = std::fs::read_dir(stage)
        .map_err(|e| format!("couldn't read the unpacked update: {e}"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "app"));
    match (apps.next(), apps.next()) {
        (Some(app), None) => Ok(app),
        _ => Err("the update doesn't hold exactly one app".into()),
    }
}

/// Checks that `new` is a complete, intact Urushi of `release`'s version.
pub fn check(new: &Path, release: &Release) -> Result<(), String> {
    let info = bundle::info(new).ok_or("the update has no readable Info.plist")?;
    if info.id != BUNDLE_ID {
        return Err(format!("the update isn't Urushi (it is {:?})", info.id));
    }
    if info.version != Some(release.version) {
        return Err(format!(
            "the update isn't version {} (it says {})",
            release.version,
            info.version
                .map_or_else(|| "nothing".into(), |v| v.to_string())
        ));
    }
    if let Some(missing) = EXECUTABLES.iter().find(|e| !new.join(e).is_file()) {
        return Err(format!("the update is incomplete (no {missing})"));
    }
    tool("codesign", &["--verify", "--deep", "--strict", &arg(new)])
        .map_err(|e| format!("the update's signature is broken: {e}"))?;
    Ok(())
}

/// Puts `new` where `bundle` is, moving the old one aside to `old` and deleting it. If
/// the second rename fails, the old bundle goes back.
fn swap(bundle: &Path, new: &Path, old: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(old);
    std::fs::rename(bundle, old)
        .map_err(|e| format!("couldn't move the installed app aside: {e}"))?;
    if let Err(e) = std::fs::rename(new, bundle) {
        let _ = std::fs::rename(old, bundle);
        return Err(format!("couldn't put the update in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(old);
    Ok(())
}

/// The file's SHA-256 in lowercase hex, from `shasum` (part of macOS).
fn sha256(file: &Path) -> Result<String, String> {
    let out = tool("shasum", &["-a", "256", &arg(file)])
        .map_err(|e| format!("couldn't check the download: {e}"))?;
    github::parse_sha256(&String::from_utf8_lossy(&out))
}

fn tool(program: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    exec::run(program, args, None, TOOL_TIMEOUT)
}

fn arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("couldn't read {}: {e}", path.display()))
}
