//! Formatted notes (UR-72, ADR 0058): the editor's toolbar and shortcuts write Markdown,
//! Enter continues lists, the color is picked and saved with the notes (not with older
//! daemons), and cards show the first line formatted and in the notes' color.

use urushi_client::v1::{self, request::Body};
use urushi_ui::{Role, Theme};

use super::{attached, bodies, key, named, workspace};
use crate::keys::{Mods, Named};
use crate::msg::{Msg, NoteFormat, ThreadMsg};
use crate::organize::NOTES_COLOR_SINCE_MINOR;
use crate::state::{AppState, Dialog};

const APPLE: bool = cfg!(any(target_os = "macos", target_os = "ios"));

/// Cmd on Apple platforms, Ctrl elsewhere; with Shift when `shift`.
fn command(shift: bool) -> Mods {
    Mods {
        logo: APPLE,
        ctrl: !APPLE,
        shift,
        ..Mods::default()
    }
}

/// A workspace with thread `t` holding `notes` in wire `color`, on a daemon that keeps
/// the notes' color.
fn with_notes(notes: &str, color: i32) -> AppState {
    let mut s = attached();
    s.daemon_minor = NOTES_COLOR_SINCE_MINOR;
    let mut w = workspace();
    let repo = w.projects[0].id.clone();
    w.threads.push(v1::Thread {
        id: "t".into(),
        project_id: repo,
        title: "Formatted".into(),
        notes: notes.into(),
        notes_color: color,
        state: v1::ThreadState::Ready.into(),
        ..Default::default()
    });
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(w)));
    s
}

/// The editor for `t`, holding `text` with `anchor..caret` selected.
fn editing(text: &str, (anchor, caret): (usize, usize)) -> AppState {
    let mut s = with_notes("", 0);
    s.update(Msg::Thread(ThreadMsg::OpenNotes("t".into())));
    if let Some(Dialog::ThreadNotes { notes, .. }) = &mut s.dialog {
        notes.set(text);
        notes.select(anchor, caret);
    }
    s
}

fn notes(s: &AppState) -> (String, Option<(usize, usize)>, usize) {
    match &s.dialog {
        Some(Dialog::ThreadNotes { notes, .. }) => {
            (notes.text().to_owned(), notes.selection(), notes.caret())
        }
        other => panic!("{other:?}"),
    }
}

fn render(s: &AppState) -> urushi_ui::Frame<Msg> {
    let mut text = urushi_ui::TextSystem::bundled_only();
    let root = crate::view::view(s, &Theme::dark(), &std::collections::HashMap::new());
    urushi_ui::build(
        &root,
        urushi_ui::Rect::new(0.0, 0.0, 1200.0, 800.0),
        &mut text,
        None,
    )
}

fn labels(s: &AppState, role: Role) -> Vec<String> {
    render(s)
        .access
        .into_iter()
        .filter(|a| a.role == role)
        .map(|a| a.label)
        .collect()
}

#[test]
fn the_toolbar_offers_every_format() {
    let s = editing("", (0, 0));
    let buttons = labels(&s, Role::Button);
    for name in [
        "Bold",
        "Italic",
        "Strikethrough",
        "Code",
        "Heading",
        "Bulleted list",
        "Numbered list",
        "Checklist",
        "Link",
    ] {
        assert!(
            buttons.iter().any(|b| b.starts_with(name)),
            "{name}: {buttons:?}"
        );
    }
}

#[test]
fn each_control_writes_its_markdown_around_the_selection() {
    let cases = [
        (NoteFormat::Bold, "**ship** it"),
        (NoteFormat::Italic, "_ship_ it"),
        (NoteFormat::Strike, "~~ship~~ it"),
        (NoteFormat::Code, "`ship` it"),
        (NoteFormat::Heading, "## ship it"),
        (NoteFormat::Bullet, "- ship it"),
        (NoteFormat::Numbered, "1. ship it"),
        (NoteFormat::Task, "- [ ] ship it"),
        (NoteFormat::Link, "[ship](url) it"),
    ];
    for (format, want) in cases {
        let mut s = editing("ship it", (0, 4));
        s.update(Msg::Thread(ThreadMsg::FormatNotes(format)));
        assert_eq!(notes(&s).0, want, "{format:?}");
    }
    // Again undoes it, and one undo takes the whole edit back.
    let mut s = editing("ship it", (0, 4));
    s.update(Msg::Thread(ThreadMsg::FormatNotes(NoteFormat::Bold)));
    assert_eq!(notes(&s).1, Some((2, 6)), "the word stays selected");
    s.update(Msg::Thread(ThreadMsg::FormatNotes(NoteFormat::Bold)));
    assert_eq!(notes(&s).0, "ship it");
    s.update(Msg::Thread(ThreadMsg::FormatNotes(NoteFormat::Bold)));
    s.update(Msg::Key(key("z", command(false))));
    assert_eq!(notes(&s).0, "ship it");
}

