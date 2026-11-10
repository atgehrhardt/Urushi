//! Finding tasks in the mirrored boards at any depth. On the wire a card is a
//! `v1::Task` and a subtask a `v1::Subtask` (ADR 0015); [`TaskRef`] reads either the
//! same way, so clients can show and edit both alike.

use urushi_proto::v1;

use crate::WorkspaceMirror;

/// A card or a subtask, read alike.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TaskRef<'a> {
    /// A card on the board.
    Card(&'a v1::Task),
    /// A subtask, at any depth.
    Sub(&'a v1::Subtask),
}

macro_rules! field {
    ($self:ident, $f:ident) => {
        match $self {
            TaskRef::Card(t) => &t.$f,
            TaskRef::Sub(s) => &s.$f,
        }
    };
}

impl<'a> TaskRef<'a> {
    /// Its id.
    pub fn id(self) -> &'a str {
        field!(self, id)
    }

    /// Its title.
    pub fn title(self) -> &'a str {
        field!(self, title)
    }

    /// Its notes.
    pub fn notes(self) -> &'a str {
        field!(self, notes)
    }

    /// Its color (a wire `LabelColor`; 0 is none).
    pub fn color(self) -> i32 {
        *field!(self, color)
    }

    /// Its tags.
    pub fn tags(self) -> &'a [String] {
        field!(self, tags)
    }

    /// Its subtasks.
    pub fn subtasks(self) -> &'a [v1::Subtask] {
        field!(self, subtasks)
    }

    /// Files attached to it.
    pub fn attachments(self) -> &'a [v1::Attachment] {
        field!(self, attachments)
    }

    /// The thread started for it; empty if none.
    pub fn thread_id(self) -> &'a str {
        field!(self, thread_id)
    }

    /// Its kind (a wire `TaskKind`; 0 is unspecified). Since 1.12.
    pub fn kind(self) -> i32 {
        *field!(self, kind)
    }

    /// Its pull request link, on a local board; empty if none (1.21).
    pub fn pr_url(self) -> &'a str {
        field!(self, pr_url)
    }

    /// The Jira issue it shows, on a Jira board (1.12).
    pub fn issue(self) -> Option<&'a v1::JiraIssue> {
        field!(self, issue).as_deref()
    }

    /// Checked off (a card's progress is its column, so a card never is).
    pub fn done(self) -> bool {
        match self {
            Self::Card(_) => false,
            Self::Sub(s) => s.done,
        }
    }

    /// Whether it is a card on the board.
    pub fn is_card(self) -> bool {
        matches!(self, Self::Card(_))
    }

    /// `(checked, total)` direct subtasks.
    pub fn progress(self) -> (usize, usize) {
        let subs = self.subtasks();
        (subs.iter().filter(|s| s.done).count(), subs.len())
    }

    /// The task with this id among this one and those under it.
    fn find(self, id: &str) -> Option<Vec<TaskRef<'a>>> {
        if self.id() == id {
            return Some(vec![self]);
        }
        self.subtasks().iter().find_map(|s| {
            TaskRef::Sub(s).find(id).map(|mut path| {
                path.insert(0, self);
                path
            })
        })
    }

    /// This task and every task under it, parents first.
    pub fn subtree(self) -> Vec<TaskRef<'a>> {
        let mut out = vec![self];
        for s in self.subtasks() {
            out.extend(TaskRef::Sub(s).subtree());
        }
        out
    }

    /// This task's id and the ids of every task under it.
    pub fn subtree_ids(self) -> Vec<&'a str> {
        let mut out = vec![self.id()];
        for s in self.subtasks() {
            out.extend(TaskRef::Sub(s).subtree_ids());
        }
        out
    }
}

/// A task found on a board: where it is, and the tasks above it.
#[derive(Clone, Debug)]
pub struct FoundTask<'a> {
    /// The board.
    pub board: &'a v1::Board,
    /// The column of the card it is on (or that it is).
    pub column: &'a v1::BoardColumn,
    /// The card on the board that holds it (itself, for a card).
    pub card: &'a v1::Task,
    /// The card first, the task itself last.
    pub path: Vec<TaskRef<'a>>,
}

impl<'a> FoundTask<'a> {
    /// The task itself.
    pub fn task(&self) -> TaskRef<'a> {
        self.path
            .last()
            .copied()
            .unwrap_or(TaskRef::Card(self.card))
    }

    /// Its parent, for a subtask.
    pub fn parent(&self) -> Option<TaskRef<'a>> {
        self.path.len().checked_sub(2).map(|i| self.path[i])
    }
}

impl WorkspaceMirror {
    /// Every task on every board, cards and subtasks, parents first.
    pub fn all_tasks(&self) -> impl Iterator<Item = TaskRef<'_>> {
        self.boards()
            .iter()
            .flat_map(|b| &b.columns)
            .flat_map(|c| &c.tasks)
            .flat_map(|card| TaskRef::Card(card).subtree())
    }

    /// An attachment, with the task it is attached to.
    pub fn attachment(&self, id: &str) -> Option<(TaskRef<'_>, &v1::Attachment)> {
        self.all_tasks()
            .find_map(|t| t.attachments().iter().find(|a| a.id == id).map(|a| (t, a)))
    }

    /// A task at any depth, with where it is.
    pub fn find_task(&self, id: &str) -> Option<FoundTask<'_>> {
        self.boards().iter().find_map(|board| {
            board.columns.iter().find_map(|column| {
                column.tasks.iter().find_map(|card| {
                    TaskRef::Card(card).find(id).map(|path| FoundTask {
                        board,
                        column,
                        card,
                        path,
                    })
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_are_found_at_any_depth_with_their_path() {
        let sub = |id: &str, subtasks: Vec<v1::Subtask>| v1::Subtask {
            id: id.into(),
            title: id.into(),
            subtasks,
            ..Default::default()
        };
        let card = v1::Task {
            id: "a".into(),
            subtasks: vec![sub("b", vec![sub("c", vec![])]), sub("d", vec![])],
            ..Default::default()
        };
        let w = WorkspaceMirror::new(v1::Workspace {
            boards: vec![v1::Board {
                id: "B".into(),
                columns: vec![v1::BoardColumn {
                    id: "C".into(),
                    tasks: vec![card],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        });
        let found = w.find_task("c").unwrap();
        let ids: Vec<&str> = found.path.iter().map(|t| t.id()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!(found.card.id, "a");
        assert_eq!(found.parent().map(TaskRef::id), Some("b"));
        assert_eq!(found.column.id, "C");
        assert!(w.find_task("a").unwrap().task().is_card());
        assert!(w.find_task("a").unwrap().parent().is_none());
        assert_eq!(
            TaskRef::Card(found.card).subtree_ids(),
            ["a", "b", "c", "d"]
        );
        assert!(w.find_task("x").is_none());
    }
}
