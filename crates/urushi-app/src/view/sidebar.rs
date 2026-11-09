//! The sidebar: a Projects tab (projects → status sections → threads, then ungrouped
//! threads) and a Repos tab, the Tasks button that slides out the task board, plus
//! connection and tailnet status.

use std::sync::{Arc, LazyLock};

use urushi_ui::widgets::icon_button;
use urushi_ui::{Align, Color, Font, ImageData, Insets, Length, Node, Role, Theme, key};

use super::sidebar_scroll_key;
use crate::msg::Msg;
use crate::organize::{Prompt, SidebarTab};
use crate::state::{AppState, Connection};

pub(super) fn view(state: &AppState, theme: &Theme, scroll: f32, width: Length) -> Node<Msg> {
    let list = match state.sidebar_tab {
        SidebarTab::Projects => super::projects::list(state, theme),
        SidebarTab::Repos => repos(state, theme),
    };
    Node::column()
        .width(width)
        .height(Length::Fill(1.0))
        .background(theme.sidebar)
        .child(header(state, theme))
        .child(tabs(state, theme))
        .child(tasks_button(state, theme))
        .child(
            Node::column()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .padding(Insets::symmetric(4.0, 8.0))
                .gap(2.0)
                .scroll(sidebar_scroll_key(), scroll)
                .children(list),
        )
        .child(footer(state, theme))
}

/// The logo beside the app's name: the mark from the app icon, as raw 128×128 RGBA
/// rendered by `scripts/render-icons.swift`.
static LOGO: LazyLock<Option<Arc<ImageData>>> = LazyLock::new(|| {
    let rgba = include_bytes!("../../../../assets/icon/mark-128.rgba");
    ImageData::new(1, 128, 128, rgba.to_vec()).map(Arc::new)
});

/// Logo size in the header, in logical pixels.
const LOGO_SIZE: f32 = 24.0;

fn header(state: &AppState, theme: &Theme) -> Node<Msg> {
    let (dot, status) = match &state.connection {
        Connection::Connected { host } => (theme.success, host.clone()),
        Connection::Connecting => (theme.warning, "connecting…".to_owned()),
        Connection::Disconnected(_) => (theme.danger, "disconnected".to_owned()),
    };
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(48.0))
        .padding(Insets {
            top: 0.0,
            right: 16.0,
            bottom: 0.0,
            left: 10.0,
        })
        .gap(8.0)
        .children(
            LOGO.clone().map(|logo| {
                Node::image(logo, LOGO_SIZE, LOGO_SIZE).role(Role::Label, "Urushi logo")
            }),
        )
        .child(Node::text(
            "Urushi",
            Font::UiSemibold,
            theme.font_size + 2.0,
            theme.text,
        ))
        .children(cfg!(debug_assertions).then(|| {
            // Debug builds run against the dev home; make that impossible to miss.
            Node::row()
                .padding(Insets::symmetric(1.0, 6.0))
                .radius(4.0)
                .background(theme.warning.with_alpha(0.2))
                .child(Node::text(
                    "DEV",
                    Font::UiSemibold,
                    theme.font_size_small - 1.0,
                    theme.warning,
                ))
        }))
        .child(Node::spacer())
        .child(
            Node::new()
                .width(Length::Fixed(7.0))
                .height(Length::Fixed(7.0))
                .radius(3.5)
                .background(dot),
        )
        .child(
            Node::text(status, Font::Ui, theme.font_size_small, theme.text_muted)
                .width(Length::Fixed(110.0))
                .on_click(Msg::OpenConnect)
                .role(Role::Button, "Connection"),
        )
}

/// The Projects | Repos switch, and the "+" for the shown list.
fn tabs(state: &AppState, theme: &Theme) -> Node<Msg> {
    let tab = |label: &str, which: SidebarTab| {
        let active = state.sidebar_tab == which;
        Node::row()
            .padding(Insets::symmetric(4.0, 10.0))
            .radius(theme.radius)
            .background(if active {
                theme.selected
            } else {
                Color::TRANSPARENT
            })
            .hover_background(if active { theme.selected } else { theme.hover })
            .key(key(&["sidebar-tab", label]))
            .on_click(Msg::SidebarTab(which))
            .role(Role::Item, format!("{label} tab"))
            .child(Node::text(
                label,
                Font::UiMedium,
                theme.font_size_small,
                if active { theme.text } else { theme.text_muted },
            ))
    };
    let add = match state.sidebar_tab {
        SidebarTab::Projects => icon_button(
            theme,
            "+",
            "New project",
            Msg::OpenPrompt(Prompt::NewProject(None)),
        ),
        SidebarTab::Repos => icon_button(theme, "+", "Add repo", Msg::OpenAddProject),
    };
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets {
            top: 0.0,
            right: 12.0,
            bottom: 6.0,
            left: 10.0,
        })
        .gap(4.0)
        .child(tab("Projects", SidebarTab::Projects))
        .child(tab("Repos", SidebarTab::Repos))
        .child(Node::spacer())
        .child(add)
}

