//! The macOS app bundle the daemon runs from, and what its `Info.plist` says.
//!
//! `scripts/bundle-macos.sh` writes `Info.plist` as XML, and ad-hoc signing leaves it so.
//! Two keys are read, which a few lines of string matching do without a plist library;
//! a binary plist simply isn't understood, and the caller falls back.

use std::path::{Path, PathBuf};

use urushi_core::Version;

/// Urushi's bundle identifier (`CFBundleIdentifier`).
pub const BUNDLE_ID: &str = "app.urushi.Urushi";

/// The executables a complete bundle holds, relative to it.
pub const EXECUTABLES: [&str; 3] = [
    "Contents/MacOS/Urushi",
    "Contents/MacOS/urushid",
    "Contents/Resources/bin/urushi",
];

/// The Urushi bundle holding the daemon executable `exe`
/// (`…/Urushi.app/Contents/MacOS/urushid`), or `None` for a daemon outside one (a
/// `cargo` build, a Linux install) or inside another app. The path is resolved first:
/// the CLI starts the daemon as `Contents/Resources/bin/../../MacOS/urushid`.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let exe = crate::platform::canonicalize(exe).ok()?;
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let named = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let is_app = bundle
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("app"));
    if !(named(macos, "MacOS") && named(contents, "Contents") && is_app) {
        return None;
    }
    (info(bundle)?.id == BUNDLE_ID).then(|| bundle.to_path_buf())
}

/// The keys of a bundle's `Info.plist` that updates care about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    /// `CFBundleIdentifier`.
    pub id: String,
    /// `CFBundleShortVersionString`, if it is a version.
    pub version: Option<Version>,
}

/// Reads `bundle/Contents/Info.plist`. `None` if it's missing or not an XML plist with
/// an identifier.
pub fn info(bundle: &Path) -> Option<Info> {
    let plist = std::fs::read_to_string(bundle.join("Contents").join("Info.plist")).ok()?;
    Some(Info {
        id: string_value(&plist, "CFBundleIdentifier")?,
        version: string_value(&plist, "CFBundleShortVersionString").and_then(|v| v.parse().ok()),
    })
}

/// The `<string>` right after `<key>key</key>` in an XML plist.
pub fn string_value(plist: &str, key: &str) -> Option<String> {
    let after_key = plist.split_once(&format!("<key>{key}</key>"))?.1;
    let value = after_key.trim_start().strip_prefix("<string>")?;
    let (text, _) = value.split_once("</string>")?;
    Some(unescape(text.trim()))
}

/// XML's predefined entities.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// An `Info.plist` for tests: the shape `scripts/bundle-macos.sh` writes.
#[cfg(test)]
pub(crate) fn test_plist(id: &str, version: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n  \
         <key>CFBundleName</key><string>Urushi</string>\n  \
         <key>CFBundleIdentifier</key><string>{id}</string>\n  \
         <key>CFBundleExecutable</key><string>Urushi</string>\n  \
         <key>CFBundleShortVersionString</key><string>{version}</string>\n</dict>\n</plist>\n"
    )
}
