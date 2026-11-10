//! Every request, response, event and notification kind survives encode → decode unchanged.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

use urushi_proto::codec::{FrameDecoder, encode_frame};
use urushi_proto::envelope;
use urushi_proto::v1::{self, event, notification, request, response};

fn round_trip(e: &v1::Envelope) {
    let mut decoder = FrameDecoder::new();
    decoder.push(&encode_frame(e).unwrap());
    assert_eq!(decoder.next_envelope().unwrap().as_ref(), Some(e));
}

fn screen() -> v1::ScreenUpdate {
    v1::ScreenUpdate {
        pane_id: "p".into(),
        seq: 3,
        full: true,
        cols: 80,
        rows: 24,
        lines: vec![v1::ScreenLine {
            index: 0,
            spans: vec![v1::Span {
                text: "héllo 世界".into(),
                style: Some(v1::Style {
                    fg: Some(v1::Color {
                        kind: Some(v1::color::Kind::Indexed(2)),
                    }),
                    bg: Some(v1::Color {
                        kind: Some(v1::color::Kind::Rgb(0x10_20_30)),
                    }),
                    flags: v1::StyleFlag::Bold as u32 | v1::StyleFlag::Wide as u32,
                }),
            }],
        }],
        cursor: Some(v1::Cursor {
            line: 1,
            col: 2,
            visible: true,
            shape: v1::CursorShape::Bar.into(),
        }),
        modes: Some(v1::TerminalModes {
            app_cursor: true,
            ..Default::default()
        }),
        title: "zsh".into(),
        display_offset: 0,
        history_size: 100,
    }
}

