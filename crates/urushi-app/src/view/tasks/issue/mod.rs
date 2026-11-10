//! The task editor's Jira side in full (protocol 1.13): the issue's status, watching,
//! and every field Jira lets the account edit (`fields.rs`, on the right), and its
//! comments, files, links and work log (`tabs.rs`, under the notes). Until the issue
//! arrives, the board's read-only summary shows instead (`super::jira`).

use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, TextSystem, Theme, key};

use crate::msg::Msg;

mod fields;
mod pickers;
mod tabs;

pub(super) use fields::{create_section, section};
pub(super) use tabs::tabs;

/// A small text button for a row ("Edit", "Open", "×").
fn small(theme: &Theme, text: &str, color: Color, label: &str, on_click: Msg) -> Node<Msg> {
    Node::row()
        .padding(Insets::symmetric(2.0, 6.0))
        .radius(4.0)
        .hover_background(theme.hover)
        .key(key(&["issue-action", label]))
        .on_click(on_click)
        .role(Role::Button, label)
        .child(Node::text(
            text,
            Font::UiMedium,
            theme.font_size_small,
            color,
        ))
}

/// Text that wraps to `width`, as tall as it needs (up to `max_lines`).
fn paragraph(theme: &Theme, text: &str, width: f32, max_lines: usize) -> Node<Msg> {
    let per_line = (width / (theme.font_size * 0.52)).max(8.0);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an estimate of a few lines"
    )]
    let lines: usize = text
        .lines()
        .map(|l| ((l.chars().count() as f32 / per_line).ceil() as usize).max(1))
        .sum::<usize>()
        .clamp(1, max_lines);
    #[allow(clippy::cast_precision_loss, reason = "a few lines")]
    let height = lines as f32 * TextSystem::line_height(theme.font_size);
    Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(height))
        .child(urushi_ui::widgets::wrapped_text(
            text,
            Font::Ui,
            theme.font_size,
            theme.text,
        ))
}

/// A row of a person, a time and maybe actions, over a comment or work entry.
fn byline(theme: &Theme, who: &str, when: &str) -> Node<Msg> {
    Node::row()
        .gap(8.0)
        .align(Align::Center)
        .child(Node::text(
            who,
            Font::UiSemibold,
            theme.font_size_small,
            theme.text,
        ))
        .child(Node::text(
            when,
            Font::Ui,
            theme.font_size_small,
            theme.text_muted,
        ))
}

/// A day as `2026-11-09`, from milliseconds since the epoch.
fn day(ms: i64) -> String {
    if ms <= 0 {
        return String::new();
    }
    let days = ms.div_euclid(86_400_000);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A size in bytes, as people read it.
fn size(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss, reason = "shown rounded")]
    let b = bytes as f64;
    if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else if b >= 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn days_and_sizes_read_well() {
        assert_eq!(super::day(1_794_219_630_123), "2026-11-09");
        assert_eq!(super::day(0), "");
        assert_eq!(super::size(512), "512 B");
        assert_eq!(super::size(2048), "2 KB");
        assert_eq!(super::size(3 * 1024 * 1024), "3.0 MB");
    }
}
