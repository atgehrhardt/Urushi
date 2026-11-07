//! Editing model for single-line text fields. Pure and UTF-8 safe. The caret is always on
//! a character boundary.

/// The text and caret of a field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextField {
    text: String,
    caret: usize,
}

impl TextField {
    /// A field containing `text`, with the caret at the end.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let caret = text.len();
        Self { text, caret }
    }

    /// Current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Caret byte offset.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Replaces the text, with the caret at the end.
    pub fn set(&mut self, text: impl Into<String>) {
        *self = Self::new(text);
    }

    /// Inserts at the caret. Newlines are dropped (single line).
    pub fn insert(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        self.text.insert_str(self.caret, &clean);
        self.caret += clean.len();
    }

    /// Deletes the character before the caret.
    pub fn backspace(&mut self) {
        if let Some(prev) = self.prev_boundary() {
            self.text.replace_range(prev..self.caret, "");
            self.caret = prev;
        }
    }

    /// Deletes the character after the caret.
    pub fn delete(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.text.replace_range(self.caret..next, "");
        }
    }

    /// Moves the caret one character left.
    pub fn left(&mut self) {
        if let Some(prev) = self.prev_boundary() {
            self.caret = prev;
        }
    }

    /// Moves the caret one character right.
    pub fn right(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.caret = next;
        }
    }

    /// Moves the caret to the start.
    pub fn home(&mut self) {
        self.caret = 0;
    }

    /// Moves the caret to the end.
    pub fn end(&mut self) {
        self.caret = self.text.len();
    }

    /// Deletes everything.
    pub fn clear(&mut self) {
        self.set("");
    }

    fn prev_boundary(&self) -> Option<usize> {
        self.text[..self.caret]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
    }

    fn next_boundary(&self) -> Option<usize> {
        self.text[self.caret..]
            .chars()
            .next()
            .map(|c| self.caret + c.len_utf8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_is_utf8_safe() {
        let mut f = TextField::new("héllo");
        f.backspace();
        assert_eq!(f.text(), "héll");
        f.home();
        f.right();
        f.right();
        f.backspace();
        assert_eq!(f.text(), "hll");
        f.insert("é\n");
        assert_eq!(f.text(), "héll");
        f.end();
        f.delete();
        f.left();
        f.delete();
        assert_eq!(f.text(), "hél");
        f.clear();
        f.backspace();
        f.left();
        assert_eq!((f.text(), f.caret()), ("", 0));
    }
}
