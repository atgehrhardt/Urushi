//! Editing dialog text fields: typing, the editing shortcuts (select all, cut, copy,
//! paste, undo, word and line moves, Shift to select), and the mouse (click to place
//! the caret, drag or Shift-click to select, double-click for a word, triple-click for
//! everything, right-click for a Cut/Copy/Paste menu).
//!
//! In a multiline field (task notes) Enter starts a new line, and Cmd+Enter (Apple) /
//! Ctrl+Enter (elsewhere) submits the dialog, as it does from any field.
//!
//! The shortcut modifier is Cmd on Apple platforms and Ctrl elsewhere; a dialog has no
//! terminal behind it, so plain Ctrl is free there. Word moves use Option on Apple
//! platforms and Ctrl elsewhere.

use urushi_ui::TextField;

use crate::keys::{KeyPress, Named};
use crate::msg::{Effect, Msg};
use crate::state::{AppState, Dialog, Field, FieldCommand, ThreadTarget};

const APPLE: bool = cfg!(any(target_os = "macos", target_os = "ios"));

/// The text field `focus` names in `dialog`, shared or mutable.
macro_rules! focused_field {
    ($dialog:expr, $focus:expr, $($r:tt)+) => {
        match ($dialog, $focus) {
            (Some(Dialog::AddProject { path }), Field::ProjectPath) => Some(path),
            (Some(Dialog::NewThread { title, .. }), Field::ThreadTitle)
            | (Some(Dialog::Task { title, .. }), Field::TaskTitle) => Some(title),
            (Some(Dialog::NewThread { branch, .. }), Field::ThreadBranch) => Some(branch),
            (Some(Dialog::Prompt { field, .. }), Field::Prompt) => Some(field),
            (Some(Dialog::Settings { prefix, .. }), Field::BranchPrefix) => Some(prefix),
            (Some(Dialog::Settings { shell, .. }), Field::DefaultShell) => Some(shell),
            (Some(Dialog::Settings { message, .. }), Field::TaskMessage) => Some(message),
            (Some(Dialog::Settings { look, .. }), Field::LookColor(i)) => {
                ($($r)+ look.hex).into_iter().nth(usize::from(i))
            }
            (Some(Dialog::Connect { host }), Field::Host) => Some(host),
            (Some(Dialog::Task { notes, .. }), Field::TaskNotes) => Some(notes),
            (Some(Dialog::Task { subtasks, .. }), Field::NewSubtask) => Some($($r)+ subtasks.new),
            (Some(Dialog::Task { labels, .. }), Field::TaskTag) => Some($($r)+ labels.input),
            (Some(Dialog::Task { issue, .. }), Field::JiraSearch) => Some($($r)+ issue.search),
            (Some(Dialog::Task { issue, .. }), Field::JiraComment) => Some($($r)+ issue.comment),
            (Some(Dialog::Task { issue, .. }), Field::JiraWork) => Some($($r)+ issue.work),
            (Some(Dialog::Task { issue, .. }), Field::JiraCommentEdit) => {
                match $($r)+ issue.editing {
                    Some((_, text)) => Some(text),
                    None => None,
                }
            }
            (Some(Dialog::NestTask { query, .. }), Field::NestQuery)
            | (Some(Dialog::ImportJira { query, .. }), Field::JiraQuery) => Some(query),
            (Some(Dialog::ConnectJira { site, .. }), Field::JiraSite) => Some(site),
            (Some(Dialog::ConnectJira { email, .. }), Field::JiraEmail) => Some(email),
            (Some(Dialog::ConnectJira { token, .. }), Field::JiraToken) => Some(token),
            (Some(Dialog::QuickFilter(e)), Field::QuickFilterName) => Some($($r)+ e.name),
            (Some(Dialog::QuickFilter(e)), Field::QuickFilterJql) => match $($r)+ e.jql {
                Some(jql) => Some(jql),
                None => None,
            },
            _ => None,
        }
    };
}

impl AppState {
    /// The focused text field, if a dialog (or the board's search) has one.
    pub(crate) fn field(&self) -> Option<&TextField> {
        match (self.focus?, &self.dialog) {
            (Field::BoardSearch, None) => Some(&self.board.filter.search),
            (Field::JiraText(i), Some(Dialog::Task { issue, .. })) => issue.text_at(i),
            (focus, _) => focused_field!(&self.dialog, focus, &),
        }
    }

