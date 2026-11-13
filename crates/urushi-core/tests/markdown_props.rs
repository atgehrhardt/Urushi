//! Property tests for the notes' Markdown: any text parses into spans that cover it in
//! order on character boundaries, and every formatting edit leaves valid text with a
//! selection inside it. Wrapping or prefixing twice gives the text back.
#![allow(clippy::unwrap_used, reason = "test code")]

use proptest::prelude::*;
use urushi_core::markdown::{self, Prefix, continue_list, link, toggle_prefix, toggle_wrap};

/// Text full of Markdown's special characters, some multibyte.
fn text() -> impl Strategy<Value = String> {
    "[a-cé *_~`\\[\\]()#>\\-+1.x\n]{0,40}"
}

/// A selection in `text` on character boundaries, `start <= end`.
fn selection(text: &str, a: usize, b: usize) -> std::ops::Range<usize> {
    let bounds: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    let a = bounds[a % bounds.len()];
    let b = bounds[b % bounds.len()];
    a.min(b)..a.max(b)
}

fn assert_valid(text: &str, edit: &markdown::Edit) {
    assert!(text.is_char_boundary(edit.range.start) && text.is_char_boundary(edit.range.end));
    let out = edit.apply(text);
    let (a, c) = edit.selection;
    assert!(a <= out.len() && c <= out.len(), "{out:?} {a} {c}");
    assert!(
        out.is_char_boundary(a) && out.is_char_boundary(c),
        "{out:?} {a} {c}"
    );
}

proptest! {
    #[test]
    fn spans_cover_every_line_in_order(text in text()) {
        for ((start, parsed), l) in markdown::lines(&text).zip(text.split('\n')) {
            prop_assert_eq!(&text[start..start + l.len()], l);
            let mut at = 0;
            for span in &parsed.spans {
                prop_assert_eq!(span.range.start, at);
                prop_assert!(span.range.end > span.range.start);
                prop_assert!(l.is_char_boundary(span.range.end));
                at = span.range.end;
            }
            prop_assert_eq!(at, l.len());
        }
    }

    #[test]
    fn edits_leave_valid_text_and_selections(text in text(), a in 0usize..64, b in 0usize..64) {
        let sel = selection(&text, a, b);
        for delim in ["**", "_", "~~", "`"] {
            assert_valid(&text, &toggle_wrap(&text, sel.clone(), delim));
        }
        for prefix in [Prefix::Heading, Prefix::Bullet, Prefix::Numbered, Prefix::Task] {
            assert_valid(&text, &toggle_prefix(&text, sel.clone(), prefix));
        }
        assert_valid(&text, &link(&text, sel.clone()));
        if let Some(edit) = continue_list(&text, sel.end) {
            assert_valid(&text, &edit);
        }
    }

    #[test]
    fn wrapping_plain_words_twice_gives_them_back(
        words in "[a-z]{1,6}( [a-z]{1,6}){0,4}",
        a in 0usize..64,
        b in 0usize..64,
    ) {
        let sel = selection(&words, a, b);
        prop_assume!(!words[sel.clone()].trim().is_empty());
        for delim in ["**", "_", "~~", "`"] {
            let first = toggle_wrap(&words, sel.clone(), delim);
            let wrapped = first.apply(&words);
            let (x, y) = first.selection;
            let back = toggle_wrap(&wrapped, x.min(y)..x.max(y), delim);
            prop_assert_eq!(back.apply(&wrapped), words.clone());
        }
    }

    #[test]
    fn prefixing_plain_lines_twice_gives_them_back(
        lines in "[a-z]{1,6}(\n[a-z]{0,6}){0,4}",
        a in 0usize..64,
        b in 0usize..64,
    ) {
        let sel = selection(&lines, a, b);
        for prefix in [Prefix::Heading, Prefix::Bullet, Prefix::Numbered, Prefix::Task] {
            let first = toggle_prefix(&lines, sel.clone(), prefix);
            let marked = first.apply(&lines);
            let (x, y) = first.selection;
            let back = toggle_prefix(&marked, x.min(y)..x.max(y), prefix);
            prop_assert_eq!(back.apply(&marked), lines.clone(), "{:?}", prefix);
        }
    }
}
