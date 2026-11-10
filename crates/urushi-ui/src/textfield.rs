//! Editing model for text fields: the text, the caret, an optional selection, and an
//! undo history. Pure and UTF-8 safe: the caret and the selection anchor are always on
//! character boundaries.
//!
//! A field is single-line (pasted line breaks become spaces) or multiline (it keeps
//! `\n`, Enter adds one, and Up/Down/Home/End work on lines). Multiline moves count
//! `\n`-separated lines and characters: the model knows nothing of fonts or wrapping.
//!
//! A secret field (`secret.rs`) is single-line, is drawn as bullets, and never gives
//! its text out through the selection (copy, cut).
//!
//! Every edit replaces the selection when there is one, as text fields do everywhere.
//! Moves take `extend`: with it they grow the selection (Shift+arrow), without it they
//! collapse it.

/// Most undo steps a field keeps.
const UNDO_LIMIT: usize = 100;

/// The text, caret and selection of a field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextField {
    text: String,
    caret: usize,
    /// The other end of the selection; `None` (or equal to `caret`) means none.
    anchor: Option<usize>,
    /// Earlier `(text, caret)` states, oldest first.
    undo: Vec<(String, usize)>,
    /// States undone, for redo.
    redo: Vec<(String, usize)>,
    /// Keeps line breaks (see the module docs).
    multiline: bool,
    /// Hides its text (`secret.rs`).
    secret: bool,
}

/// How characters group into words for word-wise moves and double-click selection.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punct,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punct
    }
}

impl TextField {
    /// A single-line field containing `text`, with the caret at the end.
    pub fn new(text: impl Into<String>) -> Self {
        Self::with_mode(&text.into(), false)
    }

    /// A multiline field containing `text`, with the caret at the end.
    pub fn multiline(text: impl Into<String>) -> Self {
        Self::with_mode(&text.into(), true)
    }

    fn with_mode(text: &str, multiline: bool) -> Self {
        let text = clean(text, multiline);
        let caret = text.len();
        Self {
            text,
            caret,
            multiline,
            ..Self::default()
        }
    }

    /// Whether the field keeps line breaks.
    pub fn is_multiline(&self) -> bool {
        self.multiline
    }

