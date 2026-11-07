//! The sidebar: Projects → Threads, plus connection and tailnet status.

use urushi_client::v1;
use urushi_ui::widgets::icon_button;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use super::sidebar_scroll_key;
use crate::msg::Msg;
use crate::state::{AppState, Connection};

pub(super) fn view(state: &AppState, theme: &Theme, scroll: f32, width: Length) -> Node<Msg> {
    Node::column()
        .width(width)
        .height(Length::Fill(1.0))
        .background(theme.sidebar)
        .child(header(state, theme))
        .child(section_title(theme))
        .child(
            Node::column()
                .width(Length::Fill(1.0))
                .height(Length::Fill(1.0))
                .padding(Insets::symmetric(0.0, 8.0))
                .gap(2.0)
                .scroll(sidebar_scroll_key(), scroll)
                .children(projects(state, theme)),
        )
        .child(footer(state, theme))
}

fn header(state: &AppState, theme: &Theme) -> Node<Msg> {
    let (dot, status) = match &state.connection {
        Connection::Connected { host } => (theme.success, host.clone()),
        Connection::Connecting => (theme.warning, "connecting…".to_owned()),
        Connection::Disconnected(_) => (theme.danger, "disconnected".to_owned()),
    };
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(48.0))
        .padding(Insets::symmetric(0.0, 16.0))
        .gap(8.0)
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

fn section_title(theme: &Theme) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets {
            top: 4.0,
            right: 12.0,
            bottom: 6.0,
            left: 16.0,
        })
        .child(Node::text(
            "PROJECTS",
            Font::UiMedium,
            theme.font_size_small - 1.0,
            theme.text_muted,
        ))
        .child(Node::spacer())
        .child(icon_button(theme, "+", "Add project", Msg::OpenAddProject))
}

fn projects(state: &AppState, theme: &Theme) -> Vec<Node<Msg>> {
    let mut out = Vec::new();
    if state.workspace.projects().is_empty()
        && matches!(state.connection, Connection::Connected { .. })
    {
        out.push(
            Node::column()
                .padding(Insets::all(8.0))
                .gap(8.0)
                .child(Node::text(
                    "No projects yet.",
                    Font::Ui,
                    theme.font_size,
                    theme.text_muted,
                ))
                .child(urushi_ui::widgets::button(
                    theme,
                    "Add a repository",
                    urushi_ui::widgets::ButtonKind::Secondary,
                    Msg::OpenAddProject,
                )),
        );
    }
    for project in state.workspace.projects() {
        out.push(
            Node::row()
                .width(Length::Fill(1.0))
                .height(Length::Fixed(30.0))
                .padding(Insets {
                    top: 6.0,
                    right: 4.0,
                    bottom: 0.0,
                    left: 8.0,
                })
                .child(
                    Node::text(&project.name, Font::UiSemibold, theme.font_size, theme.text)
                        .width(Length::Fill(1.0)),
                )
                .child(icon_button(
                    theme,
                    "+",
                    &format!("New thread in {}", project.name),
                    Msg::OpenNewThread(project.id.clone()),
                )),
        );
        let threads: Vec<&v1::Thread> = state.workspace.threads_of(&project.id).collect();
        if threads.is_empty() {
            out.push(
                Node::row()
                    .padding(Insets {
                        top: 2.0,
                        right: 8.0,
                        bottom: 6.0,
                        left: 12.0,
                    })
                    .child(Node::text(
                        "No threads",
                        Font::Ui,
                        theme.font_size_small,
                        theme.text_muted,
                    )),
            );
        }
        for thread in threads {
            out.push(thread_row(state, theme, thread));
        }
    }
    out
}

fn thread_row(state: &AppState, theme: &Theme, thread: &v1::Thread) -> Node<Msg> {
    let selected = state.selected_thread.as_deref() == Some(&thread.id);
    let badge = match v1::ThreadState::try_from(thread.state) {
        Ok(v1::ThreadState::Ready) => None,
        Ok(v1::ThreadState::DeletionFailed) => Some(("deletion failed", theme.danger)),
        Ok(v1::ThreadState::Deleting) => Some(("deleting", theme.warning)),
        _ => Some(("creating", theme.warning)),
    };
    let mut details = Node::column()
        .width(Length::Fill(1.0))
        .gap(1.0)
        .child(Node::text(
            &thread.title,
            Font::UiMedium,
            theme.font_size,
            if selected {
                theme.text
            } else {
                theme.text.mix(theme.text_muted, 0.3)
            },
        ))
        .child(Node::text(
            &thread.primary_branch,
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ));
    if let Some((label, color)) = badge {
        details = details.child(Node::text(
            label,
            Font::UiMedium,
            theme.font_size_small,
            color,
        ));
    }
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets {
            top: 6.0,
            right: 4.0,
            bottom: 6.0,
            left: 12.0,
        })
        .gap(4.0)
        .radius(theme.radius)
        .background(if selected {
            theme.selected
        } else {
            Color::TRANSPARENT
        })
        .hover_background(if selected {
            theme.selected
        } else {
            theme.hover
        })
        .key(key(&["thread", &thread.id]))
        .on_click(Msg::SelectThread(thread.id.clone()))
        .role(Role::Item, format!("Thread {}", thread.title))
        .child(details)
        .child(icon_button(
            theme,
            "×",
            &format!("Delete thread {}", thread.title),
            Msg::RequestDeleteThread(thread.id.clone()),
        ))
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
