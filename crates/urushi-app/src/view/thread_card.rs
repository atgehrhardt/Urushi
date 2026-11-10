//! A thread's card in the sidebar: its title (with a dot when its agent finished or
//! waits), the repo and branch it's on, and its lifecycle badge. Click to open it,
//! right-click for its menu, drag it onto a project to file it there, or onto another
//! thread to take that thread's place and join its group. A project's
//! parent-branch thread (ADR 0029) stays at its project's top: it can't be dragged, and
//! is closed from its menu rather than a delete button.

use urushi_client::v1;
use urushi_ui::widgets::icon_button;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use crate::msg::Msg;
use crate::organize::{Dragged, MenuTarget};
use crate::state::AppState;

use super::board::drop_line;

/// A thread: title, then which repo and branch it's on. Click to open, right-click
/// (two-finger click) for its menu, drag to move it to another project or another
/// thread's place.
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
    let card = Node::row()
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
        .role(
            Role::Item,
            format!(
                "Thread {}{}{}",
                thread.title,
                if thread.project_base {
                    ", parent branch"
                } else if thread.pinned {
                    ", pinned"
                } else {
                    ""
                },
                attention.map_or(String::new(), |k| format!(
                    ", {}",
                    super::attention::label(k)
                )),
            ),
        )
        .child(details);
    if thread.project_base {
        return card;
    }
    let card = card
        .on_drag(Msg::DragThread(thread.id.clone()))
        .child(icon_button(
            theme,
            "×",
            &format!("Delete thread {}", thread.title),
            Msg::RequestDeleteThread(thread.id.clone()),
        ));
    drop_target(state, theme, &thread.id, card)
}

/// While another thread is dragged, `card` takes it: the dragged thread takes this one's
/// place (protocol 1.21). Drawn above the project's own drop target, it wins. When this
/// is where the dragged thread would land, a line above or below shows it.
fn drop_target(state: &AppState, theme: &Theme, thread: &str, card: Node<Msg>) -> Node<Msg> {
    if !state.daemon_supports_thread_order()
        || !matches!(&state.dragging, Some((Dragged::Thread(t), _)) if t != thread)
    {
        return card;
    }
    let drop = Msg::DropOnThread(thread.to_owned());
    let card = card.on_drop(drop.clone());
    let (above, below) = drop_line_at(state, thread);
    if !above && !below {
        return card;
    }
    // The line takes the drop too, so the card shifting under the pointer to make room
    // for it doesn't lose the target.
    Node::column()
        .width(Length::Fill(1.0))
        .gap(2.0)
        .on_drop(drop)
        .children(above.then(|| drop_line(theme)))
        .child(card)
        .children(below.then(|| drop_line(theme)))
}

/// While a thread is dragged over `thread`: whether the line showing where it would land
/// goes above it (the dragged thread comes from further down the list) or below it.
fn drop_line_at(state: &AppState, thread: &str) -> (bool, bool) {
    let Some((Dragged::Thread(dragged), _)) = &state.dragging else {
        return (false, false);
    };
    if state.thread_over.as_deref() != Some(thread) {
        return (false, false);
    }
    let threads = &state.workspace.raw().threads;
    let at = |id: &str| threads.iter().position(|t| t.id == id);
    match (at(dragged), at(thread)) {
        (Some(from), Some(to)) => (to < from, to > from),
        _ => (false, false),
    }
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
/// they show the status here. A parent-branch thread says so.
fn thread_meta(state: &AppState, thread: &v1::Thread) -> String {
    let repo = state
        .workspace
        .repo(&thread.project_id)
        .map_or("unknown repo", |r| r.name.as_str());
    let mut meta = format!("{repo} · {}", thread.primary_branch);
    if thread.pinned && !thread.status.is_empty() {
        meta = format!("{} · {meta}", thread.status);
    }
    if thread.project_base {
        meta = format!("Parent branch · {meta}");
    } else if thread.pinned {
        meta = format!("Pinned · {meta}");
    }
    meta
}
