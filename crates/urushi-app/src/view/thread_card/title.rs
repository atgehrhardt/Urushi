//! A thread card's text: its title (bold with a dot while unread), the marks for its
//! notes and links, the line naming its repo and branch, and its notes' first line,
//! formatted and in the notes' color (ADR 0058).

use urushi_client::{ThreadLinks, v1};
use urushi_core::BackgroundTasks;
use urushi_core::markdown::BlockKind;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme, key, rich_line};

use crate::brand_icons::{Brand, mark};
use crate::msg::Msg;
use crate::running::Indicator;
use crate::state::AppState;

/// Size of a brand mark on a card.
const MARK: f32 = 12.0;
/// Size of the unread dot.
const UNREAD_DOT: f32 = 7.0;

/// A thread's title, then the marks of its links (ADR 0036), then a dot when its
/// agent finished, works or waits (ADR 0017, ADR 0039).
pub(super) fn thread_title(
    state: &AppState,
    theme: &Theme,
    thread: &v1::Thread,
    (selected, attention): (bool, Option<Indicator>),
    links: Option<&ThreadLinks>,
) -> Node<Msg> {
    // An unread thread stands out: bold, full contrast, with a dot before it.
    let (font, color) = if thread.unread {
        (Font::UiSemibold, theme.text)
    } else if selected {
        (Font::UiMedium, theme.text)
    } else {
        (Font::UiMedium, theme.text.mix(theme.text_muted, 0.3))
    };
    Node::row()
        .width(Length::Fill(1.0))
        .gap(6.0)
        .align(Align::Center)
        .children(thread.unread.then(|| unread_dot(theme)))
        .child(Node::text(&thread.title, font, theme.font_size, color).width(Length::Fill(1.0)))
        .children((!thread.notes.is_empty()).then(|| {
            note_mark(super::super::note_style::note_color(
                theme,
                thread.notes_color,
            ))
        }))
        .children(
            links
                .into_iter()
                .flat_map(|l| link_marks(theme, &thread.id, l)),
        )
        .children(attention.map(|k| {
            super::super::attention::indicator(state, theme, k, super::super::attention::DOT)
        }))
}

/// The Jira mark for the thread's issue and the GitHub mark for its pull request. Each
/// opens its page; a click there doesn't also open the thread.
fn link_marks(theme: &Theme, thread: &str, links: &ThreadLinks) -> Vec<Node<Msg>> {
    let issue = links.issue.as_ref().map(|i| {
        let label = format!("Open {} in Jira", i.key);
        (Brand::Jira, "jira", label, i.url.clone())
    });
    let pr = links.pull_request.as_ref().map(|p| {
        let label = if p.title.is_empty() {
            format!("Open pull request {}", p.label)
        } else {
            format!("Open pull request {}: {}", p.label, p.title)
        };
        (Brand::GitHub, "pr", label, p.url.clone())
    });
    issue
        .into_iter()
        .chain(pr)
        .filter_map(|(brand, name, label, url)| {
            let image = mark(brand, theme.text_muted)?;
            Some(
                Node::row()
                    .padding(Insets::symmetric(3.0, 3.0))
                    .radius(theme.radius)
                    .hover_background(theme.hover)
                    .key(key(&["thread-link", thread, name]))
                    .on_click(Msg::OpenUrl(url))
                    .role(Role::Button, label)
                    .child(Node::image(image, MARK, MARK)),
            )
        })
        .collect()
}

/// Which repo and branch a thread is on. Pinned threads leave their status section, so
/// they show the status here. A parent-branch thread says so.
pub(super) fn thread_meta(state: &AppState, thread: &v1::Thread) -> String {
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

/// The card's accessible label: the thread, what kind it is, its agent's state, and
/// whether it is unread or has notes.
pub(super) fn card_label(
    thread: &v1::Thread,
    attention: Option<Indicator>,
    background: BackgroundTasks,
) -> String {
    let mut label = format!("Thread {}", thread.title);
    if thread.project_base {
        label.push_str(", parent branch");
    } else if thread.pinned {
        label.push_str(", pinned");
    }
    if let Some(k) = attention {
        label.push_str(", ");
        label.push_str(super::super::attention::label(k));
    }
    if let Some(tasks) = super::super::attention::background_label(background, ", ") {
        label.push_str(", ");
        label.push_str(&tasks);
    }
    if thread.unread {
        label.push_str(", unread");
    }
    if !thread.notes.is_empty() {
        label.push_str(", has notes");
    }
    label
}

/// The first line of the thread's notes with text, its Markdown drawn rather than shown,
/// in the notes' color and cut to the card's width. A task item has its box before it.
pub(super) fn notes_preview(theme: &Theme, thread: &v1::Thread) -> Option<Node<Msg>> {
    let color = super::super::note_style::note_color(theme, thread.notes_color);
    let preview = super::super::note_style::preview(&thread.notes, color)?;
    let size = theme.font_size_small;
    let task = match preview.kind {
        BlockKind::Task { done } => Some(task_box(color, done)),
        _ => None,
    };
    Some(
        Node::row()
            .width(Length::Fill(1.0))
            .gap(5.0)
            .align(Align::Center)
            .key(key(&["thread-notes", &thread.id]))
            .children(task)
            .child(rich_line(&preview.text, preview.runs, size, color)),
    )
}

/// A task's check box on a card: empty, or filled with a tick when done. Drawn, since
/// the bundled fonts have no ballot box.
fn task_box(color: Color, done: bool) -> Node<Msg> {
    let size = 9.0;
    let node = Node::row()
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .radius(2.0)
        .justify(Align::Center)
        .align(Align::Center)
        .key(key(&["task-box", if done { "done" } else { "open" }]));
    if done {
        node.background(color)
            .child(Node::text("✓", Font::UiSemibold, 8.0, Color::hex(0x000000)))
    } else {
        node.border(1.0, color)
    }
}

/// The dot before an unread thread's title.
fn unread_dot(theme: &Theme) -> Node<Msg> {
    Node::new()
        .width(Length::Fixed(UNREAD_DOT))
        .height(Length::Fixed(UNREAD_DOT))
        .radius(UNREAD_DOT / 2.0)
        .background(theme.accent)
        .key(key(&["unread-dot"]))
}

/// A small page with lines on it, in the notes' color: the thread has notes. Drawn
/// rather than a glyph, which the bundled fonts don't have.
fn note_mark(color: Color) -> Node<Msg> {
    let line = || {
        Node::new()
            .width(Length::Fill(1.0))
            .height(Length::Fixed(1.0))
            .background(color)
    };
    Node::column()
        .width(Length::Fixed(9.0))
        .height(Length::Fixed(11.0))
        .padding(Insets::symmetric(2.0, 2.0))
        .gap(2.0)
        .radius(1.5)
        .border(1.0, color)
        .key(key(&["note-mark"]))
        .children([line(), line(), line()])
}
