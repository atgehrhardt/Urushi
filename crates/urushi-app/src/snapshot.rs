//! Builds the scene of a fresh app state, for offscreen snapshots (see
//! `examples/snapshot.rs`) and layout checks at different window sizes.

use std::collections::HashMap;

use urushi_client::v1;
use urushi_ui::{CellMetrics, Point, Rect, Scene, TextSystem, Theme, build};

use crate::config::ConnectTarget;
use crate::msg::Msg;
use crate::organize::{MenuPage, MenuTarget, Select, SidebarTab};
use crate::state::AppState;

/// The scene of a freshly started app connecting to `target`, at `width`×`height` logical pixels.
pub fn scene(
    target: ConnectTarget,
    theme: &Theme,
    text: &mut TextSystem,
    width: f32,
    height: f32,
) -> Scene {
    let cell: CellMetrics = text.cell_metrics(theme.terminal_font_size);
    let mut state = AppState::new(target, cell);
    state.compact = width < 700.0;
    let root = crate::view::view(&state, theme, &HashMap::new());
    build(&root, Rect::new(0.0, 0.0, width, height), text, None).scene
}

/// Named screens for [`scenario`]: a thread with no tabs (`launcher`), the new-tab
/// `picker`, the `add-repo` and `new-thread` dialogs (`new-thread-base` with the branch
/// list open), `settings`, the thread `menu` and its `menu-status` page, a thread being
/// dragged (`drag`), the `repos` tab, the task `board` (a task being dragged across it in
/// `board-drag`, a `task` with subtasks being edited, and a card's context menu in
/// `task-menu`, a column's in `column-menu`, a subtask opened as a task in
/// `task-nested`, and "Make subtask of…" in `nest`), and a terminal with an
/// agent's plan usage in the `status` bar, and the first-run `setup` wizard (its mode
/// step in `setup-mode`).
pub const SCENARIOS: &[&str] = &[
    "launcher",
    "picker",
    "add-repo",
    "new-thread",
    "new-thread-base",
    "settings",
    "menu",
    "menu-status",
    "drag",
    "repos",
    "board",
    "board-drag",
    "task",
    "task-menu",
    "column-menu",
    "task-nested",
    "nest",
    "status",
    "setup",
    "setup-mode",
];

fn sample_attachment(id: &str, name: &str, (width, height): (u32, u32)) -> v1::Attachment {
    v1::Attachment {
        id: id.into(),
        name: name.into(),
        media_type: if width > 0 {
            "image/png"
        } else {
            "application/pdf"
        }
        .into(),
        size: 48_213,
        width,
        height,
        ..Default::default()
    }
}

/// A thumbnail standing in for one the daemon would send: a soft gradient.
fn sample_thumbnail() -> Option<std::sync::Arc<urushi_ui::ImageData>> {
    let (w, h) = (96u32, 64u32);
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            #[allow(clippy::cast_possible_truncation, reason = "values under 256")]
            rgba.extend([(60 + x) as u8, (90 + y) as u8, 200, 255]);
        }
    }
    urushi_ui::ImageData::new(1 << 63, w, h, rgba).map(std::sync::Arc::new)
}

/// The scenarios that show the task board.
const BOARD_SCENARIOS: &[&str] = &[
    "board",
    "board-drag",
    "task",
    "task-menu",
    "column-menu",
    "task-nested",
    "nest",
];