#[test]
fn shortcuts_format_the_notes() {
    let mut s = editing("ship it", (0, 4));
    s.update(Msg::Key(key("b", command(false))));
    assert_eq!(notes(&s).0, "**ship** it");
    s.update(Msg::Key(key("i", command(false))));
    assert_eq!(notes(&s).0, "**_ship_** it");
    let mut s = editing("ship it", (5, 7));
    s.update(Msg::Key(key("X", command(true))));
    assert_eq!(notes(&s).0, "ship ~~it~~");
    s.update(Msg::Key(key("e", command(false))));
    assert_eq!(notes(&s).0, "ship ~~`it`~~");
    let mut s = editing("docs", (0, 4));
    s.update(Msg::Key(key("k", command(false))));
    assert_eq!(notes(&s).0, "[docs](url)");
    // Cut still works: Cmd+X without Shift isn't strikethrough.
    let mut s = editing("ship it", (0, 5));
    s.update(Msg::Key(key("x", command(false))));
    assert_eq!(notes(&s).0, "it");
}

#[test]
fn enter_continues_a_list_and_ends_it_on_an_empty_item() {
    let mut s = editing("- [ ] one", (9, 9));
    s.update(Msg::Key(named(Named::Enter)));
    assert_eq!(notes(&s).0, "- [ ] one\n- [ ] ");
    s.update(Msg::Key(named(Named::Enter)));
    assert_eq!(notes(&s).0, "- [ ] one\n");
    s.update(Msg::Key(named(Named::Enter)));
    assert_eq!(
        notes(&s).0,
        "- [ ] one\n\n",
        "an ordinary new line after that"
    );
    assert!(s.dialog.is_some(), "Enter never saves the notes");
}

#[test]
fn the_color_is_picked_and_saved_with_the_notes() {
    let teal = v1::LabelColor::Teal as i32;
    let mut s = with_notes("hi", teal);
    s.update(Msg::Thread(ThreadMsg::OpenNotes("t".into())));
    assert!(matches!(&s.dialog, Some(Dialog::ThreadNotes { color, .. }) if *color == teal));
    assert!(
        labels(&s, Role::Item).iter().any(|l| l == "Color Teal"),
        "{:?}",
        labels(&s, Role::Item)
    );
    let purple = v1::LabelColor::Purple as i32;
    s.update(Msg::Thread(ThreadMsg::NotesColor(purple)));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::UpdateThread(r)] if r.notes.as_deref() == Some("hi")
                && r.notes_color == Some(purple)
        ),
        "{fx:?}"
    );
}

#[test]
fn older_daemons_get_no_color_but_formatting_still_works() {
    let mut s = with_notes("hi", 0);
    s.daemon_minor = NOTES_COLOR_SINCE_MINOR - 1;
    s.update(Msg::Thread(ThreadMsg::OpenNotes("t".into())));
    assert!(
        !labels(&s, Role::Item)
            .iter()
            .any(|l| l.starts_with("Color ")),
        "{:?}",
        labels(&s, Role::Item)
    );
    if let Some(Dialog::ThreadNotes { notes, .. }) = &mut s.dialog {
        notes.select(0, 2);
    }
    s.update(Msg::Thread(ThreadMsg::FormatNotes(NoteFormat::Bold)));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::UpdateThread(r)] if r.notes.as_deref() == Some("**hi**")
                && r.notes_color.is_none()
        ),
        "{fx:?}"
    );
}

#[test]
fn the_editor_draws_the_source_styled() {
    let s = editing("**ship** it", (0, 0));
    let dump = render(&s).scene.dump();
    let theme = Theme::dark();
    assert!(
        dump.contains(&format!(
            "\"**\" ui-semibold 13 {}",
            theme.text_muted.to_hex()
        )),
        "{dump}"
    );
    assert!(
        dump.contains(&format!("\"ship\" ui-semibold 13 {}", theme.text.to_hex())),
        "{dump}"
    );
}

/// Whether `dump` draws `text` in `font` and `color` (at any size).
fn drawn(dump: &str, (text, font): (&str, &str), color: &str) -> bool {
    dump.lines()
        .any(|l| l.contains(&format!("\"{text}\" {font} ")) && l.ends_with(color))
}

#[test]
fn cards_show_the_first_line_formatted_in_the_notes_color() {
    let theme = Theme::dark();
    let s = with_notes("\n- [ ] **Ask** about _retries_\nthen ship", 0);
    let dump = render(&s).scene.dump();
    let yellow = theme.labels[urushi_core::LabelColor::Yellow.index()].to_hex();
    assert!(drawn(&dump, ("Ask", "ui-semibold"), &yellow), "{dump}");
    assert!(drawn(&dump, ("retries", "ui-italic"), &yellow), "{dump}");
    assert!(
        !dump.contains("**Ask**") && !dump.contains("then ship"),
        "{dump}"
    );
    // The task's box, drawn before the line.
    assert!(
        dump.contains(&format!("9x9 #00000000 r2 border 1 {yellow}")),
        "{dump}"
    );

    let s = with_notes("Ship it", v1::LabelColor::Blue as i32);
    let dump = render(&s).scene.dump();
    let blue = theme.labels[urushi_core::LabelColor::Blue.index()].to_hex();
    assert!(drawn(&dump, ("Ship it", "ui"), &blue), "{dump}");
    // The note mark beside the title takes the color too.
    assert!(dump.contains(&format!("border 1 {blue}")), "{dump}");
}
