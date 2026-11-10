//! Moving around in the task editor: opening a subtask (or any task) from the editor,
//! **Back** to the task it was opened from, and asking before unsaved edits are lost.
//!
//! The editor keeps the editors it came from ([`EditorNav::back`]) with their unsaved
//! edits, so going down to a subtask and back up loses nothing, and nothing is saved
//! until **Save**, which saves every one of them.

use crate::msg::{Effect, Followup};
use crate::state::{AppState, Dialog, EditorNav, Field};

/// The task an editor shows (`None` for a new task's editor, or another dialog).
pub(super) fn editor_task(dialog: &Dialog) -> Option<&str> {
    match dialog {
        Dialog::Task { task, .. } => task.as_deref(),
        _ => None,
    }
}

fn nav_mut(dialog: &mut Dialog) -> Option<&mut EditorNav> {
    match dialog {
        Dialog::Task { nav, .. } => Some(nav),
        _ => None,
    }
}

impl AppState {
    /// Opens the editor on an existing task or subtask. From the editor of another
    /// task, that editor is kept (unsaved edits too) for **Back**; a task already
    /// opened on the way here is returned to as it was left.
    pub(super) fn edit_task(&mut self, id: &str, fx: &mut Vec<Effect>) {
        let Some(found) = self.workspace.find_task(id) else {
            return;
        };
        let mut next = super::tasks::editor_for(&found);
        if let Some(mut current) = self.dialog.take_if(|d| editor_task(d).is_some()) {
            let mut back = nav_mut(&mut current)
                .map(|n| std::mem::take(&mut n.back))
                .unwrap_or_default();
            let on_the_way = back.iter().position(|d| editor_task(d) == Some(id));
            if let Some(mut target) = on_the_way.and_then(|i| back.split_off(i).into_iter().next())
            {
                // An editor on the way here: back to it (those opened from it go).
                if let Some(nav) = nav_mut(&mut target) {
                    nav.back = back;
                }
                next = target;
            } else if editor_task(&current) == Some(id) {
                if let Some(nav) = nav_mut(&mut current) {
                    nav.back = back;
                }
                next = current;
            } else {
                back.push(current);
                if let Some(nav) = nav_mut(&mut next) {
                    nav.back = back;
                }
            }
        }
        if let Some(nav) = nav_mut(&mut next) {
            nav.confirm_discard = false;
        }
        self.open(next, Some(Field::TaskTitle));
        self.request_issue(fx);
    }

    /// **Back**: the editor this one was opened from, as it was left.
    pub(super) fn editor_back(&mut self) {
        let Some(Dialog::Task { nav, .. }) = &mut self.dialog else {
            return;
        };
        let mut back = std::mem::take(&mut nav.back);
        let Some(mut previous) = back.pop() else {
            return;
        };
        if let Some(nav) = nav_mut(&mut previous) {
            nav.back = back;
            nav.confirm_discard = false;
        }
        self.open(previous, Some(Field::TaskTitle));
    }

    /// The requests that save the editor on screen and every editor it was opened
    /// from; the last one carries `Followup::CloseDialog`. The editors on the way are
    /// marked saved (their draft subtasks go), so saving again can't add them twice.
    pub(super) fn editor_changes(&mut self) -> Vec<Effect> {
        let Some(current) = self.dialog.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Dialog::Task { nav, .. } = current {
            for earlier in &nav.back {
                out.extend(self.task_changes(earlier, Followup::None));
            }
        }
        out.extend(self.task_changes(current, Followup::CloseDialog));
        if let Some(Dialog::Task { nav, .. }) = &mut self.dialog {
            for earlier in &mut nav.back {
                if let Dialog::Task {
                    subtasks, issue, ..
                } = earlier
                {
                    subtasks.drafts.clear();
                    subtasks.new.clear();
                    issue.saved();
                }
            }
        }
        out
    }

    /// Whether the task editor on screen, or one it was opened from, has edits that
    /// Save would send.
    pub(crate) fn editor_dirty(&self) -> bool {
        let Some(dialog @ Dialog::Task { nav, .. }) = &self.dialog else {
            return false;
        };
        std::iter::once(dialog)
            .chain(nav.back.iter())
            .any(|d| self.editor_has_edits(d))
    }

    fn editor_has_edits(&self, dialog: &Dialog) -> bool {
        match dialog {
            Dialog::Task {
                task: None,
                title,
                notes,
                subtasks,
                labels,
                ..
            } => {
                !title.text().trim().is_empty()
                    || !notes.text().trim().is_empty()
                    || !subtasks.drafts.is_empty()
                    || !subtasks.new.text().trim().is_empty()
                    || !labels.with_typed().is_empty()
            }
            Dialog::Task { .. } => !self.task_changes(dialog, Followup::None).is_empty(),
            _ => false,
        }
    }

    /// Closing the task editor: with unsaved edits it asks first (a second close, e.g.
    /// Escape, keeps editing). Returns whether the close was held back.
    pub(super) fn hold_editor_close(&mut self) -> bool {
        let dirty = self.editor_dirty();
        let Some(Dialog::Task { nav, .. }) = &mut self.dialog else {
            return false;
        };
        if nav.confirm_discard {
            nav.confirm_discard = false;
            return true;
        }
        if dirty {
            nav.confirm_discard = true;
            return true;
        }
        false
    }

    /// "Keep editing" on the discard question.
    pub(super) fn keep_editing(&mut self) {
        if let Some(Dialog::Task { nav, .. }) = &mut self.dialog {
            nav.confirm_discard = false;
        }
    }

    /// "Discard" on the discard question: close without saving anything.
    pub(super) fn discard_editor(&mut self) {
        if matches!(self.dialog, Some(Dialog::Task { .. })) {
            self.dialog = None;
            self.focus = None;
        }
    }
}
