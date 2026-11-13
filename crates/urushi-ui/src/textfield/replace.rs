//! Edits made by a program rather than by typing, such as a formatting control that
//! wraps the selection in Markdown marks: replace any range and choose the selection
//! afterwards, as one undo step.

use std::ops::Range;

use super::{TextField, clean};

impl TextField {
    /// Replaces `range` with `insert` (cleaned as typing is) and then selects from
    /// `selection.0` (the anchor) to `selection.1` (the caret), in the new text. One undo
    /// step. Offsets are clamped to the text and snapped back to character boundaries.
    /// Does nothing on a secret field, whose text is never edited this way.
    pub fn replace(&mut self, range: Range<usize>, insert: &str, selection: (usize, usize)) {
        if self.secret {
            return;
        }
        let start = self.boundary(range.start);
        let end = self.boundary(range.end.max(start));
        let insert = clean(insert, self.multiline);
        if self.text[start..end] != insert {
            self.checkpoint();
            self.text.replace_range(start..end, &insert);
        }
        self.select(selection.0, selection.1);
    }

    /// Selects from `anchor` to `caret` (equal: just the caret), clamped to the text
    /// and snapped back to character boundaries.
    pub fn select(&mut self, anchor: usize, caret: usize) {
        let anchor = self.boundary(anchor);
        self.caret = self.boundary(caret);
        self.anchor = (anchor != self.caret).then_some(anchor);
    }

    /// `at` clamped to the text and moved back to a character boundary.
    fn boundary(&self, at: usize) -> usize {
        let mut at = at.min(self.text.len());
        while !self.text.is_char_boundary(at) {
            at -= 1;
        }
        at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replacement_selects_what_it_was_told_and_undoes_in_one_step() {
        let mut f = TextField::multiline("make bold");
        f.replace(5..9, "**bold**", (7, 11));
        assert_eq!(f.text(), "make **bold**");
        assert_eq!(f.selection(), Some((7, 11)));
        assert_eq!(f.caret(), 11);
        f.undo();
        assert_eq!(f.text(), "make bold");
        f.redo();
        assert_eq!(f.text(), "make **bold**");
    }

    #[test]
    fn offsets_are_clamped_and_snapped_to_characters() {
        let mut f = TextField::multiline("é");
        f.select(1, 99);
        assert_eq!((f.selection(), f.caret()), (Some((0, 2)), 2));
        f.select(1, 1);
        assert_eq!((f.selection(), f.caret()), (None, 0));
        // A backwards range is taken as empty at its start.
        let (start, end) = (2, 1);
        f.replace(start..end, "x", (3, 3));
        assert_eq!(f.text(), "éx");
        assert_eq!(f.caret(), 3);
    }

    #[test]
    fn a_secret_field_is_never_replaced_into() {
        let mut f = TextField::secret("hunter2");
        f.replace(0..7, "x", (0, 0));
        assert_eq!(f.shown().0, "•••••••");
        assert_eq!(f.caret(), 7);
    }
}
