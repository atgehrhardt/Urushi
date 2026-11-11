//! The task board at phone size (UR-58): one column at a time, full-width cards, column
//! tabs, and a header whose controls all stay on screen.

use super::*;

/// An iPhone's logical width.
const PHONE: f32 = 393.0;

fn frame(name: &str) -> urushi_ui::Frame<Msg> {
    let theme = Theme::dark();
    let mut text = TextSystem::bundled_only();
    scenario_frame(name, &theme, &mut text, PHONE, 852.0).unwrap()
}

fn rects<'a>(
    frame: &'a urushi_ui::Frame<Msg>,
    prefix: &'a str,
) -> impl Iterator<Item = (&'a str, urushi_ui::Rect)> + 'a {
    frame
        .access
        .iter()
        .filter(move |a| a.label.starts_with(prefix))
        .map(|a| (a.label.as_str(), a.rect))
}

#[test]
fn a_phone_shows_one_column_of_full_width_cards() {
    for name in ["board", "jira-board"] {
        let frame = frame(name);
        let cards: Vec<_> = rects(&frame, "Task ").map(|(_, r)| r).collect();
        assert!(!cards.is_empty(), "{name}: no cards");
        for card in &cards {
            assert!(
                card.width > PHONE - 80.0 && card.x >= 0.0 && card.x + card.width <= PHONE,
                "{name}: card not full width: {cards:?}"
            );
        }
        // One column: every card in the same place across.
        assert!(
            cards.iter().all(|c| (c.x - cards[0].x).abs() < 0.5),
            "{name}: {cards:?}"
        );
        // A tab per column, each on screen.
        let tabs: Vec<_> = rects(&frame, "Column ")
            .filter(|(l, _)| l.ends_with(" tab"))
            .collect();
        assert!(tabs.len() >= 3, "{name}: {tabs:?}");
        for (label, r) in &tabs {
            assert!(
                r.x >= 0.0 && r.x + r.width <= PHONE + 0.5,
                "{name}: {label} {r:?}"
            );
        }
    }
}

#[test]
fn a_phones_board_header_keeps_its_buttons_on_screen() {
    for name in ["board", "jira-board", "jira-filters"] {
        let frame = frame(name);
        for label in ["Close tasks (Esc)", "Board actions", "Choose a board"] {
            let (_, r) = rects(&frame, label)
                .next()
                .unwrap_or_else(|| panic!("{name}: {label} missing"));
            assert!(
                r.x >= 0.0 && r.x + r.width <= PHONE + 0.5,
                "{name}: {label} off screen: {r:?}"
            );
        }
    }
}
