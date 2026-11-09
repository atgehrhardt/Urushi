//! Task cards: all the same size, with the title, two lines of notes and a footer
//! (tags, subtask progress, the thread chip); a colored task has a stripe and a tint. The copy that follows the pointer while dragging
//! is the same card.

use urushi_client::v1;
use urushi_ui::widgets;
use urushi_ui::{Color, Font, Insets, Length, Node, Point, Role, TextSystem, Theme, key};

use super::super::labels;
use super::{CARD_PADDING, CARD_ROW_GAP, FOOTER_HEIGHT, GHOST_WIDTH, NOTES_LINES, shorten};
use crate::msg::Msg;
use crate::organize::MenuTarget;
use crate::state::{AppState, TaskLink};

/// Longest thread title shown whole in a card's thread chip.
const CHIP_CHARS: usize = 22;
/// Longest tag shown whole on a card.
const TAG_CHARS: usize = 14;
/// Most tags a card shows.
const FOOTER_TAGS: usize = 3;
/// Width of a colored card's stripe.
const STRIPE: f32 = 4.0;
/// How much a card's color tints its background.
const TINT: f32 = 0.1;

/// The height of every card, whatever it holds: the title, two lines of notes and the
/// footer, so a board reads as a grid.
fn card_height(theme: &Theme) -> f32 {
    2.0 * CARD_PADDING
        + TextSystem::line_height(theme.font_size)
        + 2.0 * CARD_ROW_GAP
        + NOTES_LINES * TextSystem::line_height(theme.font_size_small)
        + FOOTER_HEIGHT
}

pub(super) fn card(state: &AppState, theme: &Theme, task: &v1::Task) -> Node<Msg> {
    let dragged = state
        .board
        .dragging
        .as_ref()
        .is_some_and(|(id, _)| *id == task.id);
    let mut node = card_body(state, theme, task, dragged, true)
        .background(if dragged {
            Color::TRANSPARENT
        } else {
            card_background(theme, task)
        })
        .border(
            1.0,
            if dragged {
                theme.accent.with_alpha(0.5)
            } else {
                theme.border
            },
        )
        .key(key(&["task", &task.id]))
        .on_click(Msg::PressTask(task.id.clone()))
        .on_drag(Msg::DragTask(task.id.clone()))
        .on_secondary_click(Msg::OpenMenu(MenuTarget::Task(task.id.clone())))
        .role(Role::Button, format!("Task {}", task.title));
    if state.board.dragging.is_none() {
        node = node.hover_background(theme.hover);
    }
    node
}

/// What a card shows, at the fixed card size: the title, the notes wrapped to fit (with
/// an ellipsis when there's more), and the footer at the bottom. `dragged` fades it
/// (the dragged card's place stays as a faint outline); without `interactive` (the copy
/// under the pointer) the thread chip isn't a button.
fn card_body(
    state: &AppState,
    theme: &Theme,
    task: &v1::Task,
    dragged: bool,
    interactive: bool,
) -> Node<Msg> {
    let fade = |c: Color| if dragged { c.with_alpha(0.35) } else { c };
    // Blank lines would waste the little room a card has.
    let notes: Vec<&str> = task
        .notes
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let height = card_height(theme);
    let content = Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(height))
        .padding(Insets::symmetric(CARD_PADDING, 10.0))
        .gap(CARD_ROW_GAP)
        .child(Node::text(
            &task.title,
            Font::UiMedium,
            theme.font_size,
            fade(theme.text),
        ))
        .child(widgets::wrapped_text(
            &notes.join("\n"),
            Font::Ui,
            theme.font_size_small,
            fade(theme.text_muted),
        ))
        .child(card_footer(state, theme, task, dragged, interactive));
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(height))
        .radius(theme.radius)
        .clip();
    // A colored task has a stripe down its left edge.
    if let Some(c) = labels::color(theme, task.color) {
        row = row.child(
            Node::new()
                .width(Length::Fixed(STRIPE))
                .height(Length::Fixed(height))
                .background(fade(c)),
        );
    }
    row.child(content)
}

