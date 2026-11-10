//! Strongly typed identifiers.
//!
//! Each kind of entity has its own id type, so a `PaneId` can never be passed where a
//! `TabId` is expected. Ids wrap UUIDs. The *daemon* generates them (`UUIDv7`, so they
//! sort by creation time). This crate only wraps, compares and formats them, which keeps
//! it free of clocks and randomness.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Wraps an existing UUID (freshly generated, loaded from storage, or
            /// received over the wire).
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Builds an id from a `u128`. Useful for deterministic ids in tests.
            pub const fn from_u128(value: u128) -> Self {
                Self(Uuid::from_u128(value))
            }

            /// The underlying UUID.
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }

            /// The last 8 hex digits: a compact, human-friendly suffix for names such
            /// as branch names. Not guaranteed unique; never use it as a key.
            pub fn short(&self) -> String {
                let simple = self.0.simple().to_string();
                simple[simple.len() - 8..].to_owned()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

define_id!(
    /// Identifies a [`crate::Repo`] (a git repository added to the workspace).
    RepoId
);
define_id!(
    /// Identifies a [`crate::Project`] (a user-made group of threads).
    ProjectId
);
define_id!(
    /// Identifies a [`crate::Thread`] (one worktree and its tabs).
    ThreadId
);
define_id!(
    /// Identifies a [`crate::Tab`] (a layout of panes inside a thread).
    TabId
);
define_id!(
    /// Identifies a pane: one terminal session inside a tab's layout.
    PaneId
);
define_id!(
    /// Identifies a [`crate::Board`] (a kanban board of tasks).
    BoardId
);
define_id!(
    /// Identifies a column of a [`crate::Board`].
    ColumnId
);
define_id!(
    /// Identifies a [`crate::Task`] on a board, a card or a subtask at any depth.
    TaskId
);
define_id!(
    /// Identifies a file attached to a task ([`crate::board::Attachment`]).
    AttachmentId
);
define_id!(
    /// Identifies a quick filter a board keeps ([`crate::board::QuickFilter`]).
    QuickFilterId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_is_last_eight_hex_digits() {
        let id = ThreadId::from_u128(0x0123_4567_89ab_cdef_0011_2233_4455_6677);
        assert_eq!(id.short(), "44556677");
    }

    #[test]
    fn display_and_parse_round_trip() {
        let id = PaneId::from_u128(42);
        let parsed: PaneId = id.to_string().parse().unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn debug_names_the_kind() {
        let id = TabId::from_u128(1);
        assert!(format!("{id:?}").starts_with("TabId("));
    }
}
