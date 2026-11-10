//! Pure domain model for Urushi.
//!
//! This crate defines *what* a workspace is: repos, projects, threads, tabs, the split-pane
//! layout tree, task boards, and settings. It knows nothing about IO, async, platforms, or
//! rendering. The daemon persists and changes these types. Clients mirror them.
//!
//! Rules (see `AGENTS.md`):
//! - No filesystem, network, process, clock or randomness access. Ids and timestamps
//!   are passed in by the caller.
//! - No platform `cfg`. This crate must compile for every target, including iOS and Android.
#![forbid(unsafe_code)]

pub mod agent;
pub mod attention;
pub mod board;
pub mod ids;
pub mod layout;
pub mod naming;
pub mod ownership;
pub mod project;
pub mod repo;
pub mod settings;
pub mod shell;
pub mod status;
pub mod tab;
pub mod thread;
pub mod time;

pub use agent::{
    Agent, AgentMode, AgentSessionId, AgentSettings, InvalidSessionId, TASK_PROMPT_ENV,
};
pub use attention::AttentionKind;
pub use board::{
    Board, BoardError, BoardSource, Column, LabelColor, Task, TaskKind, TaskPatch, task_prompt,
};
pub use ids::{
    AttachmentId, BoardId, ColumnId, PaneId, ProjectId, RepoId, TabId, TaskId, ThreadId,
};
pub use layout::{Axis, Direction, Layout, LayoutError};
pub use naming::{BranchName, BranchPrefix, NamingError, Slug};
pub use ownership::{OwnershipFacts, owned_branches};
pub use project::Project;
pub use repo::Repo;
pub use settings::{EffectiveSettings, RepoSettingsOverrides, Settings, TailnetSettings};
pub use status::{Status, StatusError};
pub use tab::Tab;
pub use thread::{NewThread, Thread, ThreadState, ThreadStateError};
pub use time::Timestamp;
