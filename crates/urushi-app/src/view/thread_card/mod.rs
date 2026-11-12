//! A thread's card in the sidebar: its title (with a dot when its agent finished, works
//! or waits, and the Jira and GitHub marks of its task's issue and pull request, which
//! open them in a browser, ADR 0036), the repo and branch it's on, the first line of its
//! notes, and its lifecycle badge. An unread thread's title is bold with a dot before
//! it, and a thread in a colored project has a stripe of that color (ADR 0048). Click to open it, right-click for its menu, drag it onto a project to file it
//! there, or onto another thread to take that thread's place and join its group. A
//! project's
//! parent-branch thread (ADR 0029) stays at its project's top: it can't be dragged, and
//! is closed from its menu rather than a delete button.

use urushi_client::{ThreadLinks, v1};
use urushi_ui::widgets::icon_button;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key};

use crate::msg::Msg;
use crate::organize::{Dragged, MenuTarget};
use crate::state::AppState;

use super::board::drop_line;

mod title;

#[cfg(test)]
mod tests;

/// Width of the stripe down a card in a colored project.
const STRIPE: f32 = 3.0;

/// A thread: title, then which repo and branch it's on. Click to open, right-click
/// (two-finger click) for its menu, drag to move it to another project or another
/// thread's place. `links` are its task's Jira issue and pull request; `stripe` is its
/// project's color.
pub(super) fn thread_card(
    state: &AppState,
    theme: &Theme,
    thread: &v1::Thread,
    links: Option<&ThreadLinks>,
    stripe: Option<Color>,
) -> Node<Msg> {
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
    let attention = state.thread_indicator(&thread.id);
    let mut details = Node::column()
        .width(Length::Fill(1.0))
        .gap(1.0)
        .child(title::thread_title(
            state,
            theme,
            thread,
            (selected, attention),
            links,
        ))
        .child(Node::text(
            title::thread_meta(state, thread),
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ))
        .children(title::notes_preview(theme, thread));
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
        .role(Role::Item, title::card_label(thread, attention))
        .child(details);
    if thread.project_base {
        return with_stripe(theme, stripe, card);
    }
    let card = card
        .on_drag(Msg::DragThread(thread.id.clone()))
        .child(icon_button(
            theme,
            "×",
            &format!("Delete thread {}", thread.title),
            Msg::RequestDeleteThread(thread.id.clone()),
        ));
    drop_target(state, theme, &thread.id, with_stripe(theme, stripe, card))
}

/// A thread the daemon is still creating (ADR 0056): its title and a "creating" badge.
/// Click to show it; it can't be dragged or deleted yet.
pub(super) fn pending_card(
    state: &AppState,
    theme: &Theme,
    pending: &crate::creations::PendingThread,
) -> Node<Msg> {
    let selected = state.shows_pending(pending.key);
    let repo = state
        .workspace
        .repo(&pending.repo)
        .map_or("", |r| r.name.as_str());
    Node::row()
        .width(Length::Fill(1.0))
        .padding(Insets {
            top: 6.0,
            right: 4.0,
            bottom: 6.0,
            left: 12.0,
        })
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
        .key(key(&["pending-thread", &pending.key.to_string()]))
        .on_click(Msg::SelectPending(pending.key))
        .role(Role::Item, format!("{}, creating", pending.title))
        .child(
            Node::column()
                .width(Length::Fill(1.0))
                .gap(1.0)
                .child(Node::text(
                    &pending.title,
                    Font::UiMedium,
                    theme.font_size,
                    theme.text,
                ))
                .child(Node::text(
                    repo,
                    Font::Ui,
                    theme.font_size_small,
                    theme.text_muted,
                ))
                .child(Node::text(
                    "creating",
                    Font::UiMedium,
                    theme.font_size_small,
                    theme.warning,
                )),
        )
}

/// `card` with a stripe of its project's color down its left edge, if it has one.
fn with_stripe(theme: &Theme, stripe: Option<Color>, card: Node<Msg>) -> Node<Msg> {
    let Some(color) = stripe else {
        return card;
    };
    Node::row()
        .width(Length::Fill(1.0))
        .radius(theme.radius)
        .clip()
        .child(
            Node::new()
                .width(Length::Fixed(STRIPE))
                .height(Length::Fill(1.0))
                .background(color)
                .key(key(&["project-stripe"])),
        )
        .child(card)
}

/// While another thread is dragged, `card` takes it: the dragged thread takes this one's
/// place (protocol 1.24). Drawn above the project's own drop target, it wins. When this
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
