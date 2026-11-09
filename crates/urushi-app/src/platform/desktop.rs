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

/// The window and taskbar icon (Windows, Linux; macOS uses the bundle's icon instead):
/// the app icon as raw 64×64 RGBA, rendered by `scripts/render-icons.swift`.
pub(crate) fn window_icon() -> Option<winit::window::Icon> {
    const SIZE: u32 = 64;
    let rgba = include_bytes!("../../../../assets/icon/window-64.rgba");
    winit::window::Icon::from_rgba(rgba.to_vec(), SIZE, SIZE).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_window_icon_is_a_valid_64px_image() {
        assert!(super::window_icon().is_some());
    }
}