/// Settings with Claude in plan mode as the task agent.
fn sample_settings() -> v1::Settings {
    v1::Settings {
        branch_prefix: "urushi/".into(),
        agents: Some(v1::AgentSettings {
            task_agent: v1::Agent::Claude.into(),
            task_mode: v1::AgentMode::Plan.into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The setup wizard on its first step (`setup`) or its mode step (`setup-mode`).
fn setup_wizard(name: &str) -> crate::state::Dialog {
    use crate::task_agent::SetupStep;
    crate::state::Dialog::Setup {
        step: if name == "setup" {
            SetupStep::Agent
        } else {
            SetupStep::Mode
        },
        agent: urushi_core::Agent::Claude,
        mode: urushi_core::AgentMode::Plan,
    }
}

/// A subtask title far wider than the task editor, which must be cut short to fit.
const LONG_SUBTASK: &str = "Publish the pricing page to every regional site once legal has \
     signed off on the tiers table and the translations are back";

/// A board with a few tasks, for the `board` and `task` scenarios.
fn sample_board() -> v1::Board {
    let task = |id: &str, title: &str, notes: &str| v1::Task {
        id: id.into(),
        title: title.into(),
        notes: notes.into(),
        ..Default::default()
    };
    let sub = |id: &str, title: &str, done: bool, thread: &str| v1::Subtask {
        id: id.into(),
        title: title.into(),
        done,
        thread_id: thread.into(),
        ..Default::default()
    };
    let mut pricing = task("k3", "Pricing page copy", "Waiting on legal\nthen ship");
    pricing.thread_id = "t2".into();
    pricing.color = v1::LabelColor::Orange.into();
    pricing.tags = vec!["copy".into(), "legal".into()];
    pricing.attachments = vec![
        sample_attachment("a1", "tiers-mockup.png", (1200, 800)),
        sample_attachment("a2", "legal-notes.pdf", (0, 0)),
    ];
    let mut invoice = task("k1", "Invoice PDF export", "Use the new template");
    invoice.color = v1::LabelColor::Blue.into();
    invoice.tags = vec!["backend".into()];
    pricing.subtasks = vec![
        sub("s1", "Draft tiers table", true, ""),
        v1::Subtask {
            notes: "Send the tiers table to legal.".into(),
            tags: vec!["legal".into()],
            subtasks: vec![
                sub("s4", "EU terms", false, ""),
                sub("s5", "US terms", true, ""),
            ],
            ..sub("s2", "Legal review", false, "t6")
        },
        sub("s3", LONG_SUBTASK, false, ""),
    ];
    let column = |id: &str, name: &str, tasks: Vec<v1::Task>| v1::BoardColumn {
        id: id.into(),
        name: name.into(),
        tasks,
        ..Default::default()
    };
    v1::Board {
        id: "b1".into(),
        name: "Billing launch".into(),
        created_at_ms: 0,
        columns: vec![
            column(
                "c1",
                "To do",
                vec![invoice, task("k2", "Retry failed webhooks", "")],
            ),
            v1::BoardColumn {
                color: v1::LabelColor::Teal.into(),
                ..column("c2", "In progress", vec![pricing])
            },
            column("c3", "Done", vec![task("k4", "Stripe sandbox keys", "")]),
        ],
    }
}

const TAB: &str = "00000000-0000-0000-0000-0000000000a1";
const PANE: &str = "00000000-0000-0000-0000-0000000000b1";

fn sample_thread(
    id: &str,
    repo: &str,
    title: &str,
    folder: &str,
    status: &str,
    pinned: bool,
) -> urushi_client::v1::Thread {
    urushi_client::v1::Thread {
        id: id.into(),
        project_id: repo.into(),
        title: title.into(),
        primary_branch: format!("urushi/{id}"),
        folder_id: folder.into(),
        status: status.into(),
        pinned,
        state: urushi_client::v1::ThreadState::Ready.into(),
        ..Default::default()
    }
}

/// The scene of a sample workspace (two repos, two projects, threads with pins and
/// statuses, Claude and Codex installed) showing the screen `name`, or `None` for an
/// unknown name.
pub fn scenario(
    name: &str,
    theme: &Theme,
    text: &mut TextSystem,
    width: f32,
    height: f32,
) -> Option<Scene> {
    scenario_frame(name, theme, text, width, height).map(|f| f.scene)
}

/// [`scenario`]'s whole frame (hit regions, accessibility nodes), for layout checks.
fn scenario_frame(
    name: &str,
    theme: &Theme,
    text: &mut TextSystem,
    width: f32,
    height: f32,
) -> Option<urushi_ui::Frame<Msg>> {
    if !SCENARIOS.contains(&name) {
        return None;
    }
    let cell: CellMetrics = text.cell_metrics(theme.terminal_font_size);
    let mut state = AppState::new(ConnectTarget::Local(None), cell);
    state.compact = width < 700.0;
    let repo = |id: &str, name: &str| v1::Project {
        id: id.into(),
        name: name.into(),
        repo_root: format!("/Users/me/Dev/{name}"),
        ..Default::default()
    };
    let folder = |id: &str, name: &str| v1::Folder {
        id: id.into(),
        name: name.into(),
        ..Default::default()
    };
    state.update(Msg::Connected {
        welcome: v1::Welcome::default(),
        workspace: v1::Workspace {
            projects: vec![repo("p", "urushi"), repo("w", "website")],
            folders: vec![folder("f1", "Billing launch"), folder("f2", "Onboarding")],
            threads: vec![
                sample_thread("t", "p", "Primary", "f1", "", true),
                v1::Thread {
                    tab_ids: vec![TAB.into()],
                    ..sample_thread("t2", "p", "Invoice API", "f1", "In review", false)
                },
                sample_thread("t3", "w", "Pricing page", "f1", "In review", false),
                sample_thread("t4", "w", "Checkout copy", "f1", "Blocked", false),
                sample_thread("t5", "p", "Welcome emails", "f2", "", false),
                sample_thread("t6", "w", "Fix typo", "", "", false),
            ],
            tabs: vec![v1::Tab {
                id: TAB.into(),
                thread_id: "t2".into(),
                title: "Claude".into(),
                layout: Some(v1::LayoutNode {
                    kind: Some(v1::layout_node::Kind::PaneId(PANE.into())),
                }),
                focused_pane_id: PANE.into(),
            }],
            boards: vec![sample_board(), {
                let mut b = sample_board();
                b.id = "b2".into();
                b.name = "Personal".into();
                b
            }],
            ..Default::default()
        },
    });
    // A daemon as new as the app (agents need protocol 1.1).
    state.daemon_minor = urushi_proto::PROTOCOL_MINOR;
    state.agents = vec![urushi_core::Agent::Claude, urushi_core::Agent::Codex];
    state.settings = Some(sample_settings());
    let branches = v1::Branches {
        branches: vec!["main".into(), "dev".into(), "release/1.2".into()],
        current: "main".into(),
    };
    match name {
        // Phones show the thread list first; open the thread like a tap would.
        "launcher" => drop(state.update(Msg::SelectThread("t".into()))),
        "picker" => drop(state.update(Msg::NewTab)),
        "add-repo" => drop(state.update(Msg::OpenAddProject)),
        "new-thread" | "new-thread-base" => {
            state.update(Msg::OpenNewThread {
                repo: Some("p".into()),
                project: Some("f1".into()),
            });
            state.branches = Some(("p".into(), branches));
            state.update(Msg::Paste("Fix Login Bug".into()));
            if name == "new-thread-base" {
                state.update(Msg::ToggleSelect(Select::Base));
            }
        }
        "settings" => drop(state.update(Msg::OpenSettings)),
        "setup" | "setup-mode" => state.dialog = Some(setup_wizard(name)),
        "menu" | "menu-status" => {
            state.update(Msg::Pointer(Point::new(150.0, 230.0)));
            state.update(Msg::OpenMenu(MenuTarget::Thread("t2".into())));
            if name == "menu-status" {
                state.update(Msg::MenuPage(MenuPage::Status));
            }
        }
        "drag" => {
            state.update(Msg::Pointer(Point::new(120.0, 470.0)));
            state.update(Msg::DragThread("t6".into()));
            state.update(Msg::DragMove(Point::new(140.0, 400.0)));
        }
        name if BOARD_SCENARIOS.contains(&name) || name == "status" => {
            show_board_or_usage(&mut state, name);
        }
        _ => drop(state.update(Msg::SidebarTab(SidebarTab::Repos))),
    }
    let root = crate::view::view(&state, theme, &HashMap::new());
    let pointer = state.dragging.as_ref().map(|(_, p)| *p);
    Some(build(
        &root,
        Rect::new(0.0, 0.0, width, height),
        text,
        pointer,
    ))
}

/// The `board`, `task` and `status` scenarios.
fn show_board_or_usage(state: &mut AppState, name: &str) {
    match name {
        name if BOARD_SCENARIOS.contains(&name) => {
            state.update(Msg::ToggleBoard);
            state.update(Msg::BoardReveal(1.0));
            if let Some(thumbnail) = sample_thumbnail() {
                state.board.thumbnails.images.insert("a1".into(), thumbnail);
            }
            if name == "task-menu" {
                state.update(Msg::Pointer(urushi_ui::Point::new(700.0, 150.0)));
                state.update(Msg::OpenMenu(MenuTarget::Task("k3".into())));
            }
            if name == "task-nested" {
                state.update(Msg::Board(crate::msg::BoardMsg::OpenTask("s2".into())));
            }
            if name == "nest" {
                state.update(Msg::Board(crate::msg::BoardMsg::OpenNest("k1".into())));
            }
            if name == "column-menu" {
                state.update(Msg::Pointer(urushi_ui::Point::new(760.0, 110.0)));
                state.update(Msg::OpenMenu(MenuTarget::Column("c2".into())));
            }
            if name == "task" {
                state.update(Msg::PressTask("k3".into()));
                state.update(Msg::PressEnded);
            }
            if name == "board-drag" {
                // "Invoice PDF export" held over "Pricing page copy".
                state.update(Msg::Pointer(urushi_ui::Point::new(330.0, 140.0)));
                state.update(Msg::DragTask("k1".into()));
                state.update(Msg::DragMove(urushi_ui::Point::new(700.0, 150.0)));
                state.update(Msg::DragOver(Some(Box::new(Msg::DropTask {
                    column: "c2".into(),
                    before: Some("k3".into()),
                }))));
            }
        }
        "status" => {
            state.update(Msg::SelectThread("t2".into()));
            state.screens.insert(PANE.into(), std::sync::Arc::default());
            state.now_ms = 1_000_000;
            let window = |label: &str, used: f64, in_ms: i64| v1::UsageWindow {
                label: label.into(),
                used_percent: used,
                resets_at_ms: 1_000_000 + in_ms,
            };
            state.usage = Some((
                PANE.into(),
                v1::AgentUsage {
                    agent: v1::Agent::Claude.into(),
                    plan: "Max 20x".into(),
                    windows: vec![
                        window("5h", 41.0, 2 * 3_600_000 + 15 * 60_000),
                        window("Week", 76.0, 3 * 86_400_000 + 5 * 3_600_000),
                    ],
                    unavailable: String::new(),
                    fetched_at_ms: 1_000_000,
                },
            ));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
