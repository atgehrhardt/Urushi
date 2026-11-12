//! The Jira scenarios: a board that follows Jira (`jira-board`), a Jira issue in the
//! editor (`jira-task`), one of its subtasks opened from it, under the epic and the
//! parent in the top bar (`jira-subtask`), the board filtered to one person's issues
//! (`jira-filters`) and the Filters menu's Assignee page (`jira-filter-menu`), an issue in
//! full with its fields and comments (`jira-issue`) and its assignee picker open
//! (`jira-issue-picker`), a new epic in the editor (`jira-new-epic`), signing in
//! (`jira-connect`), choosing a board to import (`jira-import`), and the questions
//! before an issue is deleted in Jira: from its card's menu (`jira-delete-issue`) and
//! for a subtask in the editor (`jira-delete-subtask`). With two sign-ins (ADR 0049):
//! Settings' Jira rows (`jira-accounts`) and the import dialog's site chips
//! (`jira-import-accounts`). `thread-links` shows the sidebar
//! with the marks of threads' Jira issues and pull requests (ADR 0036).

use urushi_client::v1;

use crate::filter::Dim;
use crate::msg::{FilterMsg, JiraMsg, Msg};
use crate::state::AppState;

/// The Jira scenarios.
pub(super) const SCENARIOS: &[&str] = &[
    "thread-links",
    "jira-board",
    "jira-task",
    "jira-subtask",
    "jira-filters",
    "jira-filter-menu",
    "jira-issue",
    "jira-issue-picker",
    "jira-new-epic",
    "jira-connect",
    "jira-import",
    "jira-delete-issue",
    "jira-delete-subtask",
    "jira-accounts",
    "jira-import-accounts",
];

const SITE: &str = "https://acme.atlassian.net";

