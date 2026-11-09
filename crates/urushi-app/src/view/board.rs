//! The task-board panel: slides out from under the sidebar over the terminals, which
//! show faintly through it. Board tabs across the top, then the board's columns of task
//! cards, all the same size. Cards open the task editor on click and move between
//! columns by drag and drop: while a card is dragged, its place stays as a faint outline, a line shows where
//! it would land, and a raised copy follows the pointer.

use urushi_client::v1;
use urushi_ui::widgets::{self, ButtonKind, button, icon_button};
use urushi_ui::{
    Align, Color, Font, Insets, Key, Length, Node, Point, Role, TextSystem, Theme, key,
};

use crate::board::BOARDS_SINCE_MINOR;
use crate::msg::Msg;
use crate::organize::Prompt;
use crate::state::{AppState, TaskLink};

/// How opaque the panel is over the terminals.
const PANEL_ALPHA: f32 = 0.86;
/// How opaque a column is (over the panel).
const COLUMN_ALPHA: f32 = 0.7;
/// Space between cards in a column.
const CARD_GAP: f32 = 6.0;
/// Width of the card that follows the pointer while dragging.
const GHOST_WIDTH: f32 = 260.0;
/// Space above and below a card's contents.
const CARD_PADDING: f32 = 8.0;
/// Space between a card's title, notes and footer.
const CARD_ROW_GAP: f32 = 3.0;
/// Lines of notes a card shows.
const NOTES_LINES: f32 = 2.0;
/// Height of a card's footer row (subtask progress, thread chip).
const FOOTER_HEIGHT: f32 = 18.0;

/// Key of a column's scrolling task list.
pub(crate) fn column_scroll_key(column: &str) -> Key {
    key(&["board-column", column])
}

/// The panel, slid out by `state.board.eased()`. Place it over the main area in a
/// clipped stack.
pub(super) fn panel(
    state: &AppState,
    theme: &Theme,
    scroll: &std::collections::HashMap<Key, f32>,
) -> Node<Msg> {
    let body = if state.daemon_minor < BOARDS_SINCE_MINOR {
        notice(
            theme,
            "Task boards need a newer daemon",
            "Restart the daemon from Settings to use them.",
            None,
        )
    } else if let Some(board) = state.board.board(&state.workspace) {
        columns(state, theme, board, scroll)
    } else {
        notice(
            theme,
            "No boards yet",
            "A board keeps tasks in columns: To do, In progress, Done.",
            Some(button(
                theme,
                "New board",
                ButtonKind::Primary,
                Msg::OpenPrompt(Prompt::NewBoard),
            )),
        )
    };
    Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .background(theme.background.with_alpha(PANEL_ALPHA))
        .border(1.0, theme.border)
        .key(key(&["board-panel"]))
        .on_click(Msg::BoardBackground)
        .slide_x(1.0 - state.board.eased())
        .child(header(state, theme))
        .child(urushi_ui::widgets::divider(theme, false))
        .child(body)
}

/// Board tabs, "+", and the shown board's actions.
fn header(state: &AppState, theme: &Theme) -> Node<Msg> {
    let shown = state.board.board(&state.workspace).map(|b| b.id.clone());
    let tabs = state.workspace.boards().iter().map(|b| {
        let active = shown.as_deref() == Some(b.id.as_str());
        Node::row()
            .padding(Insets::symmetric(4.0, 10.0))
            .radius(theme.radius)
            .background(if active {
                theme.selected
            } else {
                Color::TRANSPARENT
            })
            .hover_background(if active { theme.selected } else { theme.hover })
            .key(key(&["board-tab", &b.id]))
            .on_click(Msg::SelectBoard(b.id.clone()))
            .role(Role::Item, format!("Board {}", b.name))
            .child(Node::text(
                &b.name,
                Font::UiMedium,
                theme.font_size_small,
                if active { theme.text } else { theme.text_muted },
            ))
    });
    let mut row = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(44.0))
        .padding(Insets::symmetric(0.0, 12.0))
        .gap(4.0);
    // Phones need the room for the board tabs and actions.
    if !state.compact {
        row = row
            .child(Node::text(
                "Tasks",
                Font::UiSemibold,
                theme.font_size,
                theme.text,
            ))
            .child(Node::new().width(Length::Fixed(8.0)));
    }
    row = row.children(tabs);
    if state.daemon_minor >= BOARDS_SINCE_MINOR {
        row = row.child(icon_button(
            theme,
            "+",
            "New board",
            Msg::OpenPrompt(Prompt::NewBoard),
        ));
    }
    row = row.child(Node::spacer());
    if let Some(id) = shown {
        row = row
            .child(button(
                theme,
                "Rename",
                ButtonKind::Ghost,
                Msg::OpenPrompt(Prompt::RenameBoard(id.clone())),
            ))
            .child(button(
                theme,
                "Delete",
                ButtonKind::Ghost,
                Msg::OpenDeleteBoard(id),
            ));
    }
    row.child(icon_button(
        theme,
        "×",
        "Close tasks (Esc)",
        Msg::ToggleBoard,
    ))
}

fn notice(theme: &Theme, title: &str, detail: &str, action: Option<Node<Msg>>) -> Node<Msg> {
    Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .justify(Align::Center)
        .align(Align::Center)
        .gap(8.0)
        .child(Node::text(
            title,
            Font::UiSemibold,
            theme.font_size + 2.0,
            theme.text,
        ))
        .child(Node::text(
            detail,
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ))
        .children(action)
}

