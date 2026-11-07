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
