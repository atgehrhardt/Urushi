//! The status bar under the terminals: the thread's branch on the left and, when an
//! agent CLI runs in the focused pane, its plan usage on the right (e.g. Claude's
//! 5-hour and weekly windows).

use urushi_client::v1;
use urushi_ui::{Align, Color, Font, Insets, Length, Node, Role, Theme};

use crate::msg::Msg;
use crate::state::AppState;

/// Height of the bar.
pub(crate) const HEIGHT: f32 = 26.0;
/// Width of a usage meter.
const METER: f32 = 56.0;

pub(super) fn view(state: &AppState, theme: &Theme) -> Node<Msg> {
    let branch = state
        .selected_thread
        .as_ref()
        .and_then(|t| state.workspace.thread(t))
        .map(|t| t.primary_branch.clone())
        .unwrap_or_default();
    let mut bar = Node::row()
        .width(Length::Fill(1.0))
        .height(Length::Fixed(HEIGHT))
        .padding(Insets::symmetric(0.0, 12.0))
        .gap(10.0)
        .align(Align::Center)
        .child(small(theme, &format!("⎇ {branch}"), theme.text_muted).width(Length::Fill(1.0)));
    if let Some(usage) = state.shown_usage() {
        bar = bar.children(usage_items(theme, usage, state.now_ms));
    }
    Node::column()
        .width(Length::Fill(1.0))
        .child(urushi_ui::widgets::divider(theme, false))
        .child(bar)
}

fn small(theme: &Theme, text: &str, color: Color) -> Node<Msg> {
    Node::text(text, Font::Ui, theme.font_size_small, color)
}

/// "Claude · Max 20x", then a meter and percentage per window.
fn usage_items(theme: &Theme, usage: &v1::AgentUsage, now_ms: i64) -> Vec<Node<Msg>> {
    let name = match usage.agent() {
        v1::Agent::Claude => "Claude",
        v1::Agent::Codex => "Codex",
        v1::Agent::Cursor => "Cursor",
        v1::Agent::Unspecified => return Vec::new(),
    };
    let heading = if usage.plan.is_empty() {
        name.to_owned()
    } else {
        format!("{name} · {}", usage.plan)
    };
    let mut items = vec![
        Node::text(heading, Font::UiMedium, theme.font_size_small, theme.text)
            .role(Role::Label, format!("{name} plan usage")),
    ];
    if !usage.unavailable.is_empty() {
        items.push(small(theme, &usage.unavailable, theme.text_muted));
        return items;
    }
    for w in &usage.windows {
        let color = level_color(theme, w.used_percent);
        let resets = if w.resets_at_ms > now_ms && now_ms > 0 {
            format!(" · {}", until(w.resets_at_ms - now_ms))
        } else {
            String::new()
        };
        let percent = format!("{:.0}%", w.used_percent);
        let description = format!("{}: {percent} used{resets}", w.label);
        items.push(
            Node::row()
                .gap(6.0)
                .align(Align::Center)
                .role(Role::Label, description)
                .child(small(theme, &w.label, theme.text_muted))
                .child(meter(theme, w.used_percent, color))
                .child(Node::text(
                    percent,
                    Font::UiMedium,
                    theme.font_size_small,
                    color,
                ))
                .child(small(theme, &resets, theme.text_muted)),
        );
    }
    items
}

fn meter(theme: &Theme, percent: f64, color: Color) -> Node<Msg> {
    #[allow(clippy::cast_possible_truncation, reason = "a percentage")]
    let fill = (percent.clamp(0.0, 100.0) / 100.0) as f32 * METER;
    Node::row()
        .width(Length::Fixed(METER))
        .height(Length::Fixed(6.0))
        .radius(3.0)
        .background(theme.border)
        .child(
            Node::new()
                .width(Length::Fixed(fill.max(if percent > 0.0 {
                    3.0
                } else {
                    0.0
                })))
                .height(Length::Fixed(6.0))
                .radius(3.0)
                .background(color),
        )
}

/// Green-ish below 70%, amber to 90%, red above.
fn level_color(theme: &Theme, percent: f64) -> Color {
    if percent >= 90.0 {
        theme.danger
    } else if percent >= 70.0 {
        theme.warning
    } else {
        theme.accent
    }
}

/// A duration as "resets in 2d 4h" / "3h 12m" / "8m".
pub(crate) fn until(ms: i64) -> String {
    let minutes = (ms + 59_999) / 60_000;
    let (days, hours, mins) = (minutes / 1_440, minutes % 1_440 / 60, minutes % 60);
    if days > 0 {
        format!("resets in {days}d {hours}h")
    } else if hours > 0 {
        format!("resets in {hours}h {mins}m")
    } else {
        format!("resets in {mins}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_times_read_naturally() {
        assert_eq!(until(90_000), "resets in 2m");
        assert_eq!(until(3 * 3_600_000 + 12 * 60_000), "resets in 3h 12m");
        assert_eq!(until(2 * 86_400_000 + 4 * 3_600_000), "resets in 2d 4h");
    }
}
