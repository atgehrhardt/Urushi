//! The task board on a window wider than a phone but too narrow for its columns, such as
//! an unfolded Galaxy Fold: columns keep a readable width and the board scrolls sideways
//! (UR-58).

use urushi_ui::Axis;

use super::*;
use crate::view::BOARD_COLUMN_MIN_WIDTH;

fn frame(name: &str, width: f32) -> urushi_ui::Frame<Msg> {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    scenario_frame(name, &theme, &mut text, width, 820.0).unwrap()
}

/// The sideways scrolling row of columns (the tallest sideways row: the tabs and filter
/// chips scroll sideways too).
fn columns_row(frame: &urushi_ui::Frame<Msg>) -> Option<urushi_ui::ScrollRegion> {
    frame
        .scroll_regions
        .iter()
        .filter(|r| r.axis == Axis::Horizontal)
        .max_by(|a, b| a.rect.height.total_cmp(&b.rect.height))
        .copied()
}

fn card_widths(frame: &urushi_ui::Frame<Msg>) -> Vec<f32> {
    frame
        .access
        .iter()
        .filter(|a| a.label.starts_with("Task "))
        .map(|a| a.rect.width)
        .collect()
}

#[test]
fn columns_keep_their_width_and_the_board_scrolls_sideways_when_narrow() {
    for name in ["board", "jira-board"] {
        let frame = frame(name, 760.0);
        let region = columns_row(&frame).unwrap_or_else(|| panic!("{name}: no sideways scrolling"));
        assert!(region.max_offset() > 0.0, "{name}: {region:?}");
        // A card is its column's width less the column's padding.
        let min = BOARD_COLUMN_MIN_WIDTH - 20.0;
        let widths = card_widths(&frame);
        assert!(!widths.is_empty(), "{name}");
        // Cards past the edge are clipped, so only the first column's are whole.
        assert!(widths[0] >= min, "{name}: {widths:?}");
    }
}

#[test]
fn columns_share_a_wide_window_without_scrolling() {
    let frame = frame("board", 1600.0);
    let region = columns_row(&frame).unwrap();
    assert!(region.max_offset() < 0.5, "{region:?}");
    assert!(
        card_widths(&frame)
            .iter()
            .all(|w| *w > BOARD_COLUMN_MIN_WIDTH),
        "they grow past the minimum"
    );
}

#[test]
fn a_narrow_panels_header_puts_the_actions_behind_a_menu_and_keeps_close_on_screen() {
    for name in ["board", "jira-board", "jira-filters"] {
        let frame = frame(name, 760.0);
        for label in ["Close tasks (Esc)", "Board actions"] {
            let a = frame
                .access
                .iter()
                .find(|a| a.label == label)
                .unwrap_or_else(|| panic!("{name}: {label} missing"));
            assert!(
                a.rect.x + a.rect.width <= 760.0,
                "{name}: {label} {:?}",
                a.rect
            );
        }
        let dump = frame.scene.dump();
        for hidden in ["\"Sync now\"", "\"Rename\"", "\"Only my issues\""] {
            assert!(!dump.contains(hidden), "{name}: {hidden} in the header");
        }
    }
    // With room, the actions stay in the header.
    let dump = frame("jira-board", 1600.0).scene.dump();
    assert!(dump.contains("\"Sync now\"") && dump.contains("\"Only my issues\""));
}
