//! Conversions between wire types ([`crate::v1`]) and `urushi-core` domain types.
//!
//! Going core → wire never fails. Going wire → core validates everything: ids, branch
//! names and layout invariants. Anything received from a peer is untrusted.

use std::path::PathBuf;
use std::str::FromStr;

use thiserror::Error;
use urushi_core::layout::{Child, Node};
use urushi_core::{
    Agent, AgentMode, AgentSettings, Axis, BranchName, BranchPrefix, Direction, Layout, PaneId,
    PromptDelivery, Repo, RepoSettingsOverrides, Settings, Slug, Status, Tab, TabId,
    TailnetSettings, TaskMessage, Thread, ThreadKind, ThreadState, Timestamp,
};

use crate::v1;

/// A wire value could not be turned into a domain value.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("invalid {field}: {reason}")]
pub struct ConvertError {
    /// Which field was invalid.
    pub field: &'static str,
    /// Why.
    pub reason: String,
}

impl ConvertError {
    fn new(field: &'static str, reason: impl std::fmt::Display) -> Self {
        Self {
            field,
            reason: reason.to_string(),
        }
    }
}

/// Parses an id (or any `FromStr` value) from a wire string, naming the field on failure.
pub fn parse_id<T: FromStr>(value: &str, field: &'static str) -> Result<T, ConvertError>
where
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|e: T::Err| ConvertError::new(field, e))
}

// ---- enums ------------------------------------------------------------------------

/// Core axis → wire.
pub fn axis_to_proto(axis: Axis) -> v1::Axis {
    match axis {
        Axis::Row => v1::Axis::Row,
        Axis::Column => v1::Axis::Column,
    }
}

/// Wire axis (raw enum value) → core.
pub fn axis_from_proto(value: i32) -> Result<Axis, ConvertError> {
    match v1::Axis::try_from(value) {
        Ok(v1::Axis::Row) => Ok(Axis::Row),
        Ok(v1::Axis::Column) => Ok(Axis::Column),
        _ => Err(ConvertError::new("axis", format!("unknown value {value}"))),
    }
}

/// Core direction → wire.
pub fn direction_to_proto(direction: Direction) -> v1::Direction {
    match direction {
        Direction::Left => v1::Direction::Left,
        Direction::Right => v1::Direction::Right,
        Direction::Up => v1::Direction::Up,
        Direction::Down => v1::Direction::Down,
    }
}

/// Wire direction (raw enum value) → core.
pub fn direction_from_proto(value: i32) -> Result<Direction, ConvertError> {
    match v1::Direction::try_from(value) {
        Ok(v1::Direction::Left) => Ok(Direction::Left),
        Ok(v1::Direction::Right) => Ok(Direction::Right),
        Ok(v1::Direction::Up) => Ok(Direction::Up),
        Ok(v1::Direction::Down) => Ok(Direction::Down),
        _ => Err(ConvertError::new(
            "direction",
            format!("unknown value {value}"),
        )),
    }
}

// ---- layout & tabs ----------------------------------------------------------------

/// Core layout → wire.
pub fn layout_to_proto(layout: &Layout) -> v1::LayoutNode {
    node_to_proto(layout.root())
}

fn node_to_proto(node: &Node) -> v1::LayoutNode {
    use v1::layout_node::Kind;
    let kind = match node {
        Node::Pane { id } => Kind::PaneId(id.to_string()),
        Node::Split { axis, children } => Kind::Split(v1::LayoutSplit {
            axis: axis_to_proto(*axis).into(),
            children: children
                .iter()
                .map(|c| v1::LayoutChild {
                    ratio: c.ratio,
                    node: Some(node_to_proto(&c.node)),
                })
                .collect(),
        }),
    };
    v1::LayoutNode { kind: Some(kind) }
}

/// Wire layout → core, checked against all layout invariants.
pub fn layout_from_proto(node: &v1::LayoutNode) -> Result<Layout, ConvertError> {
    Layout::from_root(node_from_proto(node, 0)?).map_err(|e| ConvertError::new("layout", e))
}

/// Deeper than any real layout. Stops a hostile peer from overflowing the stack.
const MAX_LAYOUT_DEPTH: usize = 64;