/// Opens (or closes) the task-board panel. Shows how many tasks are still open.
fn tasks_button(state: &AppState, theme: &Theme) -> Node<Msg> {
    let open = state.board.open;
    let count = crate::board::open_tasks(&state.workspace);
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets {
            top: 0.0,
            right: 8.0,
            bottom: 6.0,
            left: 8.0,
        })
        .child(
            Node::row()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(30.0))
                .padding(Insets::symmetric(0.0, 10.0))
                .gap(8.0)
                .radius(theme.radius)
                .border(1.0, if open { theme.accent } else { theme.border })
                .background(if open { theme.selected } else { theme.surface })
                .hover_background(if open { theme.selected } else { theme.hover })
                .key(key(&["sidebar-tasks"]))
                .on_click(Msg::ToggleBoard)
                .role(
                    Role::Button,
                    if open { "Close tasks" } else { "Open tasks" },
                )
                .child(
                    Node::text("Tasks", Font::UiMedium, theme.font_size_small, theme.text)
                        .width(Length::Fill(1.0)),
                )
                .children((count > 0).then(|| {
                    Node::row()
                        .padding(Insets::symmetric(1.0, 7.0))
                        .radius(8.0)
                        .background(theme.hover)
                        .child(Node::text(
                            count.to_string(),
                            Font::UiMedium,
                            theme.font_size_small - 1.0,
                            theme.text_muted,
                        ))
                }))
                .child(Node::text(
                    if open { "‹" } else { "›" },
                    Font::Ui,
                    theme.font_size,
                    theme.text_muted,
                )),
        )
}

/// The Repos tab: each repo, with its path, thread count and a "+" for a new thread.
fn repos(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    if state.workspace.repos().is_empty() {
        return super::projects::no_repos(state, theme);
    }
    state
        .workspace
        .repos()
        .iter()
        .map(|repo| {
            let threads = state.workspace.threads_of_repo(&repo.id).count();
            Node::row()
                .width(Length::Fill(1.0))
                .padding(Insets {
                    top: 6.0,
                    right: 4.0,
                    bottom: 6.0,
                    left: 8.0,
                })
                .gap(4.0)
                .radius(theme.radius)
                .key(key(&["repo", &repo.id]))
                .role(Role::Item, format!("Repo {}", repo.name))
                .child(
                    Node::column()
                        .width(Length::Fill(1.0))
                        .gap(1.0)
                        .child(Node::text(
                            &repo.name,
                            Font::UiSemibold,
                            theme.font_size,
                            theme.text,
                        ))
                        .child(Node::text(
                            &repo.repo_root,
                            Font::Ui,
                            theme.font_size_small,
                            theme.text_muted,
                        ))
                        .child(Node::text(
                            match threads {
                                1 => "1 thread".to_owned(),
                                n => format!("{n} threads"),
                            },
                            Font::Ui,
                            theme.font_size_small,
                            theme.text_muted,
                        )),
                )
                .child(icon_button(
                    theme,
                    "+",
                    &format!("New thread in {}", repo.name),
                    Msg::OpenNewThread {
                        repo: Some(repo.id.clone()),
                        project: None,
                    },
                ))
        })
        .collect()
}

fn footer(state: &AppState, theme: &Theme) -> Node<Msg> {
    let (label, color) = match &state.tailnet {
        Some(t) if t.serving => (format!("Serving on {}", t.dns_name), theme.success),
        Some(t) if t.running => ("Tailnet: not serving".to_owned(), theme.text_muted),
        Some(_) => ("Tailscale unavailable".to_owned(), theme.text_muted),
        None => ("Tailnet: unknown".to_owned(), theme.text_muted),
    };
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(40.0))
        .padding(Insets::symmetric(0.0, 12.0))
        .gap(8.0)
        .align(Align::Center)
        .child(
            Node::row()
                .width(Length::Fill(1.0))
                .gap(6.0)
                .child(
                    Node::new()
                        .width(Length::Fixed(6.0))
                        .height(Length::Fixed(6.0))
                        .radius(3.0)
                        .background(color),
                )
                .child(
                    Node::text(label, Font::Ui, theme.font_size_small, theme.text_muted)
                        .width(Length::Fill(1.0)),
                )
                .on_click(Msg::OpenSettings)
                .role(Role::Button, "Tailnet status"),
        )
        .child(urushi_ui::widgets::button(
            theme,
            "Settings",
            urushi_ui::widgets::ButtonKind::Ghost,
            Msg::OpenSettings,
        ))
}
