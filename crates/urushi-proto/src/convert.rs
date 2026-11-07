//! Conversions between wire types ([`crate::v1`]) and `urushi-core` domain types.
//!
//! Going core → wire never fails. Going wire → core validates everything: ids, branch
//! names and layout invariants. Anything received from a peer is untrusted.

use std::path::PathBuf;
use std::str::FromStr;

use thiserror::Error;
use urushi_core::layout::{Child, Node};
use urushi_core::{
    Axis, BranchName, BranchPrefix, Direction, Layout, PaneId, Project, ProjectSettingsOverrides,
    Settings, Slug, Tab, TabId, TailnetSettings, Thread, ThreadState, Timestamp,
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

// ---- projects & threads -----------------------------------------------------------

fn millis_to_proto(t: Timestamp) -> i64 {
    i64::try_from(t.unix_millis()).unwrap_or(i64::MAX)
}

fn millis_from_proto(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(u64::try_from(ms).unwrap_or(0))
}

/// Core project → wire.
pub fn project_to_proto(p: &Project) -> v1::Project {
    v1::Project {
        id: p.id.to_string(),
        name: p.name.clone(),
        repo_root: p.repo_root.to_string_lossy().into_owned(),
        default_base: p.default_base.clone().unwrap_or_default(),
        settings: Some(project_settings_to_proto(&p.settings)),
        added_at_ms: millis_to_proto(p.added_at),
    }
}

/// Wire project → core.
pub fn project_from_proto(p: &v1::Project) -> Result<Project, ConvertError> {
    Ok(Project {
        id: parse_id(&p.id, "project.id")?,
        name: p.name.clone(),
        repo_root: PathBuf::from(&p.repo_root),
        default_base: non_empty(&p.default_base),
        settings: p
            .settings
            .as_ref()
            .map(project_settings_from_proto)
            .transpose()?
            .unwrap_or_default(),
        added_at: millis_from_proto(p.added_at_ms),
    })
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
        project_id: t.project.to_string(),
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
        project: parse_id(&t.project_id, "thread.project_id")?,
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
        tailnet: Some(v1::TailnetSettings {
            serve: s.tailnet.serve,
            port: u32::from(s.tailnet.port),
            allowed_logins: s.tailnet.allowed_logins.clone(),
            allow_tagged_nodes: s.tailnet.allow_tagged_nodes,
        }),
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
        tailnet: TailnetSettings {
            serve: tailnet.serve,
            port: port_from_proto(tailnet.port)?,
            allowed_logins: tailnet.allowed_logins,
            allow_tagged_nodes: tailnet.allow_tagged_nodes,
        },
    })
}

/// Validates a wire port number. Zero means the default port.
pub fn port_from_proto(port: u32) -> Result<u16, ConvertError> {
    if port == 0 {
        return Ok(TailnetSettings::DEFAULT_PORT);
    }
    u16::try_from(port).map_err(|_| ConvertError::new("port", format!("{port} is out of range")))
}

/// Core project overrides → wire.
pub fn project_settings_to_proto(o: &ProjectSettingsOverrides) -> v1::ProjectSettings {
    v1::ProjectSettings {
        delete_remote_branches: o.delete_remote_branches,
        branch_prefix: o.branch_prefix.as_ref().map(|p| p.as_str().to_owned()),
        default_shell: o.default_shell.clone(),
    }
}

/// Wire project overrides → core.
pub fn project_settings_from_proto(
    o: &v1::ProjectSettings,
) -> Result<ProjectSettingsOverrides, ConvertError> {
    Ok(ProjectSettingsOverrides {
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

#[cfg(test)]
mod tests;
