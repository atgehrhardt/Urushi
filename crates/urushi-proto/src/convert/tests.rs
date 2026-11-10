use std::path::PathBuf;

use urushi_core::thread::NewThread;
use urushi_core::{
    BranchPrefix, Direction, Layout, PaneId, Project, ProjectId, Repo, RepoId,
    RepoSettingsOverrides, Settings, Status, Tab, TabId, Thread, ThreadId, ThreadState, Timestamp,
};

use super::*;

fn p(n: u128) -> PaneId {
    PaneId::from_u128(n)
}

fn sample_layout() -> Layout {
    let mut l = Layout::single(p(1));
    l.split(p(1), p(2), Direction::Right).unwrap();
    l.split(p(2), p(3), Direction::Down).unwrap();
    l
}

#[test]
fn layout_round_trip() {
    let l = sample_layout();
    assert_eq!(layout_from_proto(&layout_to_proto(&l)).unwrap(), l);
}

#[test]
fn layout_from_proto_rejects_invalid_trees() {
    let mut wire = layout_to_proto(&sample_layout());
    // Duplicate a pane id.
    if let Some(v1::layout_node::Kind::Split(split)) = &mut wire.kind {
        split.children[1].node = Some(v1::LayoutNode {
            kind: Some(v1::layout_node::Kind::PaneId(p(1).to_string())),
        });
    }
    assert!(layout_from_proto(&wire).is_err());

    let empty = v1::LayoutNode { kind: None };
    assert!(layout_from_proto(&empty).is_err());

    let bad_id = v1::LayoutNode {
        kind: Some(v1::layout_node::Kind::PaneId("not-a-uuid".into())),
    };
    assert_eq!(
        layout_from_proto(&bad_id).unwrap_err().field,
        "layout.pane_id"
    );
}

#[test]
fn layout_depth_is_bounded() {
    let mut node = v1::LayoutNode {
        kind: Some(v1::layout_node::Kind::PaneId(p(0).to_string())),
    };
    for _ in 0..200 {
        node = v1::LayoutNode {
            kind: Some(v1::layout_node::Kind::Split(v1::LayoutSplit {
                axis: v1::Axis::Row.into(),
                children: vec![v1::LayoutChild {
                    ratio: 1.0,
                    node: Some(node),
                }],
            })),
        };
    }
    assert!(layout_from_proto(&node).is_err());
}

#[test]
fn tab_round_trip() {
    let mut tab = Tab::new(TabId::from_u128(1), ThreadId::from_u128(2), "agents", p(1));
    tab.split(p(1), p(2), Direction::Right).unwrap();
    assert_eq!(tab_from_proto(&tab_to_proto(&tab)).unwrap(), tab);
}

#[test]
fn thread_round_trip_all_states() {
    let mut thread = Thread::new(
        NewThread {
            id: ThreadId::from_u128(9),
            repo: RepoId::from_u128(1),
            project: None,
            title: "Ship it".into(),
            worktree_path: PathBuf::from("/w/t"),
            base_ref: "main".into(),
            base_commit: "abc123".into(),
            created_at: Timestamp::from_unix_millis(1_700_000_000_000),
            branch: None,
        },
        &BranchPrefix::default(),
    )
    .unwrap();
    thread.tabs.push(TabId::from_u128(5));
    for state in [
        ThreadState::Creating,
        ThreadState::Ready,
        ThreadState::Deleting,
        ThreadState::DeletionFailed {
            reason: "offline".into(),
        },
    ] {
        thread.state = state;
        assert_eq!(
            thread_from_proto(&thread_to_proto(&thread)).unwrap(),
            thread
        );
    }
    // Organization: project, pin and status.
    thread.project = Some(ProjectId::from_u128(4));
    thread.pinned = true;
    thread.status = Some(Status::new("In review").unwrap());
    assert_eq!(
        thread_from_proto(&thread_to_proto(&thread)).unwrap(),
        thread
    );
}