    /// The focused text field, to edit.
    pub(super) fn field_mut(&mut self) -> Option<&mut TextField> {
        let focus = self.focus?;
        if focus == Field::BoardSearch && self.dialog.is_none() {
            return Some(&mut self.board.filter.search);
        }
        if let Field::JiraText(i) = focus {
            return match &mut self.dialog {
                Some(Dialog::Task { issue, .. }) => issue.text_at_mut(i),
                _ => None,
            };
        }
        focused_field!(&mut self.dialog, focus, &mut)
    }

    /// The fields Tab cycles through, in order.
    pub(super) fn dialog_fields(&self) -> Vec<Field> {
        match &self.dialog {
            Some(Dialog::AddProject { .. }) if self.can_pick_folders() => Vec::new(),
            Some(Dialog::AddProject { .. }) => vec![Field::ProjectPath],
            Some(Dialog::NewThread {
                target: ThreadTarget::Existing(_),
                ..
            }) => Vec::new(),
            Some(Dialog::NewThread { .. }) => vec![Field::ThreadTitle, Field::ThreadBranch],
            Some(Dialog::Settings { look, .. }) => {
                // In the order they're shown: the task message comes first.
                let mut fields = Vec::new();
                if self.task_agent().is_some() && self.task_message().0 {
                    fields.push(Field::TaskMessage);
                }
                fields.extend([Field::BranchPrefix, Field::DefaultShell]);
                if look.custom {
                    fields.extend((0..5).map(Field::LookColor));
                }
                fields
            }
            Some(Dialog::Connect { .. }) => vec![Field::Host],
            Some(Dialog::Prompt { .. }) => vec![Field::Prompt],
            Some(Dialog::NestTask { .. }) => vec![Field::NestQuery],
            Some(Dialog::ConnectJira { .. }) => {
                vec![Field::JiraSite, Field::JiraEmail, Field::JiraToken]
            }
            Some(Dialog::ImportJira { .. }) => vec![Field::JiraQuery],
            Some(Dialog::QuickFilter(e)) if e.jql.is_some() => {
                vec![Field::QuickFilterName, Field::QuickFilterJql]
            }
            Some(Dialog::QuickFilter(_)) => vec![Field::QuickFilterName],
            Some(Dialog::Task { .. }) => {
                let mut fields = vec![Field::TaskTitle, Field::TaskNotes];
                if self.daemon_supports_board_extras() {
                    fields.push(Field::TaskTag);
                }
                // A new subtask's editor doesn't list subtasks of its own.
                if self.daemon_supports_subtasks() && !self.editing_subtask_draft() {
                    fields.push(Field::NewSubtask);
                }
                fields
            }
            _ => Vec::new(),
        }
    }

    /// A key press while a dialog is open.
    pub(super) fn dialog_key(&mut self, key: &KeyPress, fx: &mut Vec<Effect>) {
        let m = key.mods;
        let command = if APPLE { m.logo } else { m.ctrl };
        if command || m.logo {
            self.command_key(key, fx);
            return;
        }
        match key.named {
            Some(Named::Escape) if self.dialog.is_some() => self.close_dialog(),
            Some(Named::Escape) => {}
            Some(Named::Enter) => {
                if self.field_mut().is_some_and(TextField::newline) {
                    self.after_field_edit();
                } else if !self.task_editor_enter(fx) {
                    self.submit(fx);
                }
            }
            Some(Named::Tab) => {
                let fields = self.dialog_fields();
                if !fields.is_empty() {
                    let len = fields.len();
                    let step = if m.shift { len - 1 } else { 1 };
                    let i = self
                        .focus
                        .and_then(|f| fields.iter().position(|x| *x == f))
                        .map_or(0, |i| (i + step) % len);
                    self.focus = Some(fields[i]);
                }
            }
            Some(named) => {
                // Option (Apple) / Ctrl (elsewhere) moves and deletes by word.
                let word = if APPLE { m.alt } else { m.ctrl };
                if let Some(field) = self.field_mut() {
                    let select = m.shift;
                    match named {
                        Named::Backspace if word => field.delete_word_back(),
                        Named::Backspace => field.backspace(),
                        Named::Delete => field.delete(),
                        Named::Left if word => field.word_left(select),
                        Named::Left => field.left(select),
                        Named::Right if word => field.word_right(select),
                        Named::Right => field.right(select),
                        Named::Home => field.line_start(select),
                        Named::End => field.line_end(select),
                        Named::Up => field.up(select),
                        Named::Down => field.down(select),
                        Named::Space => field.insert(" "),
                        _ => {}
                    }
                    self.after_field_edit();
                }
            }
            None if APPLE && m.ctrl => self.emacs_key(key),
            None => {
                if let Some(text) = key.text.clone().or_else(|| key.character.clone())
                    && let Some(field) = self.field_mut()
                {
                    field.insert(&text);
                    self.after_field_edit();
                }
            }
        }
    }

