//! Editing dialog text fields with shortcuts, the mouse and the context menu.

use super::{connected, key, named};
use crate::keys::{KeyPress, Mods, Named};
use crate::msg::{Effect, Msg};
use crate::organize::{MenuTarget, Prompt};
use crate::state::{AppState, Field, FieldCommand};

const APPLE: bool = cfg!(any(target_os = "macos", target_os = "ios"));

/// The editing-shortcut modifier: Cmd on Apple platforms, Ctrl elsewhere.
fn command() -> Mods {
    Mods {
        logo: APPLE,
        ctrl: !APPLE,
        ..Mods::default()
    }
}

fn with_mods(n: Named, mods: Mods) -> KeyPress {
    KeyPress { mods, ..named(n) }
}

/// A one-line prompt holding `text`, focused.
fn prompt(text: &str) -> AppState {
    let mut s = connected();
    s.update(Msg::OpenPrompt(Prompt::NewBoard));
    s.update(Msg::Paste(text.into()));
    s
}

fn text(s: &AppState) -> String {
    s.field().map(|f| f.text().to_owned()).unwrap_or_default()
}

#[test]
fn select_all_copy_cut_and_paste() {
    let mut s = prompt("hello world");
    s.update(Msg::Key(key("a", command())));
    let fx = s.update(Msg::Key(key("c", command())));
    assert!(matches!(&fx[..], [Effect::SetClipboard(t)] if t == "hello world"));
    let fx = s.update(Msg::Key(key("x", command())));
    assert!(matches!(&fx[..], [Effect::SetClipboard(t)] if t == "hello world"));
    assert_eq!(text(&s), "");
    assert!(matches!(
        &s.update(Msg::Key(key("v", command())))[..],
        [Effect::RequestPaste]
    ));
    s.update(Msg::Paste("pasted\nline".into()));
    assert_eq!(text(&s), "pasted line");
    // Nothing selected: copy and cut don't touch the clipboard.
    assert!(s.update(Msg::Key(key("c", command()))).is_empty());
    assert!(s.update(Msg::Key(key("x", command()))).is_empty());
}

#[test]
fn undo_and_redo() {
    let mut s = prompt("abc");
    s.update(Msg::Key(named(Named::Backspace)));
    assert_eq!(text(&s), "ab");
    s.update(Msg::Key(key("z", command())));
    assert_eq!(text(&s), "abc");
    let redo = Mods {
        shift: true,
        ..command()
    };
    s.update(Msg::Key(key("z", redo)));
    assert_eq!(text(&s), "ab");
}

#[test]
fn shift_selects_and_word_keys_jump_words() {
    let mut s = prompt("one two three");
    let word = Mods {
        alt: APPLE,
        ctrl: !APPLE,
        ..Mods::default()
    };
    s.update(Msg::Key(with_mods(Named::Backspace, word)));
    assert_eq!(text(&s), "one two ");
    s.update(Msg::Key(with_mods(
        Named::Left,
        Mods {
            shift: true,
            ..word
        },
    )));
    assert_eq!(
        s.field().map(|f| f.selected_text().to_owned()).as_deref(),
        Some("two ")
    );
    s.update(Msg::Text("2".into()));
    assert_eq!(text(&s), "one 2");
    // Home with Shift selects back to the start.
    s.update(Msg::Key(with_mods(
        Named::Home,
        Mods {
            shift: true,
            ..Mods::default()
        },
    )));
    s.update(Msg::Key(named(Named::Delete)));
    assert_eq!(text(&s), "");
}

#[test]
fn clicking_places_the_caret_and_double_click_selects_a_word() {
    let mut s = prompt("open the door");
    s.update(Msg::FieldPress {
        caret: 6,
        extend: false,
        clicks: 1,
    });
    assert_eq!(s.field().map(urushi_ui::TextField::caret), Some(6));
    s.update(Msg::FieldDrag(13));
    assert_eq!(
        s.field().map(|f| f.selected_text().to_owned()).as_deref(),
        Some("he door")
    );
    s.update(Msg::FieldPress {
        caret: 6,
        extend: false,
        clicks: 2,
    });
    assert_eq!(
        s.field().map(|f| f.selected_text().to_owned()).as_deref(),
        Some("the")
    );
    s.update(Msg::FieldPress {
        caret: 6,
        extend: false,
        clicks: 3,
    });
    assert_eq!(s.field().map(|f| f.selected_text().len()), Some(13));
    s.update(Msg::FieldPress {
        caret: 2,
        extend: false,
        clicks: 1,
    });
    s.update(Msg::FieldPress {
        caret: 4,
        extend: true,
        clicks: 1,
    });
    assert_eq!(
        s.field().map(|f| f.selected_text().to_owned()).as_deref(),
        Some("en")
    );
}

#[test]
fn right_click_opens_an_edit_menu_on_the_field() {
    let mut s = prompt("text");
    s.update(Msg::OpenMenu(MenuTarget::Field(Field::Prompt)));
    assert!(matches!(
        s.menu.as_ref().map(|m| &m.target),
        Some(MenuTarget::Field(Field::Prompt))
    ));
    s.update(Msg::EditField(FieldCommand::SelectAll));
    assert!(s.menu.is_none(), "choosing an item closes the menu");
    let fx = s.update(Msg::EditField(FieldCommand::Copy));
    assert!(matches!(&fx[..], [Effect::SetClipboard(t)] if t == "text"));
    // Escape closes the menu before the dialog.
    s.update(Msg::OpenMenu(MenuTarget::Field(Field::Prompt)));
    s.update(Msg::Key(named(Named::Escape)));
    assert!(s.menu.is_none() && s.dialog.is_some());
    // No dialog, no field menu.
    s.dialog = None;
    s.update(Msg::OpenMenu(MenuTarget::Field(Field::Prompt)));
    assert!(s.menu.is_none());
}

#[test]
fn shift_tab_goes_back_a_field() {
    let mut s = connected();
    s.update(Msg::OpenNewThread {
        repo: None,
        project: None,
    });
    assert_eq!(s.focus, Some(Field::ThreadTitle));
    s.update(Msg::Key(with_mods(
        Named::Tab,
        Mods {
            shift: true,
            ..Mods::default()
        },
    )));
    assert_eq!(s.focus, Some(Field::ThreadBranch));
}
