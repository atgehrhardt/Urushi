/// Clipboard placeholder for mobile: copy and paste stay inside the app.
pub(crate) struct Clipboard(Option<String>);

impl Clipboard {
    pub(crate) fn new() -> Self {
        Self(None)
    }

    pub(crate) fn set(&mut self, text: &str) {
        self.0 = Some(text.to_owned());
    }

    pub(crate) fn get(&mut self) -> Option<String> {
        self.0.clone()
    }
}

/// Mobile apps never add local repositories.
pub(crate) fn pick_folder() -> Option<String> {
    None
}

/// Mobile has no folder picker (and no local daemon whose folders it could pick).
pub(crate) const HAS_FOLDER_PICKER: bool = false;

/// Mobile apps get their icon from the app package, not the window.
pub(crate) fn window_icon() -> Option<winit::window::Icon> {
    None
}