/// The card's background: the surface, tinted with the task's color if it has one.
fn card_background(theme: &Theme, task: &v1::Task) -> Color {
    labels::color(theme, task.color).map_or(theme.surface, |c| theme.surface.mix(c, TINT))
}

/// The task's tags as small chips, at most [`FOOTER_TAGS`] and then "+n".
fn footer_tags(theme: &Theme, task: &v1::Task, dragged: bool) -> Vec<Node<Msg>> {
    let size = theme.font_size_small - 1.0;
    let mut out: Vec<Node<Msg>> = task
        .tags
        .iter()
        .take(FOOTER_TAGS)
        .map(|t| {
            let chip = labels::tag_chip(theme, &shorten(t, TAG_CHARS), size, None);
            if dragged {
                chip.background(Color::TRANSPARENT)
            } else {
                chip
            }
        })
        .collect();
    if task.tags.len() > FOOTER_TAGS {
        out.push(Node::text(
            format!("+{}", task.tags.len() - FOOTER_TAGS),
            Font::UiMedium,
            size,
            theme.text_muted,
        ));
    }
    out
}

/// Tags, subtask progress ("✓ 1/3") and the task's thread, if it has any; an empty row
/// otherwise, so every card is the same height. The thread chip goes to the thread.
fn card_footer(
    state: &AppState,
    theme: &Theme,
    task: &v1::Task,
    dragged: bool,
    interactive: bool,
) -> Node<Msg> {
    let link = TaskLink {
        task: task.id.clone(),
        subtask: None,
    };
    let thread = state.linked_thread(&link);
    let muted = if dragged {
        theme.text_muted.with_alpha(0.35)
    } else {
        theme.text_muted
    };
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(FOOTER_HEIGHT))
        .gap(6.0)
        .children(footer_tags(theme, task, dragged));
    if !task.subtasks.is_empty() {
        let done = task.subtasks.iter().filter(|s| s.done).count();
        let all = done == task.subtasks.len();
        row = row
            .child(Node::text(
                "✓",
                Font::UiSemibold,
                theme.font_size_small,
                if all && !dragged {
                    theme.success
                } else {
                    muted
                },
            ))
            .child(Node::text(
                format!("{done}/{}", task.subtasks.len()),
                Font::UiMedium,
                theme.font_size_small,
                muted,
            ));
    }
    if let Some(thread) = thread {
        let mut chip = Node::row()
            .padding(Insets::symmetric(1.0, 6.0))
            .radius(4.0)
            .background(theme.accent.with_alpha(0.16))
            .child(Node::text(
                format!("→ {}", shorten(&thread.title, CHIP_CHARS)),
                Font::UiMedium,
                theme.font_size_small - 0.5,
                if dragged { muted } else { theme.accent },
            ));
        if interactive {
            chip = chip
                .hover_background(theme.accent.with_alpha(0.3))
                .key(key(&["task-thread", &task.id]))
                .on_click(Msg::StartThread(link))
                .role(Role::Button, format!("Go to thread {}", thread.title));
        }
        row = row.child(Node::spacer()).child(chip);
    }
    row
}

/// A raised copy of the dragged card, under the pointer.
pub(in crate::view) fn drag_ghost(state: &AppState, theme: &Theme) -> Option<Node<Msg>> {
    let (task, at) = state.board.dragging.as_ref()?;
    let (_, _, task) = state.workspace.task(task)?;
    let card = card_body(state, theme, task, false, false)
        .background(card_background(theme, task).mix(theme.hover, 0.5))
        .border(1.5, theme.accent);
    // A soft dark rim stands in for a drop shadow.
    Some(
        Node::column()
            .width(Length::Fixed(GHOST_WIDTH))
            .padding(Insets {
                top: 1.0,
                right: 4.0,
                bottom: 5.0,
                left: 2.0,
            })
            .radius(theme.radius + 3.0)
            .background(Color::hex(0x000000).with_alpha(0.35))
            .child(card)
            .at(Point::new(at.x - 24.0, at.y - 14.0)),
    )
}