fn node_from_proto(node: &v1::LayoutNode, depth: usize) -> Result<Node, ConvertError> {
    use v1::layout_node::Kind;
    if depth > MAX_LAYOUT_DEPTH {
        return Err(ConvertError::new("layout", "nested too deeply"));
    }
    match &node.kind {
        Some(Kind::PaneId(id)) => Ok(Node::Pane {
            id: parse_id(id, "layout.pane_id")?,
        }),
        Some(Kind::Split(split)) => Ok(Node::Split {
            axis: axis_from_proto(split.axis)?,
            children: split
                .children
                .iter()
                .map(|c| {
                    let child = c
                        .node
                        .as_ref()
                        .ok_or_else(|| ConvertError::new("layout", "child without node"))?;
                    Ok(Child {
                        ratio: c.ratio,
                        node: node_from_proto(child, depth + 1)?,
                    })
                })
                .collect::<Result<_, ConvertError>>()?,
        }),
        None => Err(ConvertError::new("layout", "empty node")),
    }
}

/// Core tab → wire.
pub fn tab_to_proto(tab: &Tab) -> v1::Tab {
    v1::Tab {
        id: tab.id().to_string(),
        thread_id: tab.thread().to_string(),
        title: tab.title().to_owned(),
        layout: Some(layout_to_proto(tab.layout())),
        focused_pane_id: tab.focused().to_string(),
        // Volatile: the daemon adds them from its own state (see `urushi_core::attention`).
        attention: Vec::new(),
        running: Vec::new(),
    }
}

/// Wire tab → core.
pub fn tab_from_proto(tab: &v1::Tab) -> Result<Tab, ConvertError> {
    let layout = tab
        .layout
        .as_ref()
        .ok_or_else(|| ConvertError::new("tab.layout", "missing"))?;
    Tab::from_parts(
        parse_id::<TabId>(&tab.id, "tab.id")?,
        parse_id(&tab.thread_id, "tab.thread_id")?,
        tab.title.clone(),
        layout_from_proto(layout)?,
        parse_id::<PaneId>(&tab.focused_pane_id, "tab.focused_pane_id")?,
    )
    .map_err(|e| ConvertError::new("tab.focused_pane_id", e))
}

// ---- repos, projects & threads -----------------------------------------------------
//
// v1 names are frozen (ADR 0010): wire `Project` = core `Repo`, wire `Folder` = core
// `Project`.

pub(crate) fn millis_to_proto(t: Timestamp) -> i64 {
    i64::try_from(t.unix_millis()).unwrap_or(i64::MAX)
}

pub(crate) fn millis_from_proto(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(u64::try_from(ms).unwrap_or(0))
}

/// Core repo → wire (`v1::Project`).
pub fn repo_to_proto(p: &Repo) -> v1::Project {
    v1::Project {
        id: p.id.to_string(),
        name: p.name.clone(),
        repo_root: p.repo_root.to_string_lossy().into_owned(),
        default_base: p.default_base.clone().unwrap_or_default(),
        settings: Some(repo_settings_to_proto(&p.settings)),
        added_at_ms: millis_to_proto(p.added_at),
    }
}

/// Wire repo (`v1::Project`) → core.
pub fn repo_from_proto(p: &v1::Project) -> Result<Repo, ConvertError> {
    Ok(Repo {
        id: parse_id(&p.id, "project.id")?,
        name: p.name.clone(),
        repo_root: PathBuf::from(&p.repo_root),
        default_base: non_empty(&p.default_base),
        settings: p
            .settings
            .as_ref()
            .map(repo_settings_from_proto)
            .transpose()?
            .unwrap_or_default(),
        added_at: millis_from_proto(p.added_at_ms),
    })
}

/// A wire status: empty means none.
pub fn status_from_proto(status: &str) -> Result<Option<Status>, ConvertError> {
    if status.trim().is_empty() {
        return Ok(None);
    }
    Status::new(status)
        .map(Some)
        .map_err(|e| ConvertError::new("status", e))
}

/// Core thread → wire.
pub fn thread_to_proto(t: &Thread) -> v1::Thread {
    let (state, failure_reason) = match &t.state {
        ThreadState::Creating => (v1::ThreadState::Creating, String::new()),
        ThreadState::Ready => (v1::ThreadState::Ready, String::new()),
        ThreadState::Deleting => (v1::ThreadState::Deleting, String::new()),
        ThreadState::DeletionFailed { reason } => (v1::ThreadState::DeletionFailed, reason.clone()),
    };
    v1::Thread {
        id: t.id.to_string(),
        project_id: t.repo.to_string(),
        title: t.title.clone(),
        slug: t.slug.to_string(),
        worktree_path: t.worktree_path.to_string_lossy().into_owned(),
        primary_branch: t.primary_branch.to_string(),
        base_ref: t.base_ref.clone(),
        base_commit: t.base_commit.clone(),
        created_at_ms: millis_to_proto(t.created_at),
        state: state.into(),
        failure_reason,
        tab_ids: t.tabs.iter().map(ToString::to_string).collect(),
        folder_id: t.project.map(|p| p.to_string()).unwrap_or_default(),
        pinned: t.pinned,
        status: t
            .status
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        project_base: t.is_project_base(),
        attached_checkout: t.is_attached(),
    }
}