fn columns(
    state: &AppState,
    theme: &Theme,
    board: &v1::Board,
    scroll: &std::collections::HashMap<Key, f32>,
) -> Node<Msg> {
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets::symmetric(14.0, 14.0))
        .gap(12.0)
        .align(Align::Stretch)
        .children(board.columns.iter().map(|c| {
            let offset = scroll
                .get(&column_scroll_key(&c.id))
                .copied()
                .unwrap_or(0.0);
            column(state, theme, board, c, offset)
        }))
}

fn column(
    state: &AppState,
    theme: &Theme,
    board: &v1::Board,
    column: &v1::BoardColumn,
    offset: f32,
) -> Node<Msg> {
    let dragging = state.board.dragging.is_some();
    let over = state
        .board
        .over
        .as_ref()
        .filter(|(c, _)| *c == column.id)
        .map(|(_, before)| before.as_deref());
    let drop_here = Msg::DropTask {
        column: column.id.clone(),
        before: None,
    };
    let head = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(34.0))
        .padding(Insets {
            top: 0.0,
            right: 4.0,
            bottom: 0.0,
            left: 12.0,
        })
        .gap(6.0)
        .child(Node::text(
            &column.name,
            Font::UiSemibold,
            theme.font_size_small,
            theme.text,
        ))
        .child(Node::text(
            column.tasks.len().to_string(),
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ))
        .child(Node::spacer())
        .child(icon_button(
            theme,
            "+",
            &format!("New task in {}", column.name),
            Msg::NewTask {
                board: board.id.clone(),
                column: column.id.clone(),
            },
        ));
    // Each card sits in a slot that includes the space under it, and the drop line in
    // one that drops in the same place, so the pointer is always over some target and
    // the line doesn't flicker as it moves the cards.
    let mut cards: Vec<Node<Msg>> = Vec::new();
    for t in &column.tasks {
        let onto = Msg::DropTask {
            column: column.id.clone(),
            before: Some(t.id.clone()),
        };
        if over == Some(Some(t.id.as_str())) {
            cards.push(slot(dragging, onto.clone()).child(drop_line(theme)));
        }
        cards.push(slot(dragging, onto).child(card(state, theme, t)));
    }
    if over == Some(None) {
        cards.push(slot(dragging, drop_here.clone()).child(drop_line(theme)));
    }
    let cards = Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets::symmetric(2.0, 8.0))
        .scroll(column_scroll_key(&column.id), offset)
        .children(cards);
    let mut node = Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .padding(Insets {
            top: 4.0,
            right: 0.0,
            bottom: 8.0,
            left: 0.0,
        })
        .radius(theme.radius + 2.0)
        .background(theme.sidebar.with_alpha(COLUMN_ALPHA))
        .border(
            1.0,
            if over.is_some() {
                theme.accent.with_alpha(0.7)
            } else {
                theme.border
            },
        )
        .key(key(&["board-column-box", &column.id]))
        .child(head)
        .child(cards);
    if dragging {
        node = node
            .on_drop(drop_here)
            .hover_background(theme.sidebar.mix(theme.hover, 0.6).with_alpha(0.85));
    }
    node
}

/// A card's place in a column: the card and the gap under it. While a card is dragged,
/// it drops there.
fn slot(dragging: bool, drop: Msg) -> Node<Msg> {
    let node = Node::column().width(Length::Fill(1.0)).padding(Insets {
        top: 0.0,
        right: 0.0,
        bottom: CARD_GAP,
        left: 0.0,
    });
    if dragging { node.on_drop(drop) } else { node }
}

/// Where a dragged card would land.
fn drop_line(theme: &Theme) -> Node<Msg> {
    Node::new()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(3.0))
        .radius(1.5)
        .background(theme.accent)
}

/// The height of every card, whatever it holds: the title, two lines of notes and the
/// footer, so a board reads as a grid.
fn card_height(theme: &Theme) -> f32 {
    2.0 * CARD_PADDING
        + TextSystem::line_height(theme.font_size)
        + 2.0 * CARD_ROW_GAP
        + NOTES_LINES * TextSystem::line_height(theme.font_size_small)
        + FOOTER_HEIGHT
}

fn card(state: &AppState, theme: &Theme, task: &v1::Task) -> Node<Msg> {
    let dragged = state
        .board
        .dragging
        .as_ref()
        .is_some_and(|(id, _)| *id == task.id);
    let mut node = card_body(state, theme, task, dragged, true)
        .background(if dragged {
            Color::TRANSPARENT
        } else {
            theme.surface
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
    Node::column()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(card_height(theme)))
        .padding(Insets::symmetric(CARD_PADDING, 10.0))
        .gap(CARD_ROW_GAP)
        .radius(theme.radius)
        .clip()
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
        .child(card_footer(state, theme, task, dragged, interactive))
}

/// Subtask progress ("✓ 1/3") and the task's thread, if it has either; an empty row
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
        .gap(8.0);
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
                format!("→ {}", thread.title),
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
pub(super) fn drag_ghost(state: &AppState, theme: &Theme) -> Option<Node<Msg>> {
    let (task, at) = state.board.dragging.as_ref()?;
    let (_, _, task) = state.workspace.task(task)?;
    let card = card_body(state, theme, task, false, false)
        .background(theme.surface.mix(theme.hover, 0.5))
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