#[test]
#[allow(clippy::too_many_lines, reason = "one entry per request kind")]
fn all_request_kinds() {
    use request::Body as B;
    let bodies = vec![
        B::Hello(v1::Hello {
            protocol_major: 1,
            protocol_minor: 0,
            client_name: "t".into(),
            client_version: "0".into(),
        }),
        B::ListWorkspace(v1::ListWorkspaceRequest { subscribe: true }),
        B::AddProject(v1::AddProjectRequest {
            path: "/r".into(),
            name: String::new(),
        }),
        B::RemoveProject(v1::RemoveProjectRequest {
            project_id: "x".into(),
        }),
        B::ListBranches(v1::ListBranchesRequest {
            project_id: "x".into(),
        }),
        B::CreateThread(v1::CreateThreadRequest {
            project_id: "x".into(),
            title: "t".into(),
            base_ref: "main".into(),
            branch: "Primary".into(),
            skip_initial_tab: true,
            folder_id: "f".into(),
            task_id: "task".into(),
            subtask_id: "sub".into(),
            start_task_agent: true,
        }),
        B::RenameThread(v1::RenameThreadRequest {
            thread_id: "t".into(),
            title: "n".into(),
        }),
        B::PlanThreadDeletion(v1::PlanThreadDeletionRequest {
            thread_id: "t".into(),
        }),
        B::DeleteThread(v1::DeleteThreadRequest {
            thread_id: "t".into(),
            token: "k".into(),
        }),
        B::CreateTab(v1::CreateTabRequest {
            thread_id: "t".into(),
            title: String::new(),
            agent: v1::Agent::Claude.into(),
            task_id: "task".into(),
            subtask_id: "sub".into(),
            start_task_agent: true,
        }),
        B::CloseTab(v1::CloseTabRequest { tab_id: "t".into() }),
        B::RenameTab(v1::RenameTabRequest {
            tab_id: "t".into(),
            title: "n".into(),
        }),
        B::SplitPane(v1::SplitPaneRequest {
            tab_id: "t".into(),
            pane_id: "p".into(),
            side: v1::Direction::Right.into(),
        }),
        B::ClosePane(v1::ClosePaneRequest {
            tab_id: "t".into(),
            pane_id: "p".into(),
        }),
        B::FocusPane(v1::FocusPaneRequest {
            tab_id: "t".into(),
            pane_id: "p".into(),
        }),
        B::MoveDivider(v1::MoveDividerRequest {
            tab_id: "t".into(),
            anchor_pane_id: "p".into(),
            axis: v1::Axis::Column.into(),
            position: 0.3,
        }),
        B::ResizePaneEdge(v1::ResizePaneEdgeRequest {
            tab_id: "t".into(),
            pane_id: "p".into(),
            side: v1::Direction::Up.into(),
            amount: -0.1,
        }),
        B::Equalize(v1::EqualizeRequest { tab_id: "t".into() }),
        B::AttachPane(v1::AttachPaneRequest {
            pane_id: "p".into(),
            cols: 80,
            rows: 24,
        }),
        B::DetachPane(v1::DetachPaneRequest {
            pane_id: "p".into(),
        }),
        B::SetPaneSize(v1::SetPaneSizeRequest {
            pane_id: "p".into(),
            cols: 100,
            rows: 30,
        }),
        B::GetSettings(v1::GetSettingsRequest {}),
        B::UpdateSettings(v1::UpdateSettingsRequest {
            patch: Some(v1::SettingsPatch {
                delete_remote_branches: Some(false),
                set_allowed_logins: true,
                claude_skip_permissions: Some(true),
                codex_skip_permissions: Some(false),
                task_agent: Some(v1::Agent::Claude.into()),
                task_mode: Some(v1::AgentMode::Plan.into()),
                update_base_from_remote: Some(false),
                task_draft: Some(true),
                task_message_custom: Some(true),
                task_message: Some("Pick up {key}".into()),
                ..Default::default()
            }),
        }),
        B::UpdateProjectSettings(v1::UpdateProjectSettingsRequest {
            project_id: "x".into(),
            settings: Some(v1::ProjectSettings::default()),
        }),
        B::TailnetStatus(v1::TailnetStatusRequest {}),
        B::SetTailnetServe(v1::SetTailnetServeRequest { serve: true }),
        B::ReadScreenText(v1::ReadScreenTextRequest {
            pane_id: "p".into(),
        }),
        B::ListAgents(v1::ListAgentsRequest {}),
        B::UpdateThread(v1::UpdateThreadRequest {
            thread_id: "t".into(),
            title: None,
            pinned: Some(true),
            status: Some(String::new()),
            folder_id: Some("f".into()),
        }),
        B::CreateFolder(v1::CreateFolderRequest {
            name: "Billing".into(),
        }),
        B::RenameFolder(v1::RenameFolderRequest {
            folder_id: "f".into(),
            name: "Billing v2".into(),
        }),
        B::DeleteFolder(v1::DeleteFolderRequest {
            folder_id: "f".into(),
        }),
        B::MoveFolder(v1::MoveFolderRequest {
            folder_id: "f".into(),
            index: 2,
        }),
        B::MoveBoard(v1::MoveBoardRequest {
            board_id: "b".into(),
            index: 3,
        }),
        B::UpdateFolderSettings(v1::UpdateFolderSettingsRequest {
            folder_id: "f".into(),
            settings: Some(v1::FolderSettings {
                base_repo_id: "r".into(),
                base_branch: "origin/develop".into(),
                base_thread: Some(false),
            }),
        }),
        B::ClearAttention(v1::ClearAttentionRequest {
            pane_ids: vec!["p".into(), "q".into()],
        }),
    ];
    for (i, body) in bodies.into_iter().enumerate() {
        round_trip(&envelope::request(i as u64, body));
    }
}

