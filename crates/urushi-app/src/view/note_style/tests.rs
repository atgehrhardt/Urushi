use super::*;

fn theme() -> Theme {
    Theme::dark()
}

/// Each run as `(text, font, color)`.
fn shown<'a>(text: &'a str, runs: &[Run]) -> Vec<(&'a str, Font, Option<Color>)> {
    runs.iter()
        .map(|r| (&text[r.range.clone()], r.style.font, r.style.color))
        .collect()
}

#[test]
fn the_editor_dims_marks_and_styles_what_they_mark() {
    let t = theme();
    let text = "## Plan\n- **ship** it\n[docs](https://x.y)";
    let runs = editor_runs(&t, text);
    let muted = Some(t.text_muted);
    assert_eq!(
        shown(text, &runs),
        vec![
            ("## ", Font::UiSemibold, muted),
            ("Plan", Font::UiSemibold, None),
            ("- ", Font::Ui, muted),
            ("**", Font::UiSemibold, muted),
            ("ship", Font::UiSemibold, None),
            ("**", Font::UiSemibold, muted),
            ("[", Font::Ui, muted),
            ("docs", Font::Ui, Some(t.accent)),
            ("](https://x.y)", Font::Ui, muted),
        ]
    );
    let link = runs
        .iter()
        .find(|r| &text[r.range.clone()] == "docs")
        .unwrap();
    assert!(link.style.underline);
    // Plain text has no runs: it draws as it always has.
    assert!(editor_runs(&t, "just words").is_empty());
}

#[test]
fn done_tasks_and_struck_text_are_struck_through() {
    let t = theme();
    let text = "- [x] old\n~~gone~~";
    let runs = editor_runs(&t, text);
    let struck: Vec<&str> = runs
        .iter()
        .filter(|r| r.style.strike)
        .map(|r| &text[r.range.clone()])
        .collect();
    assert_eq!(struck, vec!["old", "gone"]);
    let italic = editor_runs(&t, "_it_ and **_both_**");
    assert!(italic.iter().any(|r| r.style.font == Font::UiItalic));
    assert!(
        italic
            .iter()
            .any(|r| r.style.font == Font::UiSemiboldItalic)
    );
    assert!(
        editor_runs(&t, "`x`")
            .iter()
            .all(|r| r.style.font == Font::MONO)
    );
}

#[test]
fn a_preview_drops_the_marks_and_colors_everything() {
    let red = Color::hex(0xff0000);
    let p = preview("\n  \n- **Ask** about _retries_\nmore", red).unwrap();
    assert_eq!(p.text, "• Ask about retries");
    assert_eq!(p.kind, BlockKind::Bullet);
    assert!(p.runs.iter().all(|r| r.style.color == Some(red)));
    assert_eq!(
        shown(&p.text, &p.runs),
        vec![
            ("• ", Font::Ui, Some(red)),
            ("Ask", Font::UiSemibold, Some(red)),
            (" about ", Font::Ui, Some(red)),
            ("retries", Font::UiItalic, Some(red)),
        ]
    );
    assert_eq!(preview("3. third", red).unwrap().text, "3. third");
    assert_eq!(preview("  spaced  ", red).unwrap().text, "spaced");
    let task = preview("- [x] shipped", red).unwrap();
    assert_eq!(
        (task.text.as_str(), task.kind),
        ("shipped", BlockKind::Task { done: true })
    );
    assert!(task.runs[0].style.strike);
    assert_eq!(
        preview("## Heading", red).unwrap().runs[0].style.font,
        Font::UiSemibold
    );
}

#[test]
fn notes_with_nothing_to_show_have_no_preview() {
    let red = Color::hex(0xff0000);
    assert_eq!(preview("", red), None);
    assert_eq!(preview(" \n\t", red), None);
    assert_eq!(preview("- ", red), None);
    assert_eq!(preview("- [ ]", red), None);
}

#[test]
fn notes_without_a_color_are_yellow() {
    let t = theme();
    let yellow = t.labels[LabelColor::Yellow.index()];
    assert_eq!(note_color(&t, 0), yellow);
    assert_eq!(note_color(&t, 999), yellow);
    let teal = urushi_proto::convert::color_to_proto(Some(LabelColor::Teal));
    assert_eq!(note_color(&t, teal), t.labels[LabelColor::Teal.index()]);
    assert_eq!(
        shown_color(0),
        urushi_proto::convert::color_to_proto(Some(LabelColor::Yellow))
    );
    assert_eq!(shown_color(teal), teal);
}
