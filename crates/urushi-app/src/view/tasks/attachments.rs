//! The task editor's attachments: one row per file (an image's thumbnail or the file's
//! type, its name and size, ×), clicked to open it; and "Add files…" (desktop), or
//! drop files on the window. A new task gets attachments once it is saved.

use std::collections::HashMap;

use urushi_client::{TaskRef, v1};
use urushi_ui::widgets::{ButtonKind, button, icon_button};
use urushi_ui::{Align, Font, Insets, Key, Length, Node, Role, Theme, key};

use super::super::dialogs::{hint, label};
use super::super::labels::thumbnail_box;
use crate::msg::{BoardMsg, Msg};
use crate::state::AppState;

/// Rows shown before the list scrolls.
const VISIBLE: usize = 4;
/// Height of a row.
const ROW: f32 = 40.0;
/// Size of a row's thumbnail.
const THUMB: f32 = 32.0;

/// Key of the attachment list's scroll.
pub(crate) fn attachments_scroll_key() -> Key {
    key(&["task-attachments"])
}

pub(super) fn section(
    state: &AppState,
    (theme, scroll): (&Theme, &HashMap<Key, f32>),
    task: Option<TaskRef<'_>>,
) -> Vec<Node<Msg>> {
    let mut out = vec![label(theme, "Attachments")];
    let Some(task) = task else {
        out.push(hint(theme, "Add the task first, then attach files."));
        return out;
    };
    let rows: Vec<Node<Msg>> = task
        .attachments()
        .iter()
        .map(|a| row(state, theme, a))
        .collect();
    if !rows.is_empty() {
        let mut list = Node::column().width(Length::Fill(1.0));
        if rows.len() > VISIBLE {
            #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
            let height = VISIBLE as f32 * ROW;
            let offset = scroll
                .get(&attachments_scroll_key())
                .copied()
                .unwrap_or(0.0);
            list = list
                .height(Length::Fixed(height))
                .scroll(attachments_scroll_key(), offset);
        }
        out.push(list.children(rows));
    }
    let mut actions = Node::row().gap(8.0).align(Align::Center);
    if AppState::can_pick_files() {
        actions = actions
            .child(button(
                theme,
                "Add files…",
                ButtonKind::Secondary,
                Msg::Board(BoardMsg::AddFiles(task.id().to_owned())),
            ))
            .child(hint(theme, "or drop them here"));
    } else {
        actions = actions.child(hint(theme, "Attach files from a computer."));
    }
    out.push(actions);
    out
}

fn row(state: &AppState, theme: &Theme, a: &v1::Attachment) -> Node<Msg> {
    let preview = if a.width > 0 {
        thumbnail_box(theme, state.board.thumbnails.get(&a.id), THUMB)
    } else {
        // A file: its extension, in a box the size of a thumbnail.
        let ext = a
            .name
            .rsplit_once('.')
            .map_or("FILE", |(_, e)| e)
            .chars()
            .take(4)
            .collect::<String>()
            .to_uppercase();
        Node::row()
            .width(Length::Fixed(THUMB))
            .height(Length::Fixed(THUMB))
            .radius(4.0)
            .justify(Align::Center)
            .align(Align::Center)
            .background(theme.background)
            .child(Node::text(
                ext,
                Font::UiSemibold,
                theme.font_size_small - 2.0,
                theme.text_muted,
            ))
    };
    let open = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fill(1.0))
        .gap(8.0)
        .align(Align::Center)
        .key(key(&["attachment", &a.id]))
        .on_click(Msg::Board(BoardMsg::OpenAttachment(a.id.clone())))
        .role(Role::Button, format!("Open {}", a.name))
        .child(preview)
        .child(Node::text(&a.name, Font::Ui, theme.font_size, theme.text).width(Length::Fill(1.0)))
        .child(Node::text(
            size(a.size),
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ));
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(ROW))
        .padding(Insets::symmetric(0.0, 4.0))
        .gap(6.0)
        .radius(theme.radius)
        .child(open)
        .child(icon_button(
            theme,
            "×",
            &format!("Delete attachment {}", a.name),
            Msg::Board(BoardMsg::DeleteAttachment(a.id.clone())),
        ))
}

/// "812 B", "4.2 KB", "1.5 MB".
fn size(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss, reason = "sizes are shown rounded")]
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes_read_like_a_file_browser() {
        assert_eq!(super::size(812), "812 B");
        assert_eq!(super::size(4300), "4.2 KB");
        assert_eq!(super::size(1_572_864), "1.5 MB");
    }
}