/// Wire thread → core.
pub fn thread_from_proto(t: &v1::Thread) -> Result<Thread, ConvertError> {
    let state = match v1::ThreadState::try_from(t.state) {
        Ok(v1::ThreadState::Creating) => ThreadState::Creating,
        Ok(v1::ThreadState::Ready) => ThreadState::Ready,
        Ok(v1::ThreadState::Deleting) => ThreadState::Deleting,
        Ok(v1::ThreadState::DeletionFailed) => ThreadState::DeletionFailed {
            reason: t.failure_reason.clone(),
        },
        _ => {
            return Err(ConvertError::new(
                "thread.state",
                format!("unknown value {}", t.state),
            ));
        }
    };
    Ok(Thread {
        id: parse_id(&t.id, "thread.id")?,
        repo: parse_id(&t.project_id, "thread.project_id")?,
        project: non_empty(&t.folder_id)
            .map(|id| parse_id(&id, "thread.folder_id"))
            .transpose()?,
        pinned: t.pinned,
        status: status_from_proto(&t.status)?,
        title: t.title.clone(),
        slug: Slug::from_title(&t.slug),
        worktree_path: PathBuf::from(&t.worktree_path),
        primary_branch: BranchName::new(t.primary_branch.clone())
            .map_err(|e| ConvertError::new("thread.primary_branch", e))?,
        base_ref: t.base_ref.clone(),
        base_commit: t.base_commit.clone(),
        created_at: millis_from_proto(t.created_at_ms),
        state,
        tabs: t
            .tab_ids
            .iter()
            .map(|id| parse_id(id, "thread.tab_ids"))
            .collect::<Result<_, _>>()?,
        kind: if t.project_base {
            ThreadKind::ProjectBase {
                attached: t.attached_checkout,
            }
        } else {
            ThreadKind::Branch
        },
    })
}

// ---- settings -----------------------------------------------------------------------

/// Core settings → wire.
pub fn settings_to_proto(s: &Settings) -> v1::Settings {
    v1::Settings {
        delete_remote_branches: s.delete_remote_branches,
        branch_prefix: s.branch_prefix.as_str().to_owned(),
        default_shell: s.default_shell.clone().unwrap_or_default(),
        worktree_root: s
            .worktree_root
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        update_base_from_remote: s.update_base_from_remote,
        tailnet: Some(v1::TailnetSettings {
            serve: s.tailnet.serve,
            port: u32::from(s.tailnet.port),
            allowed_logins: s.tailnet.allowed_logins.clone(),
            allow_tagged_nodes: s.tailnet.allow_tagged_nodes,
        }),
        agents: Some(v1::AgentSettings {
            claude_skip_permissions: s.agents.skips_permissions(Agent::Claude),
            codex_skip_permissions: s.agents.skips_permissions(Agent::Codex),
            cursor_skip_permissions: s.agents.skips_permissions(Agent::Cursor),
            task_agent: s
                .agents
                .task_agent
                .map_or(v1::Agent::Unspecified, agent_to_proto)
                .into(),
            task_mode: agent_mode_to_proto(s.agents.task_mode).into(),
            task_draft: s.agents.task_delivery.is_draft(),
            task_message_custom: s.agents.task_message.custom,
            task_message: s.agents.task_message.text.clone(),
        }),
        appearance: Some(appearance_to_proto(s.appearance)),
    }
}

/// Wire settings → core.
pub fn settings_from_proto(s: &v1::Settings) -> Result<Settings, ConvertError> {
    let tailnet = s.tailnet.clone().unwrap_or_default();
    Ok(Settings {
        delete_remote_branches: s.delete_remote_branches,
        branch_prefix: BranchPrefix::new(s.branch_prefix.clone())
            .map_err(|e| ConvertError::new("settings.branch_prefix", e))?,
        default_shell: non_empty(&s.default_shell),
        worktree_root: non_empty(&s.worktree_root).map(PathBuf::from),
        update_base_from_remote: s.update_base_from_remote,
        tailnet: TailnetSettings {
            serve: tailnet.serve,
            port: port_from_proto(tailnet.port)?,
            allowed_logins: tailnet.allowed_logins,
            allow_tagged_nodes: tailnet.allow_tagged_nodes,
        },
        agents: agent_settings_from_proto(s.agents.clone().unwrap_or_default()),
        appearance: appearance_from_proto(s.appearance.as_ref()),
    })
}

