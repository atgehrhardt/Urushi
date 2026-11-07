//! Pure domain model for Urushi.
//!
//! This crate defines *what* a workspace is: projects, threads, tabs, the split-pane
//! layout tree, and settings. It knows nothing about IO, async, platforms, or rendering.
//! The daemon persists and changes these types. Clients mirror them.
//!
//! Rules (see `AGENTS.md`):
//! - No filesystem, network, process, clock or randomness access. Ids and timestamps
//!   are passed in by the caller.
//! - No platform `cfg`. This crate must compile for every target, including iOS and Android.
#![forbid(unsafe_code)]

pub mod ids;
pub mod layout;
pub mod naming;
pub mod ownership;
pub mod project;
pub mod settings;
pub mod tab;
pub mod thread;
pub mod time;

pub use ids::{PaneId, ProjectId, TabId, ThreadId};
pub use layout::{Axis, Direction, Layout, LayoutError};
pub use naming::{BranchName, BranchPrefix, NamingError, Slug};
pub use ownership::{OwnershipFacts, owned_branches};
pub use project::Project;
pub use settings::{EffectiveSettings, ProjectSettingsOverrides, Settings, TailnetSettings};
pub use tab::Tab;
pub use thread::{NewThread, Thread, ThreadState, ThreadStateError};
pub use time::Timestamp;