    /// Cmd (Apple) / Ctrl (elsewhere) plus a key.
    fn command_key(&mut self, key: &KeyPress, fx: &mut Vec<Effect>) {
        let select = key.mods.shift;
        if key.named == Some(Named::Enter) {
            self.submit(fx);
            return;
        }
        if let Some(named) = key.named {
            if let Some(field) = self.field_mut() {
                match named {
                    // Cmd+arrows go to the ends of the line on Apple platforms…
                    Named::Left if APPLE => field.line_start(select),
                    Named::Right if APPLE => field.line_end(select),
                    Named::Backspace if APPLE => field.delete_to_start(),
                    // …Ctrl+arrows move by word elsewhere.
                    Named::Left => field.word_left(select),
                    Named::Right => field.word_right(select),
                    Named::Backspace => field.delete_word_back(),
                    Named::Up | Named::Home => field.home(select),
                    Named::Down | Named::End => field.end(select),
                    _ => {}
                }
                self.after_field_edit();
            }
            return;
        }
        let command = match key.character.as_deref().map(str::to_lowercase).as_deref() {
            Some("a") => FieldCommand::SelectAll,
            Some("c") => FieldCommand::Copy,
            Some("x") => FieldCommand::Cut,
            Some("v") => FieldCommand::Paste,
            Some("z") if select => FieldCommand::Redo,
            Some("z") => FieldCommand::Undo,
            Some("y") if !APPLE => FieldCommand::Redo,
            Some("w") => {
                self.close_dialog();
                return;
            }
            _ => return,
        };
        self.edit_field(command, fx);
    }

    /// The Emacs-style Ctrl keys every macOS text field understands.
    fn emacs_key(&mut self, key: &KeyPress) {
        let Some(field) = self.field_mut() else {
            return;
        };
        match key.character.as_deref() {
            Some("a") => field.line_start(key.mods.shift),
            Some("e") => field.line_end(key.mods.shift),
            Some("p") => field.up(key.mods.shift),
            Some("n") => field.down(key.mods.shift),
            Some("b") => field.left(key.mods.shift),
            Some("f") => field.right(key.mods.shift),
            Some("h") => field.backspace(),
            Some("d") => field.delete(),
            _ => return,
        }
        self.after_field_edit();
    }

    /// Cut, copy, paste, select all, undo or redo on the focused field.
    pub(super) fn edit_field(&mut self, command: FieldCommand, fx: &mut Vec<Effect>) {
        self.menu = None;
        let Some(field) = self.field_mut() else {
            return;
        };
        match command {
            FieldCommand::Copy => {
                let selected = field.selected_text();
                if !selected.is_empty() {
                    fx.push(Effect::SetClipboard(selected.to_owned()));
                }
                return;
            }
            FieldCommand::Cut => {
                let taken = field.cut();
                if !taken.is_empty() {
                    fx.push(Effect::SetClipboard(taken));
                }
            }
            // The text arrives as `Msg::Paste`, which inserts it.
            FieldCommand::Paste => fx.push(Effect::RequestPaste),
            FieldCommand::SelectAll => field.select_all(),
            FieldCommand::Undo => {
                field.undo();
            }
            FieldCommand::Redo => {
                field.redo();
            }
        }
        self.after_field_edit();
    }

    /// A press inside the focused field (the runner focused it first).
    pub(super) fn field_press(&mut self, caret: usize, extend: bool, clicks: u8) {
        let Some(field) = self.field_mut() else {
            return;
        };
        match clicks {
            0 | 1 => field.place(caret, extend),
            2 => field.select_word(caret),
            _ => field.select_all(),
        }
    }

    /// Right-click on a field: focus it and open its Cut/Copy/Paste menu.
    pub(super) fn open_field_menu(&mut self, field: Field) -> bool {
        if self.dialog.is_none() {
            return false;
        }
        self.focus = Some(field);
        self.menu = Some(crate::organize::ContextMenu {
            target: crate::organize::MenuTarget::Field(field),
            page: crate::organize::MenuPage::Main,
            at: self.last_press.unwrap_or_default(),
        });
        true
    }
}

impl Msg {
    /// Opens the Cut/Copy/Paste menu for a dialog field.
    pub(crate) fn field_menu(field: Field) -> Self {
        Self::OpenMenu(crate::organize::MenuTarget::Field(field))
    }
}
