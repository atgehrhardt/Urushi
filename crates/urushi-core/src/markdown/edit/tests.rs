use super::*;

/// Applies `edit` and draws the selection into the result: `[` anchor, `|` caret, or
/// `|` alone for a caret.
fn shown(text: &str, edit: &Edit) -> String {
    let mut out = edit.apply(text);
    let (anchor, caret) = edit.selection;
    if anchor == caret {
        out.insert(caret, '|');
    } else {
        let (a, c) = (anchor.min(caret), anchor.max(caret));
        out.insert(c, ']');
        out.insert(a, '[');
    }
    out
}

/// The byte range of `part` in `text` (its first occurrence).
fn at(text: &str, part: &str) -> Range<usize> {
    let i = text.find(part).unwrap();
    i..i + part.len()
}

#[test]
fn wrapping_hugs_the_text_and_selects_it() {
    let text = "make this bold";
    assert_eq!(
        shown(text, &toggle_wrap(text, at(text, " this "), "**")),
        "make **[this]** bold"
    );
    assert_eq!(shown("ab", &toggle_wrap("ab", 1..1, "_")), "a_|_b");
    assert_eq!(shown("a  b", &toggle_wrap("a  b", 1..3, "~~")), "a  ~~|~~b");
}

#[test]
fn wrapping_twice_gives_the_text_back() {
    for delim in ["**", "_", "~~", "`"] {
        let text = "one two three";
        let first = toggle_wrap(text, at(text, "two"), delim);
        let wrapped = first.apply(text);
        let (a, c) = first.selection;
        let back = toggle_wrap(&wrapped, a..c, delim);
        assert_eq!(back.apply(&wrapped), text, "{delim}");
        assert_eq!(back.selection, (4, 7));
    }
}

#[test]
fn a_selection_that_includes_the_marks_unwraps() {
    let text = "a **b** c";
    assert_eq!(
        shown(text, &toggle_wrap(text, at(text, "**b**"), "**")),
        "a [b] c"
    );
    // An empty pair inserted by mistake goes away again.
    assert_eq!(shown("a****b", &toggle_wrap("a****b", 3..3, "**")), "a|b");
}

#[test]
fn a_prefix_goes_on_every_selected_line_and_comes_off_again() {
    let text = "one\ntwo\n\nthree";
    let all = 0..text.len();
    let numbered = toggle_prefix(text, all.clone(), Prefix::Numbered);
    assert_eq!(numbered.apply(text), "1. one\n2. two\n\n3. three");
    let again = numbered.apply(text);
    let (a, c) = numbered.selection;
    assert_eq!(
        toggle_prefix(&again, a..c, Prefix::Numbered).apply(&again),
        text
    );
    assert_eq!(
        toggle_prefix(text, all, Prefix::Task).apply(text),
        "- [ ] one\n- [ ] two\n\n- [ ] three"
    );
}

#[test]
fn a_prefix_replaces_another_and_keeps_the_caret_on_its_text() {
    let text = "- [x] done\nnext";
    assert_eq!(
        shown(text, &toggle_prefix(text, 8..8, Prefix::Bullet)),
        "- do|ne\nnext"
    );
    assert_eq!(
        shown(text, &toggle_prefix(text, 1..1, Prefix::Heading)),
        "## |done\nnext"
    );
    assert_eq!(shown("", &toggle_prefix("", 0..0, Prefix::Bullet)), "- |");
    let heading = "  ## not a heading";
    assert_eq!(
        toggle_prefix(heading, 0..0, Prefix::Heading).apply(heading),
        "## ## not a heading"
    );
}

#[test]
fn a_selection_ending_at_a_line_start_leaves_that_line_alone() {
    let text = "one\ntwo";
    let edit = toggle_prefix(text, 0..4, Prefix::Bullet);
    assert_eq!(edit.apply(text), "- one\ntwo");
    assert_eq!(edit.selection, (2, 6));
}

#[test]
fn links_take_the_selection_as_label_or_address() {
    let text = "see docs here";
    assert_eq!(
        shown(text, &link(text, at(text, "docs"))),
        "see [docs]([url]) here"
    );
    let text = "go https://x.y now";
    assert_eq!(
        shown(text, &link(text, at(text, "https://x.y"))),
        "go [|](https://x.y) now"
    );
    assert_eq!(shown("", &link("", 0..0)), "[[text]](url)");
}

#[test]
fn enter_continues_lists_and_quotes() {
    let cases = [
        ("- one", "- one\n- |"),
        ("* one", "* one\n* |"),
        ("  - [x] one", "  - [x] one\n  - [ ] |"),
        ("9. nine", "9. nine\n10. |"),
        ("> said", "> said\n> |"),
    ];
    for (text, want) in cases {
        let edit = continue_list(text, text.len()).unwrap();
        assert_eq!(shown(text, &edit), want, "{text:?}");
    }
    // The rest of the line moves to the new item.
    let edit = continue_list("- onetwo", 5).unwrap();
    assert_eq!(shown("- onetwo", &edit), "- one\n- |two");
}

#[test]
fn enter_on_an_empty_item_ends_the_list() {
    let text = "- one\n- ";
    assert_eq!(
        shown(text, &continue_list(text, text.len()).unwrap()),
        "- one\n|"
    );
    let text = "1. one\n2. ";
    assert_eq!(
        shown(text, &continue_list(text, text.len()).unwrap()),
        "1. one\n|"
    );
}

#[test]
fn enter_elsewhere_is_just_a_new_line() {
    assert_eq!(continue_list("plain", 5), None);
    assert_eq!(continue_list("## Heading", 10), None);
    // In the marker itself.
    assert_eq!(continue_list("- one", 1), None);
}
