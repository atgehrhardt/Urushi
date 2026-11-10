//! Subtasks written in the task editor and not yet created (ADR 0025). Each is the
//! whole editor it was written in, so opening it again shows it as it was left, Jira
//! fields too; Save creates it with everything it has.

use urushi_client::v1;
use urushi_ui::TextField;

use crate::board::TaskLabels;
use crate::issue::IssueEditor;
use crate::state::Dialog;

/// Protocol minor version from which a new subtask is created with its details
/// (notes, color, tags, kind, Jira fields), not only its title.
pub const DETAILS_SINCE_MINOR: u32 = 14;

/// A subtask to create on Save: the editor it was written in (a `Dialog::Task` with no
/// task).
#[derive(Clone, Debug, PartialEq)]
pub struct SubtaskDraft(Box<Dialog>);

impl SubtaskDraft {
    /// A subtask with only a title (typed in the "Add a subtask" field).
    pub fn titled(board: &str, column: &str, title: &str) -> Self {
        let mut draft = Self::blank(board, column);
        if let Dialog::Task { title: t, .. } = draft.0.as_mut() {
            *t = TextField::new(title.to_owned());
        }
        draft
    }

    /// An empty subtask of a task on `board`, of the kind subtasks there are (a Jira
    /// board's are Sub-task issues).
    pub fn blank(board: &str, column: &str) -> Self {
        Self(Box::new(Dialog::Task {
            board: board.to_owned(),
            column: column.to_owned(),
            task: None,
            title: TextField::default(),
            notes: TextField::multiline(""),
            subtasks: Box::default(),
            labels: Box::default(),
            nav: Box::default(),
            issue: Box::default(),
        }))
    }

    /// Keeps an editor as a draft; `None` for anything but a new task's editor.
    pub fn from_dialog(dialog: Dialog) -> Option<Self> {
        matches!(dialog, Dialog::Task { task: None, .. }).then(|| Self(Box::new(dialog)))
    }

    /// The editor, to open it again.
    pub fn into_dialog(self) -> Dialog {
        *self.0
    }

    fn parts(&self) -> Option<(&TextField, &TextField, &TaskLabels, &IssueEditor)> {
        match self.0.as_ref() {
            Dialog::Task {
                title,
                notes,
                labels,
                issue,
                ..
            } => Some((title, notes, labels, issue)),
            _ => None,
        }
    }

    /// Its title, trimmed.
    pub fn title(&self) -> &str {
        self.parts().map_or("", |(t, ..)| t.text().trim())
    }

    /// Whether anything besides the title was set (it shows as "with details").
    pub fn has_details(&self) -> bool {
        self.parts().is_some_and(|(_, notes, labels, issue)| {
            !notes.text().trim().is_empty()
                || labels.color != 0
                || (labels.kind != 0 && labels.kind != i32::from(v1::TaskKind::Subtask))
                || !labels.with_typed().is_empty()
                || !issue.edits().is_empty()
        })
    }

    /// Its notes, color, tags, kind and Jira fields, as the wire takes them.
    pub fn request(&self) -> v1::NewSubtask {
        let Some((title, notes, labels, issue)) = self.parts() else {
            return v1::NewSubtask::default();
        };
        v1::NewSubtask {
            title: title.text().trim().to_owned(),
            notes: notes.text().to_owned(),
            color: labels.color,
            tags: labels.with_typed(),
            kind: labels.kind,
            jira_fields: issue.edits(),
        }
    }

    /// The editor's Jira side, for checking required fields.
    pub fn issue(&self) -> Option<&IssueEditor> {
        self.parts().map(|(.., issue)| issue)
    }
}