fn agent_settings_from_proto(a: v1::AgentSettings) -> AgentSettings {
    let mut settings = AgentSettings::default();
    settings.set_skip_permissions(Agent::Claude, a.claude_skip_permissions);
    settings.set_skip_permissions(Agent::Codex, a.codex_skip_permissions);
    settings.set_skip_permissions(Agent::Cursor, a.cursor_skip_permissions);
    settings.task_agent = agent_from_proto(a.task_agent);
    settings.task_mode = agent_mode_from_proto(a.task_mode);
    settings.task_delivery = PromptDelivery::drafting(a.task_draft);
    settings.task_message = TaskMessage {
        custom: a.task_message_custom,
        text: a.task_message,
    };
    settings
}

// ---- agents -------------------------------------------------------------------------

/// Core agent → wire.
pub fn agent_to_proto(agent: Agent) -> v1::Agent {
    match agent {
        Agent::Claude => v1::Agent::Claude,
        Agent::Codex => v1::Agent::Codex,
        Agent::Cursor => v1::Agent::Cursor,
    }
}

/// Wire agent → core. `AGENT_UNSPECIFIED` means a plain shell, so it is `None`, and so
/// is any value from a newer peer that this build doesn't know.
pub fn agent_from_proto(agent: i32) -> Option<Agent> {
    match v1::Agent::try_from(agent).ok()? {
        v1::Agent::Unspecified => None,
        v1::Agent::Claude => Some(Agent::Claude),
        v1::Agent::Codex => Some(Agent::Codex),
        v1::Agent::Cursor => Some(Agent::Cursor),
    }
}

/// Core agent mode → wire.
pub fn agent_mode_to_proto(mode: AgentMode) -> v1::AgentMode {
    match mode {
        AgentMode::Normal => v1::AgentMode::Normal,
        AgentMode::Plan => v1::AgentMode::Plan,
    }
}

/// Wire agent mode → core. Unspecified, and any value from a newer peer that this build
/// doesn't know, mean the normal mode: the agent then just asks as usual.
pub fn agent_mode_from_proto(mode: i32) -> AgentMode {
    match v1::AgentMode::try_from(mode) {
        Ok(v1::AgentMode::Plan) => AgentMode::Plan,
        _ => AgentMode::Normal,
    }
}

/// Validates a wire port number. Zero means the default port.
pub fn port_from_proto(port: u32) -> Result<u16, ConvertError> {
    if port == 0 {
        return Ok(TailnetSettings::DEFAULT_PORT);
    }
    u16::try_from(port).map_err(|_| ConvertError::new("port", format!("{port} is out of range")))
}

/// Core repo overrides → wire.
pub fn repo_settings_to_proto(o: &RepoSettingsOverrides) -> v1::ProjectSettings {
    v1::ProjectSettings {
        delete_remote_branches: o.delete_remote_branches,
        branch_prefix: o.branch_prefix.as_ref().map(|p| p.as_str().to_owned()),
        default_shell: o.default_shell.clone(),
    }
}

/// Wire repo overrides → core.
pub fn repo_settings_from_proto(
    o: &v1::ProjectSettings,
) -> Result<RepoSettingsOverrides, ConvertError> {
    Ok(RepoSettingsOverrides {
        delete_remote_branches: o.delete_remote_branches,
        branch_prefix: o
            .branch_prefix
            .clone()
            .map(BranchPrefix::new)
            .transpose()
            .map_err(|e| ConvertError::new("project.settings.branch_prefix", e))?,
        default_shell: o.default_shell.clone().filter(|s| !s.is_empty()),
    })
}

fn non_empty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_owned())
}

mod appearance;
mod folders;
pub use appearance::{appearance_from_proto, appearance_to_proto};
pub use folders::{
    project_from_proto, project_settings_from_proto, project_settings_to_proto, project_to_proto,
};

mod quick;
pub use quick::{quick_terminal_from_proto, quick_terminal_to_proto};

mod boards;
pub use boards::{
    attachment_to_proto, board_from_proto, board_to_proto, category_to_proto, color_from_proto,
    color_to_proto, column_to_proto, epic_from_proto, epic_to_proto, issue_from_proto,
    issue_to_proto, kind_from_proto, kind_to_proto, source_from_proto, source_to_proto,
    subtask_in_board_to_proto, subtask_to_proto, task_in_board_to_proto, task_to_proto,
};

#[cfg(test)]
mod tests;
