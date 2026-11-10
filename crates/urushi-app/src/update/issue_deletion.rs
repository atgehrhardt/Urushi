//! Deleting a Jira issue from Urushi (ADR 0033). The daemon deletes it in Jira, for
//! good and for everyone, with its sub-tasks, so the app always asks first: in the
//! editor, in place of its buttons (for the issue or one of its subtasks); from a
//! card's menu, in a dialog. Tasks of Urushi's own are deleted right away, as before.

use crate::msg::{Effect, Followup};
use crate::state::{AppState, Dialog, IssueDeletion};

impl AppState {
    /// What deleting `task` would delete, if it is a Jira issue.
    pub(crate) fn issue_deletion(&self, task: &str) -> Option<IssueDeletion> {
        let found = self.workspace.find_task(task)?;
        let t = found.task();
        let issue = t.issue()?;
        Some(IssueDeletion {
            task: task.to_owned(),
            key: issue.key.clone(),
            title: t.title().to_owned(),
            subtasks: t.subtree().len().saturating_sub(1),
        })
    }

    /// Deletes `task`, asking first if it is a Jira issue. A delete sent at once
    /// carries `followup`.
    pub(super) fn delete_task(&mut self, task: String, followup: Followup, fx: &mut Vec<Effect>) {
        let Some(about) = self.issue_deletion(&task) else {
            fx.push(super::request(self.delete_body(task), followup));
            return;
        };
        match &mut self.dialog {
            Some(Dialog::Task { nav, .. }) => {
                nav.settle();
                nav.confirm_delete = Some(about);
            }
            _ => self.open(Dialog::DeleteIssue(about), None),
        }
    }

    /// "Delete from Jira" on the question: deletes the issue it names. The editor
    /// closes when its own task goes, and stays for a subtask.
    pub(super) fn confirm_delete_issue(&mut self, fx: &mut Vec<Effect>) {
        let (task, followup) = match &mut self.dialog {
            Some(Dialog::DeleteIssue(about)) => (about.task.clone(), Followup::CloseDialog),
            Some(Dialog::Task { task, nav, .. }) => {
                let Some(about) = nav.confirm_delete.take() else {
                    return;
                };
                let own = task.as_deref() == Some(about.task.as_str());
                let followup = if own {
                    Followup::CloseDialog
                } else {
                    Followup::None
                };
                (about.task, followup)
            }
            _ => return,
        };
        fx.push(super::request(self.delete_body(task), followup));
    }
}
