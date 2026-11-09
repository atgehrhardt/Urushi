//! Bringing things in: adding projects (folder picker, drops), files dropped on a
//! terminal, and the new-thread form's branch name that follows its title.

use urushi_client::v1::{self, request::Body};
use urushi_core::Slug;

use super::{input, request};
use crate::msg::{Effect, Followup};
use crate::state::{AppState, Dialog, Field};

impl AppState {
    /// Opens the add-repo dialog. A folder picker only reaches this machine's files; for
    /// a remote daemon the path is typed instead.
    pub(super) fn open_add_repo(&mut self) {
        let focus = (!self.can_pick_folders()).then_some(Field::ProjectPath);
        self.open(
            Dialog::AddProject {
                path: urushi_ui::TextField::default(),
            },
            focus,
        );
    }

    /// Adds the repository containing `path`, closing the add-project dialog when done.
    pub(super) fn add_project(path: &str, fx: &mut Vec<Effect>) {
        let body = Body::AddProject(v1::AddProjectRequest {
            path: path.trim().to_owned(),
            name: String::new(),
        });
        fx.push(request(body, Followup::CloseDialog));
    }

    /// A file or folder dropped on the window. A file dropped on an open task's editor is
    /// attached to the task. Folders become projects; other files (say, a screenshot for
    /// an agent) are pasted into the focused terminal as a shell-escaped path, the way
    /// other terminals do it.
    pub(super) fn file_dropped(
        &mut self,
        path: &str,
        is_dir: bool,
        over_terminal: bool,
        fx: &mut Vec<Effect>,
    ) {
        if self.drop_onto_task(path, is_dir, fx) {
            return;
        }
        match &self.dialog {
            Some(Dialog::AddProject { .. }) if is_dir => return Self::add_project(path, fx),
            Some(_) => return,
            None => {}
        }
        let pane = self.focused_pane();
        match pane {
            Some(pane) if over_terminal || !is_dir => {
                let text = format!("{} ", escape_path(path));
                fx.push(input(pane, v1::input::Kind::Paste(text)));
            }
            _ if is_dir => Self::add_project(path, fx),
            _ => {}
        }
    }

    /// Keeps the new-thread branch name in step with the title, until the user edits the
    /// branch themselves. Clearing the branch field makes it follow the title again.
    pub(super) fn after_field_edit(&mut self) {
        let prefix = self
            .settings
            .as_ref()
            .map_or_else(|| "urushi/".to_owned(), |s| s.branch_prefix.clone());
        let focus = self.focus;
        let Some(Dialog::NewThread {
            title,
            branch,
            branch_edited,
            ..
        }) = &mut self.dialog
        else {
            return;
        };
        match focus {
            Some(Field::ThreadBranch) => *branch_edited = !branch.text().is_empty(),
            Some(Field::ThreadTitle) if !*branch_edited => {
                branch.set(suggested_branch(&prefix, title.text()));
            }
            _ => {}
        }
    }
}

/// The branch suggested for a title: the prefix, then the title in lower case with
/// spaces and punctuation as `-`. Empty while the title is.
pub(crate) fn suggested_branch(prefix: &str, title: &str) -> String {
    if title.trim().is_empty() {
        String::new()
    } else {
        format!("{prefix}{}", Slug::from_title(title))
    }
}

/// Backslash-escapes characters a shell would interpret, like macOS Terminal and iTerm2
/// do for dropped files. Agent CLIs (e.g. Claude Code) recognise such paths and attach
/// the image.
fn escape_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_whitespace() || "\\'\"`$&|;<>()[]{}*?!#~^,=".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggested_branch_is_lowercase_with_dashes() {
        assert_eq!(suggested_branch("urushi/", "Primary"), "urushi/primary");
        assert_eq!(suggested_branch("", "Fix Login Bug"), "fix-login-bug");
        assert_eq!(suggested_branch("urushi/", "  "), "");
    }

    #[test]
    fn dropped_paths_are_shell_escaped() {
        assert_eq!(
            escape_path("/Users/me/Desktop/Screen Shot (1).png"),
            r"/Users/me/Desktop/Screen\ Shot\ \(1\).png"
        );
        assert_eq!(escape_path("/tmp/plain.png"), "/tmp/plain.png");
    }
}
