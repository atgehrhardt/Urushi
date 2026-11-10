//! Secret fields, for passwords and API tokens: their text is drawn as bullets, screen
//! readers hear only bullets, and copy and cut give nothing (the text is still edited,
//! pasted into and read by the app as usual).

use super::TextField;

/// What a secret field shows for each character.
const BULLET: char = '•';

impl TextField {
    /// A single-line secret field containing `text`, with the caret at the end.
    pub fn secret(text: impl Into<String>) -> Self {
        let mut field = Self::new(text);
        field.secret = true;
        field
    }

    /// Whether the field hides its text.
    pub fn is_secret(&self) -> bool {
        self.secret
    }

    /// What to draw: the text (bullets for a secret field), and the caret and selection
    /// as byte offsets into it.
    pub fn shown(&self) -> (String, usize, Option<(usize, usize)>) {
        if !self.secret {
            return (self.text.clone(), self.caret, self.selection());
        }
        let at = |offset: usize| self.text[..offset].chars().count() * BULLET.len_utf8();
        let shown = BULLET.to_string().repeat(self.text.chars().count());
        (
            shown,
            at(self.caret),
            self.selection().map(|(a, b)| (at(a), at(b))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_show_bullets_and_never_give_their_text_out() {
        let mut f = TextField::secret("tökén");
        assert!(f.is_secret());
        f.select_all();
        let (shown, caret, selection) = f.shown();
        assert_eq!(shown, "•••••");
        assert_eq!(caret, shown.len());
        assert_eq!(selection, Some((0, shown.len())));
        assert_eq!(f.selected_text(), "");
        assert_eq!(f.cut(), "", "cut deletes but gives nothing");
        assert_eq!(f.text(), "");
        f.set("new");
        assert!(f.is_secret(), "setting the text keeps it secret");
        assert_eq!(f.text(), "new");
    }

    #[test]
    fn ordinary_fields_show_their_text() {
        let f = TextField::new("abc");
        assert_eq!(f.shown(), ("abc".to_owned(), 3, None));
    }
}
