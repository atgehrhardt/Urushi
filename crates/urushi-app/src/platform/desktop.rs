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

#[cfg(test)]
mod tests {
    #[test]
    fn the_window_icon_is_a_valid_64px_image() {
        assert!(super::window_icon().is_some());
    }
}
