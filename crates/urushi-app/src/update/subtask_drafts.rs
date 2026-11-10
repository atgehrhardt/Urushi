//! Writing a new subtask in an editor of its own (ADR 0025).
//!
//! **New subtask…** in the task editor opens the editor for a new subtask (title,
//! notes, kind, color, tags, and on a Jira board the sub-task's own fields), stacked on
//! the editor it came from like an existing subtask opened there. **Back** (or Done)
//! keeps it in that editor's subtask list as a draft, which opens again as it was left;
//! **Save** in either saves everything, creating the subtask with all its details.

use urushi_client::v1;

use super::task_nav::nav_mut;
use crate::drafts::{DETAILS_SINCE_MINOR, SubtaskDraft};
use crate::msg::Effect;
use crate::state::{AppState, Dialog, Field};

impl AppState {
    /// Whether the daemon creates a subtask with its details, not only its title.
    pub(crate) fn daemon_supports_subtask_details(&self) -> bool {
        self.daemon_minor >= DETAILS_SINCE_MINOR
    }

    /// Whether the task editor on screen writes a new subtask of the one before it.
    pub(crate) fn editing_subtask_draft(&self) -> bool {
        matches!(&self.dialog, Some(Dialog::Task { nav, .. }) if nav.draft_of_previous.is_some())
    }

    /// **New subtask…**: an editor for a new subtask of the task on screen, titled with
    /// what is typed in its "Add a subtask" field.
    pub(super) fn new_subtask(&mut self, fx: &mut Vec<Effect>) {
        let Some(Dialog::Task {
            board,
            column,
            subtasks,
            ..
        }) = &mut self.dialog
        else {
            return;
        };
        let draft = SubtaskDraft::titled(board, column, subtasks.new.text().trim());
        subtasks.new.clear();
        let at = subtasks.drafts.len();
        self.open_draft(draft, at, fx);
    }

    /// Opens draft subtask `i` of the editor on screen, as it was left.
    pub(super) fn open_subtask_draft(&mut self, i: usize, fx: &mut Vec<Effect>) {
        let Some(Dialog::Task { subtasks, .. }) = &mut self.dialog else {
            return;
        };
        if i >= subtasks.drafts.len() {
            return;
        }
        let draft = subtasks.drafts.remove(i);
        self.open_draft(draft, i, fx);
    }

    fn open_draft(&mut self, draft: SubtaskDraft, at: usize, fx: &mut Vec<Effect>) {
        let Some(mut parent) = self.dialog.take() else {
            return;
        };
        let mut back = nav_mut(&mut parent)
            .map(|n| std::mem::take(&mut n.back))
            .unwrap_or_default();
        back.push(parent);
        let mut editor = draft.into_dialog();
        let mut needs_fields = false;
        if let Dialog::Task {
            board,
            nav,
            labels,
            issue,
            ..
        } = &mut editor
        {
            nav.back = back;
            nav.draft_of_previous = Some(at);
            nav.confirm_discard = false;
            // On a Jira board every subtask is a Sub-task issue, with that type's fields.
            if self
                .workspace
                .board(board)
                .is_some_and(|b| b.jira.is_some())
            {
                labels.kind = v1::TaskKind::Subtask.into();
                needs_fields = issue.details.is_none();
            }
        }
        self.open(editor, Some(Field::TaskTitle));
        if needs_fields {
            self.request_create_fields(fx);
        }
    }

    /// Puts the draft-subtask editor on screen back into the editor it was opened from,
    /// at its place in the subtask list (left out when nothing was written in it), and
    /// shows that editor. Returns whether the editor on screen was a draft.
    pub(super) fn fold_draft(&mut self) -> bool {
        let Some(at) = (match &self.dialog {
            Some(Dialog::Task { nav, .. }) => nav.draft_of_previous,
            _ => None,
        }) else {
            return false;
        };
        let Some(mut current) = self.dialog.take() else {
            return false;
        };
        let mut back = Vec::new();
        if let Some(nav) = nav_mut(&mut current) {
            back = std::mem::take(&mut nav.back);
            nav.draft_of_previous = None;
            nav.confirm_discard = false;
        }
        let Some(mut parent) = back.pop() else {
            return false;
        };
        let draft =
            SubtaskDraft::from_dialog(current).filter(|d| !d.title().is_empty() || d.has_details());
        if let Dialog::Task { subtasks, nav, .. } = &mut parent {
            if let Some(draft) = draft {
                subtasks.drafts.insert(at.min(subtasks.drafts.len()), draft);
            }
            nav.back = back;
            nav.confirm_discard = false;
        }
        self.open(parent, None);
        true
    }

    /// **Remove subtask** in a draft subtask's editor: forget it and go back.
    pub(super) fn remove_subtask_draft(&mut self) {
        if !self.editing_subtask_draft() {
            return;
        }
        if let Some(Dialog::Task {
            title,
            notes,
            labels,
            issue,
            ..
        }) = &mut self.dialog
        {
            *title = urushi_ui::TextField::default();
            *notes = urushi_ui::TextField::multiline("");
            **labels = crate::board::TaskLabels::default();
            issue.saved();
        }
        self.fold_draft();
    }

    /// Why the draft subtasks of the editor on screen (and those it was opened from)
    /// can't be created yet: one without a title, or a Jira sub-task missing a required
    /// field.
    pub(super) fn drafts_problem(&self) -> Option<String> {
        let Some(current @ Dialog::Task { nav, .. }) = &self.dialog else {
            return None;
        };
        let drafts = std::iter::once(current)
            .chain(nav.back.iter())
            .filter_map(|d| match d {
                Dialog::Task { subtasks, .. } => Some(subtasks.drafts.iter()),
                _ => None,
            })
            .flatten();
        for draft in drafts {
            if draft.title().is_empty() {
                return Some("Give every subtask a title first.".to_owned());
            }
            let missing = draft.issue().map(Self::missing_fields).unwrap_or_default();
            if !missing.is_empty() {
                return Some(format!(
                    "Fill in {} of the subtask \u{201c}{}\u{201d} first.",
                    missing.join(", "),
                    draft.title()
                ));
            }
        }
        None
    }
}
