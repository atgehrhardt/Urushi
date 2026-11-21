//! How a thread's agents look (ADR 0017, ADR 0039): a green dot when an agent finished,
//! a pulsing blue one while it works, an amber one when something waits for the user.
//! Shown on thread rows, tabs, task cards (a badge, and a colored border for attention)
//! and the thread chips and subtask rows that lead to them. Thread rows also count the
//! shells and monitors their agents keep open in the background (ADR 0059).

use urushi_core::{BackgroundTasks, LabelColor};
use urushi_ui::{Align, Color, Font, Insets, Node, Theme};

use super::labels::dot;
use crate::msg::Msg;
use crate::running::Indicator;
use crate::state::AppState;

/// Size of the dot on thread rows and tabs.
pub(super) const DOT: f32 = 8.0;
/// How faint a running dot gets at the low point of its pulse.
const PULSE_FLOOR: f32 = 0.3;

/// The color for `indicator`.
pub(super) fn color(theme: &Theme, indicator: Indicator) -> Color {
    match indicator {
        Indicator::Done => theme.success,
        Indicator::Running => theme.labels[LabelColor::Blue.index()],
        Indicator::Waiting => theme.warning,
    }
}

/// A short word for `indicator`, shown under a thread's title and read out by screen
/// readers.
pub(super) fn label(indicator: Indicator) -> &'static str {
    match indicator {
        Indicator::Done => "finished",
        Indicator::Running => "running",
        Indicator::Waiting => "needs input",
    }
}

/// The background tasks as words joined by `separator` ("2 shells · 1 monitor"), or
/// `None` when nothing is open.
pub(super) fn background_label(tasks: BackgroundTasks, separator: &str) -> Option<String> {
    let count = |n: u32, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    let parts: Vec<String> = [
        count(tasks.shells, "shell", "shells"),
        count(tasks.monitors, "monitor", "monitors"),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join(separator))
}

/// The color of `indicator`'s dot right now: a running one pulses.
fn dot_color(state: &AppState, theme: &Theme, indicator: Indicator) -> Color {
    let c = color(theme, indicator);
    match indicator {
        Indicator::Running => c.with_alpha(PULSE_FLOOR + (1.0 - PULSE_FLOOR) * state.pulse),
        Indicator::Done | Indicator::Waiting => c,
    }
}

/// A pill with a dot and a word ("Finished", "Running", "Needs input"), for task cards.
pub(super) fn badge(state: &AppState, theme: &Theme, indicator: Indicator) -> Node<Msg> {
    let c = color(theme, indicator);
    let word = match indicator {
        Indicator::Done => "Finished",
        Indicator::Running => "Running",
        Indicator::Waiting => "Needs input",
    };
    Node::row()
        .padding(Insets::symmetric(1.0, 6.0))
        .gap(4.0)
        .align(Align::Center)
        .radius(4.0)
        .background(c.with_alpha(0.16))
        .child(dot(theme, Some(dot_color(state, theme, indicator)), 6.0))
        .child(Node::text(
            word,
            Font::UiMedium,
            theme.font_size_small - 0.5,
            c,
        ))
}

/// A dot for `indicator`.
pub(super) fn indicator(
    state: &AppState,
    theme: &Theme,
    indicator: Indicator,
    size: f32,
) -> Node<Msg> {
    dot(theme, Some(dot_color(state, theme, indicator)), size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_labels_count_shells_then_monitors() {
        let label =
            |shells, monitors| background_label(BackgroundTasks { shells, monitors }, " · ");
        assert_eq!(label(0, 0), None);
        assert_eq!(label(1, 0).as_deref(), Some("1 shell"));
        assert_eq!(label(0, 1).as_deref(), Some("1 monitor"));
        assert_eq!(label(2, 3).as_deref(), Some("2 shells · 3 monitors"));
    }
}
