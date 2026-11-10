//! What sort of work a task is: a story, a bug, an epic… Any board's tasks can have a
//! kind; Jira boards take it from each issue's type.
//!
//! Kinds are a small fixed set rather than free text so every client can draw the same
//! glyph and color for them. Jira's own type names (which a site can rename or extend)
//! are kept on the issue ([`super::Issue::type_name`]); [`TaskKind::from_jira`] maps them
//! onto this set.

use serde::{Deserialize, Serialize};

/// The kind of a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    /// A piece of work.
    Task,
    /// A user-facing feature.
    Story,
    /// Something broken.
    Bug,
    /// A large body of work that groups other tasks.
    Epic,
    /// A step of another task.
    Subtask,
}

impl TaskKind {
    /// Every kind, in the order pickers list them.
    pub const ALL: [Self; 5] = [
        Self::Task,
        Self::Story,
        Self::Bug,
        Self::Epic,
        Self::Subtask,
    ];

    /// The lowercase name (`"story"`), as shown and as typed in the CLI.
    pub fn name(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Story => "story",
            Self::Bug => "bug",
            Self::Epic => "epic",
            Self::Subtask => "subtask",
        }
    }

    /// The kind with this name (any case; "sub-task" too).
    pub fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase().replace('-', "");
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The kind of a Jira issue, from its type's name and hierarchy level (`1` for
    /// epics, `-1` for subtasks, `0` for everything else). Types a site renamed or
    /// added fall back on the level: a standard-level type is a task.
    pub fn from_jira(type_name: &str, hierarchy_level: i32) -> Self {
        match hierarchy_level {
            l if l >= 1 => Self::Epic,
            l if l < 0 => Self::Subtask,
            _ => match Self::parse(type_name) {
                Some(Self::Story) => Self::Story,
                Some(Self::Bug) => Self::Bug,
                _ => Self::Task,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_have_names_that_parse_back() {
        for k in TaskKind::ALL {
            assert_eq!(TaskKind::parse(k.name()), Some(k));
        }
        assert_eq!(TaskKind::parse(" Sub-task "), Some(TaskKind::Subtask));
        assert_eq!(TaskKind::parse("chore"), None);
    }

    #[test]
    fn jira_types_map_by_level_then_name() {
        assert_eq!(TaskKind::from_jira("Epic", 1), TaskKind::Epic);
        assert_eq!(TaskKind::from_jira("Initiative", 2), TaskKind::Epic);
        assert_eq!(TaskKind::from_jira("Sub-task", -1), TaskKind::Subtask);
        assert_eq!(TaskKind::from_jira("Story", 0), TaskKind::Story);
        assert_eq!(TaskKind::from_jira("Bug", 0), TaskKind::Bug);
        assert_eq!(TaskKind::from_jira("Spike", 0), TaskKind::Task);
    }
}