#[test]
fn folders_are_projects_and_statuses_are_validated() {
    let project = Project {
        id: ProjectId::from_u128(8),
        name: "Billing launch".into(),
        created_at: Timestamp::from_unix_millis(7),
    };
    assert_eq!(
        project_from_proto(&project_to_proto(&project)).unwrap(),
        project
    );
    assert_eq!(status_from_proto("").unwrap(), None);
    assert!(status_from_proto(&"x".repeat(80)).is_err());
}

#[test]
fn thread_from_proto_rejects_bad_branch() {
    let mut wire = v1::Thread {
        id: ThreadId::from_u128(1).to_string(),
        project_id: RepoId::from_u128(1).to_string(),
        primary_branch: "bad..branch".into(),
        state: v1::ThreadState::Ready.into(),
        ..Default::default()
    };
    assert_eq!(
        thread_from_proto(&wire).unwrap_err().field,
        "thread.primary_branch"
    );
    wire.primary_branch = "ok".into();
    wire.state = 99;
    assert_eq!(thread_from_proto(&wire).unwrap_err().field, "thread.state");
}

#[test]
fn repo_round_trip() {
    let repo = Repo {
        id: RepoId::from_u128(3),
        name: "urushi".into(),
        repo_root: PathBuf::from("/src/urushi"),
        default_base: Some("main".into()),
        settings: RepoSettingsOverrides {
            delete_remote_branches: Some(false),
            branch_prefix: Some(BranchPrefix::new("a/").unwrap()),
            default_shell: None,
        },
        added_at: Timestamp::from_unix_millis(42),
    };
    assert_eq!(repo_from_proto(&repo_to_proto(&repo)).unwrap(), repo);
}

#[test]
fn settings_round_trip() {
    let mut s = Settings {
        default_shell: Some("/bin/zsh".into()),
        worktree_root: Some(PathBuf::from("/tmp/wt")),
        ..Settings::default()
    };
    s.tailnet.serve = true;
    s.tailnet.allowed_logins = vec!["me@example.com".into()];
    s.agents
        .set_skip_permissions(urushi_core::Agent::Claude, true);
    s.agents
        .set_skip_permissions(urushi_core::Agent::Cursor, true);
    s.agents.task_agent = Some(urushi_core::Agent::Claude);
    s.agents.task_mode = urushi_core::AgentMode::Plan;
    s.update_base_from_remote = false;
    assert_eq!(settings_from_proto(&settings_to_proto(&s)).unwrap(), s);
    assert_eq!(
        settings_from_proto(&settings_to_proto(&Settings::default())).unwrap(),
        Settings::default()
    );
}

#[test]
fn agents_round_trip_and_unknown_values_mean_a_plain_shell() {
    for agent in urushi_core::Agent::ALL {
        assert_eq!(agent_from_proto(agent_to_proto(agent).into()), Some(agent));
    }
    assert_eq!(agent_from_proto(v1::Agent::Unspecified.into()), None);
    assert_eq!(agent_from_proto(99), None, "a value from a newer peer");
}

#[test]
fn agent_modes_round_trip_and_unknown_values_mean_normal() {
    for mode in urushi_core::AgentMode::ALL {
        assert_eq!(
            agent_mode_from_proto(agent_mode_to_proto(mode).into()),
            mode
        );
    }
    let normal = urushi_core::AgentMode::Normal;
    assert_eq!(
        agent_mode_from_proto(v1::AgentMode::Unspecified.into()),
        normal
    );
    assert_eq!(
        agent_mode_from_proto(99),
        normal,
        "a value from a newer peer"
    );
}

#[test]
fn ports_are_validated() {
    assert_eq!(
        port_from_proto(0).unwrap(),
        urushi_core::TailnetSettings::DEFAULT_PORT
    );
    assert_eq!(port_from_proto(9000).unwrap(), 9000);
    assert!(port_from_proto(70_000).is_err());
}

#[test]
fn enums_reject_unspecified() {
    assert!(axis_from_proto(0).is_err());
    assert!(direction_from_proto(0).is_err());
    for d in [
        Direction::Left,
        Direction::Right,
        Direction::Up,
        Direction::Down,
    ] {
        assert_eq!(
            direction_from_proto(direction_to_proto(d).into()).unwrap(),
            d
        );
    }
}
