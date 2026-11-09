//! Editing model tests.

use super::*;

#[test]
fn editing_is_utf8_safe() {
    let mut f = TextField::new("héllo");
    f.backspace();
    assert_eq!(f.text(), "héll");
    f.home(false);
    f.right(false);
    f.right(false);
    f.backspace();
    assert_eq!(f.text(), "hll");
    f.insert("é\u{7}");
    assert_eq!(f.text(), "héll");
    f.end(false);
    f.delete();
    f.left(false);
    f.delete();
    assert_eq!(f.text(), "hél");
    f.place(2, false);
    assert_eq!(f.caret(), 1, "snapped back onto a character boundary");
    f.clear();
    f.backspace();
    f.left(false);
    assert_eq!((f.text(), f.caret()), ("", 0));
}

#[test]
fn pasted_line_breaks_become_spaces() {
    let mut f = TextField::default();
    f.insert("one\r\ntwo\nthree\tfour");
    assert_eq!(f.text(), "one two three four");
}

#[test]
fn typing_and_deleting_replace_the_selection() {
    let mut f = TextField::new("hello world");
    f.home(false);
    f.word_right(true);
    assert_eq!(f.selected_text(), "hello");
    f.insert("goodbye");
    assert_eq!(f.text(), "goodbye world");
    f.select_all();
    f.backspace();
    assert_eq!(f.text(), "");
    let mut f = TextField::new("abc");
    f.left(true);
    f.left(true);
    assert_eq!(f.selected_text(), "bc");
    f.delete();
    assert_eq!((f.text(), f.caret()), ("a", 1));
}

#[test]
fn moves_collapse_or_extend_the_selection() {
    let mut f = TextField::new("abcdef");
    f.place(2, false);
    f.place(4, true);
    assert_eq!(f.selection(), Some((2, 4)));
    f.left(false);
    assert_eq!(
        (f.caret(), f.selection()),
        (2, None),
        "collapses to the left end"
    );
    f.end(true);
    f.home(true);
    assert_eq!(f.selection(), Some((0, 2)), "the anchor stays put");
    f.right(false);
    assert_eq!((f.caret(), f.selection()), (2, None));
}

#[test]
fn word_moves_skip_spaces_and_stop_at_punctuation() {
    let mut f = TextField::new("fix  the-bug now");
    f.word_left(false);
    assert_eq!(f.caret(), 13);
    f.word_left(false);
    assert_eq!(f.caret(), 9, "start of `bug`");
    f.word_left(false);
    assert_eq!(f.caret(), 8, "the dash is its own run");
    f.home(false);
    f.word_right(false);
    assert_eq!(f.caret(), 3);
    f.word_right(false);
    assert_eq!(f.caret(), 8, "spaces skipped, then `the`");
    f.end(false);
    f.delete_word_back();
    assert_eq!(f.text(), "fix  the-bug ");
    f.delete_to_start();
    assert_eq!((f.text(), f.caret()), ("", 0));
}

#[test]
fn double_click_selects_a_word() {
    let mut f = TextField::new("open the door");
    f.select_word(6);
    assert_eq!(f.selected_text(), "the");
    f.select_word(13);
    assert_eq!(
        f.selected_text(),
        "door",
        "at the very end, the word before"
    );
    f.select_word(4);
    assert_eq!(f.selected_text(), " ", "a run of spaces");
}

#[test]
fn cut_undo_and_redo() {
    let mut f = TextField::new("keep this");
    f.word_left(true);
    assert_eq!(f.cut(), "this");
    assert_eq!(f.text(), "keep ");
    f.insert("that");
    assert!(f.undo());
    assert_eq!(f.text(), "keep ");
    assert!(f.undo());
    assert_eq!(f.text(), "keep this");
    assert!(!f.undo(), "nothing older: `new` isn't an edit");
    assert!(f.redo());
    assert!(f.redo());
    assert_eq!(f.text(), "keep that");
    assert!(!f.redo());
    assert_eq!(TextField::default().cut(), "");
}

#[test]
fn a_multiline_field_keeps_line_breaks() {
    let mut f = TextField::multiline("a\r\nb\rc\td");
    assert_eq!(f.text(), "a\nb\nc d");
    assert!(f.is_multiline() && f.newline());
    f.insert("e\nf");
    assert_eq!(f.text(), "a\nb\nc d\ne\nf");
    f.set("fresh");
    assert!(f.is_multiline(), "set keeps the mode");
    let mut single = TextField::new("x");
    assert!(!single.newline());
    assert_eq!(
        single.text(),
        "x",
        "Enter adds nothing to a single-line field"
    );
}

#[test]
fn multiline_moves_work_on_lines() {
    let mut f = TextField::multiline("first line\nab\nthird line");
    f.place(5, false); // "first|"
    f.down(false);
    assert_eq!(f.caret(), 13, "end of the shorter line");
    f.down(false);
    assert_eq!(f.caret(), 16, "column 2 of the third line");
    f.down(false);
    assert_eq!(f.caret(), f.text().len(), "past the last line: the end");
    f.place(16, false);
    f.up(true);
    assert_eq!(f.selected_text(), "\nth", "up extends the selection");
    f.up(false);
    assert_eq!(f.caret(), 2);
    f.up(false);
    assert_eq!(f.caret(), 0, "above the first line: the start");
    f.place(13, false);
    f.line_start(false);
    assert_eq!(f.caret(), 11);
    f.line_end(true);
    assert_eq!(f.selected_text(), "ab");
    // A single-line field: up and down go to the ends.
    let mut s = TextField::new("one two");
    s.place(3, false);
    s.up(false);
    assert_eq!(s.caret(), 0);
    s.down(false);
    assert_eq!(s.caret(), 7);
    s.line_start(false);
    assert_eq!(s.caret(), 0);
}

#[test]
fn delete_to_start_stops_at_the_line_start() {
    let mut f = TextField::multiline("keep\ngone");
    f.delete_to_start();
    assert_eq!(f.text(), "keep\n");
    f.delete_to_start();
    assert_eq!(f.text(), "keep", "at a line start, the line break goes");
    f.delete_to_start();
    f.delete_to_start();
    assert_eq!((f.text(), f.caret()), ("", 0));
}
