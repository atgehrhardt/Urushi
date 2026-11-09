//! A thread's card in the sidebar: its title (with a dot when its agent finished or
//! waits), the repo and branch it's on, and its lifecycle badge. Click to open it,
//! right-click for its menu, drag to file it under another project.

use urushi_client::v1;
use urushi_ui::widgets::icon_button;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use crate::msg::Msg;
use crate::organize::{Dragged, MenuTarget};
use crate::state::AppState;

/// A thread: title, then which repo and branch it's on. Click to open, right-click
/// (two-finger click) for its menu, drag to move it to another project.
pub(super) fn thread_card(state: &AppState, theme: &Theme, thread: &v1::Thread) -> Node<Msg> {
    let selected = state.selected_thread.as_deref() == Some(&thread.id);
    let dragged = state
        .dragging
        .as_ref()
        .is_some_and(|(d, _)| matches!(d, Dragged::Thread(t) if *t == thread.id));
    let badge = match v1::ThreadState::try_from(thread.state) {
        Ok(v1::ThreadState::Ready) => None,
        Ok(v1::ThreadState::DeletionFailed) => Some(("deletion failed", theme.danger)),
        Ok(v1::ThreadState::Deleting) => Some(("deleting", theme.warning)),
        _ => Some(("creating", theme.warning)),
    };
    let attention = state.thread_attention(&thread.id);
    let mut details = Node::column()
        .width(Length::Fill(1.0))
        .gap(1.0)
        .child(thread_title(theme, thread, selected, attention))
        .child(Node::text(
            thread_meta(state, thread),
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ));
    // The thread's lifecycle (creating, deleting) matters more than its agent.
    let badge = badge.or_else(|| {
        attention.map(|k| {
            (
                super::attention::label(k),
                super::attention::color(theme, k),
            )
        })
    });
    if let Some((label, color)) = badge {
        details = details.child(Node::text(
            label,
            Font::UiMedium,
            theme.font_size_small,
            color,
        ));
    }
    let background = if selected || dragged {
        theme.selected
    } else {
        Color::TRANSPARENT
    };
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets {
            top: 6.0,
            right: 4.0,
            bottom: 6.0,
            left: 12.0,
        })
        .gap(4.0)
        .align(Align::Center)
        .radius(theme.radius)
        .background(background)
        .hover_background(if selected {
            theme.selected
        } else {
            theme.hover
        })
        .key(key(&["thread", &thread.id]))
        .on_click(Msg::SelectThread(thread.id.clone()))
        .on_secondary_click(Msg::OpenMenu(MenuTarget::Thread(thread.id.clone())))
        .on_drag(Msg::DragThread(thread.id.clone()))
        .role(
            Role::Item,
            format!(
                "Thread {}{}{}",
                thread.title,
                if thread.pinned { ", pinned" } else { "" },
                attention.map_or(String::new(), |k| format!(
                    ", {}",
                    super::attention::label(k)
                )),
            ),
        )
        .child(details)
        .child(icon_button(
            theme,
            "×",
            &format!("Delete thread {}", thread.title),
            Msg::RequestDeleteThread(thread.id.clone()),
        ))
}

/// A thread's title, with a dot when its agent finished or waits (ADR 0017).
fn thread_title(
    theme: &Theme,
    thread: &v1::Thread,
    selected: bool,
    attention: Option<v1::AttentionKind>,
) -> Node<Msg> {
    let color = if selected {
        theme.text
    } else {
        theme.text.mix(theme.text_muted, 0.3)
    };
    Node::row()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .align(Align::Center)
        .child(
            Node::text(&thread.title, Font::UiMedium, theme.font_size, color)
                .width(Length::Fill(1.0)),
        )
        .children(attention.map(|k| super::attention::indicator(theme, k, super::attention::DOT)))
}

/// Which repo and branch a thread is on. Pinned threads leave their status section, so
/// they show the status here.
fn thread_meta(state: &AppState, thread: &v1::Thread) -> String {
    let repo = state
        .workspace
        .repo(&thread.project_id)
        .map_or("unknown repo", |r| r.name.as_str());
    let mut meta = format!("{repo} · {}", thread.primary_branch);
    if thread.pinned && !thread.status.is_empty() {
        meta = format!("{} · {meta}", thread.status);
    }
    if thread.pinned {
        meta = format!("Pinned · {meta}");
    }
    meta
}