pub(super) fn issue(
    key: &str,
    (kind, status): (v1::TaskKind, &str),
    assignee: &str,
    epic: &str,
) -> v1::Task {
    let type_name = match kind {
        v1::TaskKind::Story => "Story",
        v1::TaskKind::Bug => "Bug",
        v1::TaskKind::Epic => "Epic",
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
            assignee_id: if assignee.is_empty() {
                String::new()
            } else {
                format!("acc-{}", assignee.to_lowercase().replace(' ', "-"))
            },
            priority: "Medium".into(),
            epic_key: epic.into(),
            notes_editable: true,
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// Starts `task` as thread "Pricing page", whose pull request Jira reports (ADR 0036).
fn link_thread(task: &mut v1::Task) {
    task.thread_id = "t3".into();
    if let Some(issue) = task.issue.as_mut() {
        issue.pull_requests = vec![v1::PullRequest {
            url: "https://github.com/acme/web/pull/412".into(),
            title: "ACME-139 Invoice PDFs".into(),
            status: v1::PullRequestStatus::Open.into(),
        }];
    }
}

/// "ACME board": three columns of stories, bugs and tasks in two epics.
pub(super) fn board() -> v1::Board {
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
    link_thread(&mut invoices);
    let subtask = |key: &str, title: &str, done: bool| {
        let task = issue(
            key,
            (v1::TaskKind::Subtask, if done { "Done" } else { "To Do" }),
            "",
            "",
        );
        v1::Subtask {
            id: task.id,
            title: title.into(),
            done,
            kind: task.kind,
            issue: task.issue,
            ..Default::default()
        }
    };
    invoices.subtasks = vec![
        subtask("ACME-140", "Template", true),
        subtask("ACME-141", "Email them", false),
    ];
    let mut keys = issue(
        "ACME-120",
        (v1::TaskKind::Task, "Done"),
        "Ada Lovelace",
        "ACME-7",
    );
    keys.title = "Rotate API keys".into();
    // Epics are cards too (ADR 0027).
    let mut payments = issue("ACME-7", (v1::TaskKind::Epic, "In Progress"), "", "");
    payments.title = "Payments".into();
    v1::Board {
        id: "jb".into(),
        name: "ACME board".into(),
        columns: vec![
            column("1", "To Do", vec![checkout, crash]),
            column("3", "In Progress", vec![invoices, payments]),
            column("10001", "Done", vec![keys]),
        ],
        jira: Some(Box::new(v1::JiraBoardSource {
            site: SITE.into(),
            board_id: 7,
            name: "ACME board".into(),
            project_key: "ACME".into(),
            synced_at_ms: 1_000_000 - 120_000,
            url: format!("{SITE}/secure/RapidBoard.jspa?rapidView=7"),
            quick_filters: vec![
                v1::JiraQuickFilter {
                    id: 1,
                    name: "Bugs".into(),
                    jql: "type = Bug".into(),
                },
                v1::JiraQuickFilter {
                    id: 2,
                    name: "Blocked".into(),
                    jql: "status = Blocked".into(),
                },
            ],
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

/// **+** on To Do, with Type Epic picked.
fn new_epic(state: &mut AppState) {
    state.update(Msg::NewTask {
        board: "jb".into(),
        column: "1".into(),
    });
    state.update(Msg::Board(crate::msg::BoardMsg::PickKind(
        v1::TaskKind::Epic.into(),
    )));
}

/// The questions before an issue is deleted in Jira: from a card's menu, or for a
/// subtask in its parent's editor.
fn ask_to_delete(state: &mut AppState, name: &str) {
    if name == "jira-delete-issue" {
        state.update(Msg::Board(crate::msg::BoardMsg::DeleteTask(
            "acme-139".into(),
        )));
    } else {
        state.update(Msg::PressTask("acme-139".into()));
        state.update(Msg::PressEnded);
        state.update(Msg::DeleteSubtask("acme-141".into()));
    }
}

/// The sign-in the ACME board is synced with.
pub(super) fn account() -> v1::JiraAccount {
    v1::JiraAccount {
        connected: true,
        site: SITE.into(),
        email: "ada@acme.com".into(),
        display_name: "Ada Lovelace".into(),
        account_id: "acc-ada".into(),
        key: "acme.atlassian.net/ada@acme.com".into(),
    }
}

/// The scenario's sign-ins: none to connect, a second site's in the two-sign-in
/// scenarios, else the ACME one.
fn sign_in(state: &mut AppState, name: &str) {
    state.jira_accounts = if name == "jira-connect" {
        Vec::new()
    } else {
        vec![account()]
    };
    if name == "jira-accounts" || name == "jira-import-accounts" {
        state.jira_accounts.push(v1::JiraAccount {
            connected: true,
            site: "https://globex.atlassian.net".into(),
            email: "ada@globex.com".into(),
            display_name: "Ada Lovelace".into(),
            account_id: "acc-ada-globex".into(),
            key: "globex.atlassian.net/ada@globex.com".into(),
        });
    }
}

/// Sets up a Jira scenario on a connected state.
pub(super) fn show(state: &mut AppState, name: &str) {
    let mut workspace = state.workspace.raw().clone();
    workspace.boards.insert(0, board());
    state.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    state.now_ms = 1_000_000;
    sign_in(state, name);
    if name == "thread-links" {
        return;
    }
    if name == "jira-accounts" {
        state.update(Msg::OpenSettings);
        return;
    }
    state.update(Msg::ToggleBoard);
    state.update(Msg::BoardReveal(1.0));
    state.update(Msg::SelectBoard("jb".into()));
    match name {
        "jira-task" | "jira-subtask" => {
            state.update(Msg::PressTask("acme-139".into()));
            state.update(Msg::PressEnded);
            if name == "jira-subtask" {
                state.update(Msg::Board(crate::msg::BoardMsg::OpenTask(
                    "acme-141".into(),
                )));
            }
        }
        "jira-new-epic" => new_epic(state),
        "jira-delete-issue" | "jira-delete-subtask" => ask_to_delete(state, name),
        "jira-filters" => {
            state.update(Msg::Filter(FilterMsg::Toggle(
                Dim::Assignee,
                "acc-ada-lovelace".into(),
            )));
            state.update(Msg::Filter(FilterMsg::Recent));
        }
        "jira-filter-menu" => {
            state.update(Msg::Pointer(urushi_ui::Point::new(1000.0, 100.0)));
            state.update(Msg::OpenMenu(crate::organize::MenuTarget::Filter(
                "jb".into(),
            )));
            state.update(Msg::MenuPage(crate::organize::MenuPage::Filter(
                Dim::Assignee,
            )));
            state.update(Msg::Filter(FilterMsg::Toggle(
                Dim::Assignee,
                "acc-grace-hopper".into(),
            )));
        }
        "jira-issue" | "jira-issue-picker" => {
            state.update(Msg::PressTask("acme-139".into()));
            state.update(Msg::PressEnded);
            state.update(Msg::Response {
                followup: crate::msg::Followup::JiraIssue("acme-139".into()),
                result: Ok(v1::response::Outcome::JiraIssueDetails(
                    super::jira_issue::details(),
                )),
            });
            if name == "jira-issue-picker" {
                state.update(Msg::Issue(crate::msg::IssueMsg::Open(Some(
                    "assignee".into(),
                ))));
                state.update(Msg::Response {
                    followup: crate::msg::Followup::JiraFound("assignee".into()),
                    result: Ok(v1::response::Outcome::JiraUsers(v1::JiraUsers {
                        users: super::jira_issue::people(),
                    })),
                });
            }
        }
        "jira-connect" => {
            state.update(Msg::Jira(JiraMsg::OpenConnect));
            state.update(Msg::Paste("acme".into()));
        }
        "jira-import" | "jira-import-accounts" => {
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
