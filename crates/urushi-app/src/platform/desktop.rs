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
