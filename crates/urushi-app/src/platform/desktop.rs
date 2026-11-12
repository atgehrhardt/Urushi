/// The system clipboard (text only).
pub(crate) struct Clipboard(Option<arboard::Clipboard>);

impl Clipboard {
    pub(crate) fn new() -> Self {
        Self(arboard::Clipboard::new().ok())
    }

    pub(crate) fn set(&mut self, text: &str) {
        if let Some(c) = &mut self.0 {
            let _ = c.set_text(text.to_owned());
        }
    }

    pub(crate) fn get(&mut self) -> Option<String> {
        self.0.as_mut().and_then(|c| c.get_text().ok())
    }
}

/// Shows the system folder picker (blocking, on the UI thread, as the OS requires).
pub(crate) fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Choose a repository")
        .pick_folder()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Desktop has a system folder picker.
pub(crate) const HAS_FOLDER_PICKER: bool = true;

/// Shows the system file picker for attachments (blocking, on the UI thread).
pub(crate) fn pick_files() -> Vec<String> {
    rfd::FileDialog::new()
        .set_title("Attach files")
        .pick_files()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Desktop has a system file picker.
pub(crate) const HAS_FILE_PICKER: bool = true;

/// Posts a system notification. Runs on its own thread: on Linux it is a D-Bus call,
/// which must not hold up the UI. Failures (no notification server) are only logged.
pub(crate) fn notify(title: &str, body: &str) {
    let (title, body) = (title.to_owned(), body.to_owned());
    let spawned = std::thread::Builder::new()
        .name("notify".into())
        .spawn(move || {
            notifications::identify_app();
            let shown = notify_rust::Notification::new()
                .appname("Urushi")
                .summary(&title)
                .body(&body)
                .show();
            if let Err(e) = shown {
                tracing::debug!(error = %e, "couldn't post a notification");
            }
        });
    if let Err(e) = spawned {
        tracing::debug!(error = %e, "couldn't start the notification thread");
    }
}

/// Desktop has a secondary (right or two-finger) click, which opens context menus.
pub(crate) const HAS_SECONDARY_CLICK: bool = true;

/// Opens a file with the system's default app for it.
pub(crate) fn open_path(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer");
    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");
    // The opener returns at once on macOS and Linux; Explorer's exit code means
    // nothing, so it isn't waited for.
    command.arg(path).spawn().map(drop)
}

/// Whether the app can be updated in place (ADR 0049): only macOS has a released app
/// bundle the daemon can replace.
pub(crate) const HAS_APP_UPDATES: bool = cfg!(target_os = "macos");

/// Opens the app bundle at `bundle` once this process has exited, so the new version
/// starts as the old one quits (ADR 0049). A detached shell waits for this process,
/// then runs `open`. The caller quits right after.
pub(crate) fn relaunch(bundle: &str) -> std::io::Result<()> {
    if !HAS_APP_UPDATES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "relaunching is only possible on macOS",
        ));
    }
    let script = "while kill -0 \"$0\" 2>/dev/null; do sleep 0.2; done; exec open \"$1\"";
    std::process::Command::new("/bin/sh")
        .args(["-c", script])
        .arg(std::process::id().to_string())
        .arg(bundle)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
}

/// Opens a web page in the system's browser. Only `https` addresses: anything else
/// (or an address the opener could read as an option) is refused.
pub(crate) fn open_url(url: &str) -> std::io::Result<()> {
    if !url.starts_with("https://") || url.chars().any(char::is_whitespace) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a web address",
        ));
    }
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer");
    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("xdg-open");
    command.arg(url).spawn().map(drop)
}

/// The window and taskbar icon (Windows, Linux; macOS uses the bundle's icon instead):
/// the app icon as raw 64×64 RGBA, rendered by `scripts/render-icons.swift`.
pub(crate) fn window_icon() -> Option<winit::window::Icon> {
    const SIZE: u32 = 64;
    let rgba = include_bytes!("../../../../assets/icon/window-64.rgba");
    winit::window::Icon::from_rgba(rgba.to_vec(), SIZE, SIZE).ok()
}

/// Who notifications come from.
mod notifications {
    /// On macOS, notifications from the installed app carry its name and icon, and
    /// clicking one brings it to the front. A bare executable (the dev build) has no
    /// bundle, so its notifications keep the system default.
    #[cfg(target_os = "macos")]
    pub(super) fn identify_app() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let bundled = std::env::current_exe()
                .is_ok_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/"));
            if bundled && let Err(e) = notify_rust::set_application(BUNDLE_ID) {
                tracing::debug!(error = %e, "couldn't set the notification app");
            }
        });
    }

    /// Elsewhere the application name is enough.
    #[cfg(not(target_os = "macos"))]
    pub(super) fn identify_app() {}

    /// `CFBundleIdentifier` in `scripts/bundle-macos.sh`.
    #[cfg(target_os = "macos")]
    const BUNDLE_ID: &str = "app.urushi.Urushi";
}

/// This computer's name, which a LAN host lists this device under when it pairs.
pub(crate) fn device_name() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Urushi desktop".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_window_icon_is_a_valid_64px_image() {
        assert!(super::window_icon().is_some());
    }
}