#[test]
fn all_response_kinds() {
    use response::Outcome as O;
    let outcomes = vec![
        O::Error(v1::Error {
            code: v1::ErrorCode::Conflict.into(),
            message: "stale".into(),
        }),
        O::Ok(v1::Empty {}),
        O::Welcome(v1::Welcome {
            protocol_major: 1,
            protocol_minor: 0,
            daemon_version: "0.1".into(),
            host_name: "h".into(),
        }),
        O::Workspace(v1::Workspace {
            revision: 7,
            ..Default::default()
        }),
        O::Project(v1::Project::default()),
        O::Branches(v1::Branches {
            branches: vec!["main".into()],
            current: "main".into(),
            remote_branches: vec!["origin/feature".into()],
        }),
        O::Thread(v1::Thread::default()),
        O::DeletionPlan(v1::DeletionPlan {
            local_branches: vec!["urushi/x".into()],
            remote_branches: vec![v1::RemoteBranch {
                remote: "origin".into(),
                branch: "urushi/x".into(),
            }],
            token: "abc".into(),
            keeps_worktree: true,
            ..Default::default()
        }),
        O::Tab(v1::Tab::default()),
        O::PaneCreated(v1::PaneCreated {
            tab_id: "t".into(),
            pane_id: "p".into(),
        }),
        O::Screen(screen()),
        O::Settings(v1::Settings {
            update_base_from_remote: true,
            agents: Some(v1::AgentSettings {
                cursor_skip_permissions: true,
                task_agent: v1::Agent::Codex.into(),
                task_mode: v1::AgentMode::Normal.into(),
                task_draft: true,
                task_message_custom: true,
                task_message: "Do {key}".into(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        O::TailnetStatus(v1::TailnetStatus {
            running: true,
            peers: vec![v1::TailnetPeer::default()],
            ..Default::default()
        }),
        O::ScreenText(v1::ScreenText {
            lines: vec!["$ ls".into()],
        }),
        O::Folder(v1::Folder {
            id: "f".into(),
            name: "Billing".into(),
            created_at_ms: 1,
            settings: Some(v1::FolderSettings {
                base_repo_id: "r".into(),
                base_branch: "main".into(),
                base_thread: None,
            }),
        }),
        O::Workspace(v1::Workspace {
            folders: vec![v1::Folder::default()],
            threads: vec![v1::Thread {
                folder_id: "f".into(),
                pinned: true,
                status: "Blocked".into(),
                project_base: true,
                attached_checkout: true,
                ..Default::default()
            }],
            ..Default::default()
        }),
        O::Agents(v1::Agents {
            agents: vec![v1::InstalledAgent {
                agent: v1::Agent::Codex.into(),
                path: "/usr/local/bin/codex".into(),
            }],
        }),
    ];
    for (i, outcome) in outcomes.into_iter().enumerate() {
        round_trip(&envelope::response(i as u64, outcome));
    }
}

#[test]
fn all_event_and_notification_kinds() {
    for body in [
        event::Body::WorkspaceChanged(v1::Workspace::default()),
        event::Body::ScreenUpdate(screen()),
        event::Body::PaneExited(v1::PaneExited {
            pane_id: "p".into(),
            exit_code: 130,
        }),
        event::Body::PaneBell(v1::PaneBell {
            pane_id: "p".into(),
        }),
    ] {
        round_trip(&envelope::event(body));
    }
    for body in [
        notification::Body::Input(v1::Input {
            pane_id: "p".into(),
            kind: Some(v1::input::Kind::Key(v1::KeyInput {
                key: v1::Key::Character.into(),
                text: "c".into(),
                modifiers: v1::Modifier::Ctrl as u32,
            })),
        }),
        notification::Body::Input(v1::Input {
            pane_id: "p".into(),
            kind: Some(v1::input::Kind::Paste("multi\nline".into())),
        }),
        notification::Body::Input(v1::Input {
            pane_id: "p".into(),
            kind: Some(v1::input::Kind::Mouse(v1::MouseInput {
                button: v1::MouseButton::Left.into(),
                action: v1::MouseAction::Release.into(),
                col: 3,
                row: 4,
                modifiers: 1,
            })),
        }),
        notification::Body::Input(v1::Input {
            pane_id: "p".into(),
            kind: Some(v1::input::Kind::Wheel(v1::WheelInput {
                up: true,
                lines: 3,
                col: 1,
                row: 2,
                modifiers: 0,
            })),
        }),
        notification::Body::AckScreen(v1::AckScreen {
            pane_id: "p".into(),
            seq: 9,
        }),
        notification::Body::ScrollPane(v1::ScrollPane {
            pane_id: "p".into(),
            lines: -3,
            to_bottom: false,
        }),
    ] {
        round_trip(&envelope::notification(body));
    }
}

#[test]
fn unknown_fields_are_ignored() {
    // A newer peer may send fields we don't know; decoding must still succeed.
    use prost::Message;
    let mut bytes = envelope::ok(1).encode_to_vec();
    // Field 15 (unknown at the envelope level), wire type varint, value 1.
    bytes.extend_from_slice(&[0x78, 0x01]);
    let decoded = v1::Envelope::decode(bytes.as_slice()).unwrap();
    assert_eq!(decoded, envelope::ok(1));
}

#[test]
fn tabs_carry_pane_attention() {
    let tab = v1::Tab {
        id: "t".into(),
        thread_id: "th".into(),
        title: "Claude".into(),
        attention: vec![
            v1::PaneAttention {
                pane_id: "p".into(),
                kind: v1::AttentionKind::Waiting.into(),
                since_ms: 1_700_000_000_000,
            },
            v1::PaneAttention {
                pane_id: "q".into(),
                kind: v1::AttentionKind::Done.into(),
                since_ms: 1_700_000_000_001,
            },
        ],
        ..Default::default()
    };
    round_trip(&envelope::response(1, response::Outcome::Tab(tab)));
}