    /// Current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Caret byte offset.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selected byte range, if anything is selected.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.caret).then(|| (anchor.min(self.caret), anchor.max(self.caret)))
    }

    /// The selected text (empty when nothing is selected, and always for a secret
    /// field).
    pub fn selected_text(&self) -> &str {
        if self.secret {
            return "";
        }
        self.selection().map_or("", |(a, b)| &self.text[a..b])
    }

    /// Replaces the text, with the caret at the end. Clears the undo history.
    pub fn set(&mut self, text: impl Into<String>) {
        let secret = self.secret;
        *self = Self::with_mode(&text.into(), self.multiline);
        self.secret = secret;
    }

    /// Inserts at the caret, replacing the selection. Tabs become spaces and other
    /// control characters are dropped; line breaks are kept in a multiline field and
    /// become spaces in a single-line one.
    pub fn insert(&mut self, s: &str) {
        let clean = clean(s, self.multiline);
        if clean.is_empty() && self.selection().is_none() {
            return;
        }
        self.checkpoint();
        self.delete_selected();
        self.text.insert_str(self.caret, &clean);
        self.caret += clean.len();
    }

    /// Deletes the selection, or the character before the caret.
    pub fn backspace(&mut self) {
        if self.selection().is_none() {
            match self.prev_boundary(self.caret) {
                Some(prev) => self.anchor = Some(prev),
                None => return,
            }
        }
        self.checkpoint();
        self.delete_selected();
    }

    /// Deletes the selection, or the character after the caret.
    pub fn delete(&mut self) {
        if self.selection().is_none() {
            match self.next_boundary(self.caret) {
                Some(next) => self.anchor = Some(next),
                None => return,
            }
        }
        self.checkpoint();
        self.delete_selected();
    }

    /// Deletes the selection, or back to the start of the word before the caret
    /// (Option+Backspace on macOS, Ctrl+Backspace elsewhere).
    pub fn delete_word_back(&mut self) {
        if self.selection().is_none() {
            let start = self.word_start(self.caret);
            if start == self.caret {
                return;
            }
            self.anchor = Some(start);
        }
        self.checkpoint();
        self.delete_selected();
    }

    /// Starts a new line at the caret (Enter), in a multiline field. Returns whether it
    /// did; a single-line field has no lines to start.
    pub fn newline(&mut self) -> bool {
        if self.multiline {
            self.insert("\n");
        }
        self.multiline
    }

    /// Deletes the selection, or everything before the caret on its line (Cmd+Backspace);
    /// at the start of a line, the line break before it.
    pub fn delete_to_start(&mut self) {
        if self.selection().is_none() {
            let start = self.line_start_of(self.caret);
            let to = if start < self.caret {
                start
            } else {
                match self.prev_boundary(self.caret) {
                    Some(prev) => prev,
                    None => return,
                }
            };
            self.anchor = Some(to);
        }
        self.checkpoint();
        self.delete_selected();
    }

    /// Removes the selection and returns it (Cut). Empty when nothing is selected.
    pub fn cut(&mut self) -> String {
        let taken = self.selected_text().to_owned();
        if self.selection().is_some() {
            self.checkpoint();
            self.delete_selected();
        }
        taken
    }

    /// Moves the caret one character left; without `extend`, a selection collapses to
    /// its left end instead.
    pub fn left(&mut self, extend: bool) {
        let to = match self.selection() {
            Some((start, _)) if !extend => start,
            _ => self.prev_boundary(self.caret).unwrap_or(0),
        };
        self.move_to(to, extend);
    }

    /// Moves the caret one character right; without `extend`, a selection collapses to
    /// its right end instead.
    pub fn right(&mut self, extend: bool) {
        let to = match self.selection() {
            Some((_, end)) if !extend => end,
            _ => self.next_boundary(self.caret).unwrap_or(self.text.len()),
        };
        self.move_to(to, extend);
    }

    /// Moves the caret to the start of the previous word.
    pub fn word_left(&mut self, extend: bool) {
        self.move_to(self.word_start(self.caret), extend);
    }

    /// Moves the caret to the end of the next word.
    pub fn word_right(&mut self, extend: bool) {
        self.move_to(self.word_end(self.caret), extend);
    }

    /// Moves the caret to the start of its line (the start, in a single-line field).
    pub fn line_start(&mut self, extend: bool) {
        self.move_to(self.line_start_of(self.caret), extend);
    }

    /// Moves the caret to the end of its line (the end, in a single-line field).
    pub fn line_end(&mut self, extend: bool) {
        self.move_to(self.line_end_of(self.caret), extend);
    }

    /// Moves the caret to the same column of the line above, or to the start from the
    /// first line (and in a single-line field).
    pub fn up(&mut self, extend: bool) {
        let start = self.line_start_of(self.caret);
        let to = match start.checked_sub(1) {
            Some(prev_end) => self.at_column(self.line_start_of(prev_end), self.column()),
            None => 0,
        };
        self.move_to(to, extend);
    }

    /// Moves the caret to the same column of the line below, or to the end from the last
    /// line (and in a single-line field).
    pub fn down(&mut self, extend: bool) {
        let end = self.line_end_of(self.caret);
        let to = if end < self.text.len() {
            self.at_column(end + 1, self.column())
        } else {
            self.text.len()
        };
        self.move_to(to, extend);
    }

    /// Moves the caret to the start.
    pub fn home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }

    /// Moves the caret to the end.
    pub fn end(&mut self, extend: bool) {
        self.move_to(self.text.len(), extend);
    }

    /// Puts the caret at byte offset `at` (snapped to a character boundary), e.g. where
    /// the field was clicked. With `extend` the selection runs from the anchor (or the
    /// old caret) to there.
    pub fn place(&mut self, at: usize, extend: bool) {
        let mut at = at.min(self.text.len());
        while !self.text.is_char_boundary(at) {
            at -= 1;
        }
        self.move_to(at, extend);
    }

    /// Selects everything.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    /// Selects the word (or run of spaces or punctuation) around byte offset `at`, as a
    /// double click does.
    pub fn select_word(&mut self, at: usize) {
        self.place(at, false);
        let after = self.text[self.caret..].chars().next();
        let before = self.text[..self.caret].chars().next_back();
        let Some(kind) = after.or(before).map(class) else {
            return;
        };
        let start = self.text[..self.caret]
            .char_indices()
            .rev()
            .take_while(|(_, c)| class(*c) == kind)
            .last()
            .map_or(self.caret, |(i, _)| i);
        let end = self.text[self.caret..]
            .char_indices()
            .find(|(_, c)| class(*c) != kind)
            .map_or(self.text.len(), |(i, _)| self.caret + i);
        self.anchor = Some(start);
        self.caret = end;
    }

    /// Undoes the last edit. Returns whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some((text, caret)) = self.undo.pop() else {
            return false;
        };
        let current = (std::mem::replace(&mut self.text, text), self.caret);
        self.redo.push(current);
        self.caret = caret;
        self.anchor = None;
        true
    }

    /// Redoes the last undone edit. Returns whether there was one.
    pub fn redo(&mut self) -> bool {
        let Some((text, caret)) = self.redo.pop() else {
            return false;
        };
        let current = (std::mem::replace(&mut self.text, text), self.caret);
        self.undo.push(current);
        self.caret = caret;
        self.anchor = None;
        true
    }

    /// Deletes everything.
    pub fn clear(&mut self) {
        self.set("");
    }

    /// Records the current state before an edit, so it can be undone.
    fn checkpoint(&mut self) {
        if self.undo.len() == UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push((self.text.clone(), self.caret));
        self.redo.clear();
    }

    fn delete_selected(&mut self) {
        if let Some((start, end)) = self.selection() {
            self.text.replace_range(start..end, "");
            self.caret = start;
        }
        self.anchor = None;
    }

    fn move_to(&mut self, to: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = to;
    }

    /// Where a word-wise move left from `from` stops: skip spaces, then one run of the
    /// same kind of character.
    fn word_start(&self, from: usize) -> usize {
        let mut chars = self.text[..from].char_indices().rev().peekable();
        let mut at = from;
        while let Some(&(i, c)) = chars.peek() {
            if class(c) != Class::Space {
                break;
            }
            at = i;
            chars.next();
        }
        let Some(&(_, first)) = chars.peek() else {
            return at;
        };
        let kind = class(first);
        for (i, c) in chars {
            if class(c) != kind {
                break;
            }
            at = i;
        }
        at
    }

    /// Where a word-wise move right from `from` stops.
    fn word_end(&self, from: usize) -> usize {
        let rest = &self.text[from..];
        let mut chars = rest.char_indices().peekable();
        while chars.next_if(|(_, c)| class(*c) == Class::Space).is_some() {}
        let Some(&(_, first)) = chars.peek() else {
            return self.text.len();
        };
        let kind = class(first);
        chars
            .find(|(_, c)| class(*c) != kind)
            .map_or(self.text.len(), |(i, _)| from + i)
    }

    /// Start of the line `at` is on.
    fn line_start_of(&self, at: usize) -> usize {
        self.text[..at].rfind('\n').map_or(0, |i| i + 1)
    }

    /// End of the line `at` is on (where its `\n` is, or the end of the text).
    fn line_end_of(&self, at: usize) -> usize {
        self.text[at..]
            .find('\n')
            .map_or(self.text.len(), |i| at + i)
    }

    /// How many characters the caret is from the start of its line.
    fn column(&self) -> usize {
        self.text[self.line_start_of(self.caret)..self.caret]
            .chars()
            .count()
    }

    /// The offset `column` characters into the line starting at `start`, or its end if
    /// the line is shorter.
    fn at_column(&self, start: usize, column: usize) -> usize {
        let end = self.line_end_of(start);
        self.text[start..end]
            .char_indices()
            .nth(column)
            .map_or(end, |(i, _)| start + i)
    }

    fn prev_boundary(&self, at: usize) -> Option<usize> {
        self.text[..at].char_indices().next_back().map(|(i, _)| i)
    }

    fn next_boundary(&self, at: usize) -> Option<usize> {
        self.text[at..].chars().next().map(|c| at + c.len_utf8())
    }
}

/// Text as a field keeps it: tabs become spaces and other control characters are
/// dropped. Line breaks (`\r\n`, `\r` or `\n`) become `\n` in a multiline field and
/// spaces in a single-line one.
fn clean(s: &str, multiline: bool) -> String {
    let line_break = if multiline { '\n' } else { ' ' };
    s.replace("\r\n", "\n")
        .chars()
        .filter_map(|c| match c {
            '\n' | '\r' => Some(line_break),
            '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

mod secret;

#[cfg(test)]
mod tests;
