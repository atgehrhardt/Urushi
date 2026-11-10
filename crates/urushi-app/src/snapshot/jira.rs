//! The Jira scenarios: a board that follows Jira (`jira-board`), a Jira issue in the
//! editor (`jira-task`), signing in (`jira-connect`) and choosing a board to import
//! (`jira-import`).

use urushi_client::v1;

use crate::msg::{JiraMsg, Msg};
use crate::state::AppState;

/// The Jira scenarios.
pub(super) const SCENARIOS: &[&str] = &["jira-board", "jira-task", "jira-connect", "jira-import"];

const SITE: &str = "https://acme.atlassian.net";

fn issue(key: &str, (kind, status): (v1::TaskKind, &str), assignee: &str, epic: &str) -> v1::Task {
    let type_name = match kind {
        v1::TaskKind::Story => "Story",
        v1::TaskKind::Bug => "Bug",
        _ => "Task",
    };
    v1::Task {
        id: key.to_lowercase(),
        kind: kind.into(),
        issue: Some(Box::new(v1::JiraIssue {
            key: key.into(),
            url: format!("{SITE}/browse/{key}"),
            type_name: type_name.into(),
            status: status.into(),
            assignee: assignee.into(),
            priority: "Medium".into(),
            epic_key: epic.into(),
            notes_editable: true,
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// "ACME board": three columns of stories, bugs and tasks in two epics.
fn board() -> v1::Board {
    let column = |id: &str, name: &str, tasks: Vec<v1::Task>| v1::BoardColumn {
        id: id.into(),
        name: name.into(),
        tasks,
        jira_statuses: vec![id.into()],
        ..Default::default()
    };
    let mut checkout = issue(
        "ACME-142",
        (v1::TaskKind::Story, "To Do"),
        "Ada Lovelace",
        "ACME-7",
    );
    checkout.title = "Checkout with saved cards".into();
    checkout.notes = "Let returning customers pay with a card on file.".into();
    checkout.tags = vec!["payments".into()];
    let mut crash = issue("ACME-151", (v1::TaskKind::Bug, "To Do"), "", "");
    crash.title = "Crash when the cart is empty".into();
    let mut invoices = issue(
        "ACME-139",
        (v1::TaskKind::Story, "In Progress"),
        "Grace Hopper",
        "ACME-8",
    );
    invoices.title = "Invoice PDFs".into();
    invoices.subtasks = vec![
        v1::Subtask {
            id: "acme-140".into(),
            title: "Template".into(),
            done: true,
            kind: v1::TaskKind::Subtask.into(),
            ..Default::default()
        },
        v1::Subtask {
            id: "acme-141".into(),
            title: "Email them".into(),
            kind: v1::TaskKind::Subtask.into(),
            ..Default::default()
        },
    ];
    let mut keys = issue(
        "ACME-120",
        (v1::TaskKind::Task, "Done"),
        "Ada Lovelace",
        "ACME-7",
    );
    keys.title = "Rotate API keys".into();
    v1::Board {
        id: "jb".into(),
        name: "ACME board".into(),
        columns: vec![
            column("1", "To Do", vec![checkout, crash]),
            column("3", "In Progress", vec![invoices]),
            column("10001", "Done", vec![keys]),
        ],
        jira: Some(Box::new(v1::JiraBoardSource {
            site: SITE.into(),
            board_id: 7,
            name: "ACME board".into(),
            project_key: "ACME".into(),
            synced_at_ms: 1_000_000 - 120_000,
            url: format!("{SITE}/secure/RapidBoard.jspa?rapidView=7"),
            ..Default::default()
        })),
        epics: vec![
            v1::Epic {
                key: "ACME-7".into(),
                summary: "Payments".into(),
                color: v1::LabelColor::Purple.into(),
                url: String::new(),
            },
            v1::Epic {
                key: "ACME-8".into(),
                summary: "Billing".into(),
                color: v1::LabelColor::Orange.into(),
                url: String::new(),
            },
        ],
        ..Default::default()
    }
}

/// Sets up a Jira scenario on a connected state.
pub(super) fn show(state: &mut AppState, name: &str) {
    let mut workspace = state.workspace.raw().clone();
    workspace.boards.insert(0, board());
    state.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    state.now_ms = 1_000_000;
    state.jira = Some(v1::JiraAccount {
        connected: name != "jira-connect",
        site: SITE.into(),
        email: "ada@acme.com".into(),
        display_name: "Ada Lovelace".into(),
    });
    state.update(Msg::ToggleBoard);
    state.update(Msg::BoardReveal(1.0));
    state.update(Msg::SelectBoard("jb".into()));
    match name {
        "jira-task" => {
            state.update(Msg::PressTask("acme-139".into()));
            state.update(Msg::PressEnded);
        }
        "jira-connect" => {
            state.update(Msg::Jira(JiraMsg::OpenConnect));
            state.update(Msg::Paste("acme".into()));
        }
        "jira-import" => {
            state.update(Msg::Jira(JiraMsg::OpenImport));
            state.update(Msg::Response {
                followup: crate::msg::Followup::JiraBoards,
                result: Ok(v1::response::Outcome::JiraBoards(v1::JiraBoardList {
                    boards: vec![
                        v1::JiraBoardInfo {
                            id: 7,
                            name: "ACME board".into(),
                            project_name: "Acme".into(),
                            imported_board_id: "jb".into(),
                            ..Default::default()
                        },
                        v1::JiraBoardInfo {
                            id: 12,
                            name: "Mobile sprint".into(),
                            scrum: true,
                            project_name: "Mobile".into(),
                            ..Default::default()
                        },
                    ],
                })),
            });
        }
        _ => {}
    }
}
