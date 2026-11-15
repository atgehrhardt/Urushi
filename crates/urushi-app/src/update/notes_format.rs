//! The notes editor's formatting (UR-72, ADR 0058). The toolbar, the shortcuts and Enter
//! in a list turn into Markdown edits from `urushi_core::markdown`, applied to the field
//! as one undoable step. The notes stay plain text, so nothing here talks to the daemon
//! until Save.

use urushi_core::markdown::{self, Edit, Prefix};
use urushi_ui::TextField;

use crate::keys::KeyPress;
use crate::msg::NoteFormat;
use crate::state::{AppState, Dialog, Field};

impl AppState {
    /// The notes field, while the notes editor is open.
    fn notes_field(&mut self) -> Option<&mut TextField> {
        match &mut self.dialog {
            Some(Dialog::ThreadNotes { notes, .. }) => Some(notes),
            _ => None,
        }
    }

    /// Applies a formatting control to the notes' selection (or at the caret), and puts
    /// the focus back in the notes.
    pub(super) fn format_notes(&mut self, format: NoteFormat) {
        let Some(field) = self.notes_field() else {
            return;
        };
        let text = field.text();
        let caret = field.caret();
        let (start, end) = field.selection().unwrap_or((caret, caret));
        let sel = start..end;
        let wrap = |delim| markdown::toggle_wrap(text, sel.clone(), delim);
        let prefix = |p| markdown::toggle_prefix(text, sel.clone(), p);
        let edit = match format {
            NoteFormat::Bold => wrap("**"),
            NoteFormat::Italic => wrap("_"),
            NoteFormat::Strike => wrap("~~"),
            NoteFormat::Code => wrap("`"),
            NoteFormat::Heading => prefix(Prefix::Heading),
            NoteFormat::Bullet => prefix(Prefix::Bullet),
            NoteFormat::Numbered => prefix(Prefix::Numbered),
            NoteFormat::Task => prefix(Prefix::Task),
            NoteFormat::Link => markdown::link(text, sel.clone()),
        };
        apply(field, &edit);
        self.focus = Some(Field::ThreadNotes);
        self.after_field_edit();
    }

    /// Enter in the notes: continues a list or quote, or ends an empty item. `false`
    /// where it is an ordinary new line.
    pub(super) fn notes_enter(&mut self) -> bool {
        if self.focus != Some(Field::ThreadNotes) {
            return false;
        }
        let Some(field) = self.notes_field() else {
            return false;
        };
        if field.selection().is_some() {
            return false;
        }
        let Some(edit) = markdown::continue_list(field.text(), field.caret()) else {
            return false;
        };
        apply(field, &edit);
        self.after_field_edit();
        true
    }

    /// The notes' formatting shortcuts, with Cmd (Apple) / Ctrl (elsewhere) held:
    /// B bold, I italic, E code, K link, Shift+X strikethrough. `false` for any other
    /// key, or outside the notes.
    pub(super) fn notes_shortcut(&mut self, key: &KeyPress) -> bool {
        if self.focus != Some(Field::ThreadNotes) {
            return false;
        }
        let shift = key.mods.shift;
        let format = match key.character.as_deref().map(str::to_lowercase).as_deref() {
            Some("b") => NoteFormat::Bold,
            Some("i") => NoteFormat::Italic,
            Some("e") => NoteFormat::Code,
            Some("k") => NoteFormat::Link,
            Some("x") if shift => NoteFormat::Strike,
            _ => return false,
        };
        self.format_notes(format);
        true
    }
}

fn apply(field: &mut TextField, edit: &Edit) {
    field.replace(edit.range.clone(), &edit.insert, edit.selection);
}
