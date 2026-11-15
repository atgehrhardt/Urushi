//! Inline Markdown as styled byte ranges of the source: `**bold**`, `_italic_` /
//! `*italic*`, `~~strike~~`, `` `code` `` and `[text](url)`. The rules match the
//! daemon's ADF converter, so notes and Jira descriptions read the same way.
//!
//! The spans cover every byte of the text, and the markup itself comes out as
//! `marker` spans. An editor can draw the source with the markers dimmed, and a preview
//! can leave them out.

use std::ops::Range;

/// How deep marks may nest before the rest is taken as text.
const MAX_DEPTH: usize = 8;

/// The marks on a run of text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent marks, any of which can combine"
)]
pub struct Style {
    /// `**bold**`.
    pub bold: bool,
    /// `_italic_` or `*italic*`.
    pub italic: bool,
    /// `~~struck through~~`.
    pub strike: bool,
    /// `` `code` ``.
    pub code: bool,
    /// The text of a `[link](url)`.
    pub link: bool,
}

/// A run of source text with one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Its bytes in the text passed in (offset by the `base` given).
    pub range: Range<usize>,
    /// The marks it has.
    pub style: Style,
    /// Markup rather than text: `**`, `` ` ``, `[`, `](url)`.
    pub marker: bool,
}

/// `text`'s spans, in order, covering all of it, with ranges offset by `base`.
pub fn spans(text: &str, base: usize) -> Vec<Span> {
    let mut out = Out {
        base,
        spans: Vec::new(),
    };
    walk(text, 0, Style::default(), 0, &mut out);
    out.spans
}

/// Spans being collected: adjacent runs of the same kind are merged.
struct Out {
    base: usize,
    spans: Vec<Span>,
}

impl Out {
    fn push(&mut self, range: Range<usize>, style: Style, marker: bool) {
        if range.is_empty() {
            return;
        }
        let range = range.start + self.base..range.end + self.base;
        if let Some(last) = self.spans.last_mut()
            && last.range.end == range.start
            && last.style == style
            && last.marker == marker
        {
            last.range.end = range.end;
            return;
        }
        self.spans.push(Span {
            range,
            style,
            marker,
        });
    }
}

/// A marked-up span opening somewhere in the text.
enum Mark {
    /// Code: nothing inside is marked up.
    Code,
    /// Bold, around more inline Markdown.
    Bold,
    /// Strikethrough, around more inline Markdown.
    Strike,
    /// Italic, around more inline Markdown.
    Italic,
    /// A link: its label, more inline Markdown, then `](url)`.
    Link,
}

impl Mark {
    /// `style` with this mark added.
    fn on(&self, mut style: Style) -> Style {
        match self {
            Self::Code => style.code = true,
            Self::Bold => style.bold = true,
            Self::Strike => style.strike = true,
            Self::Italic => style.italic = true,
            Self::Link => style.link = true,
        }
        style
    }
}

/// What opens at `i`: the mark, the inner text's range, and where the span ends.
fn span_at(text: &str, i: usize) -> Option<(Mark, Range<usize>, usize)> {
    let rest = &text[i..];
    if rest.starts_with('`') {
        let end = closing(text, i, "`")?;
        return Some((Mark::Code, i + 1..end, end + 1));
    }
    for (delim, mark) in [("**", Mark::Bold), ("~~", Mark::Strike)] {
        if rest.starts_with(delim) {
            let end = closing(text, i, delim)?;
            return Some((mark, i + 2..end, end + 2));
        }
    }
    if rest.starts_with('*') || (rest.starts_with('_') && word_edge(text, i, true)) {
        let delim = &rest[..1];
        let end = closing(text, i, delim)?;
        if delim == "_" && !word_edge(text, end, false) {
            return None;
        }
        return Some((Mark::Italic, i + 1..end, end + 1));
    }
    if rest.starts_with('[') {
        let (label, end) = link(text, i)?;
        return Some((Mark::Link, label, end));
    }
    None
}

fn walk(text: &str, base: usize, style: Style, depth: usize, out: &mut Out) {
    if depth > MAX_DEPTH {
        out.push(base..base + text.len(), style, false);
        return;
    }
    let mut plain = 0;
    let mut i = 0;
    while i < text.len() {
        let Some((mark, inner, end)) = span_at(text, i) else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        };
        out.push(base + plain..base + i, style, false);
        let marked = mark.on(style);
        out.push(base + i..base + inner.start, marked, true);
        if matches!(mark, Mark::Code) {
            out.push(base + inner.start..base + inner.end, marked, false);
        } else {
            walk(
                &text[inner.clone()],
                base + inner.start,
                marked,
                depth + 1,
                out,
            );
        }
        out.push(base + inner.end..base + end, marked, true);
        i = end;
        plain = end;
    }
    out.push(base + plain..base + text.len(), style, false);
}

/// Where a delimited span (`**…**`, `` `…` ``) that opens at `at` closes, if it does.
fn closing(text: &str, at: usize, delim: &str) -> Option<usize> {
    let start = at + delim.len();
    let end = start + text.get(start..)?.find(delim)?;
    // As in CommonMark, `* 3 *` isn't emphasis: the span hugs its text.
    let inner = &text[start..end];
    (end > start && inner.trim() == inner).then_some(end)
}

/// Whether `_` at `at` can open (or, with `open: false`, close) emphasis: not inside a
/// word, so `snake_case` stays text.
fn word_edge(text: &str, at: usize, open: bool) -> bool {
    let neighbour = if open {
        text[..at].chars().next_back()
    } else {
        text[at + 1..].chars().next()
    };
    neighbour.is_none_or(|c| !c.is_alphanumeric())
}

/// `[label](href)` at `at`: the label's range and where the link ends.
fn link(text: &str, at: usize) -> Option<(Range<usize>, usize)> {
    let close = at + text[at..].find("](")?;
    let end = close + 2 + text[close + 2..].find(')')?;
    let href = &text[close + 2..end];
    (close > at + 1 && !href.is_empty() && !href.contains(' ')).then_some((at + 1..close, end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spans as `(text, style letters, marker)`, for readable assertions.
    fn show(text: &str) -> Vec<(String, String, bool)> {
        spans(text, 0)
            .into_iter()
            .map(|s| {
                let st = s.style;
                let letters = [
                    (st.bold, 'b'),
                    (st.italic, 'i'),
                    (st.strike, 's'),
                    (st.code, 'c'),
                    (st.link, 'l'),
                ]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, c)| *c)
                .collect();
                (text[s.range].to_owned(), letters, s.marker)
            })
            .collect()
    }

    fn t(text: &str, style: &str, marker: bool) -> (String, String, bool) {
        (text.to_owned(), style.to_owned(), marker)
    }

    #[test]
    fn marks_and_their_markers() {
        assert_eq!(
            show("a **b** _c_ `d*e`"),
            vec![
                t("a ", "", false),
                t("**", "b", true),
                t("b", "b", false),
                t("**", "b", true),
                t(" ", "", false),
                t("_", "i", true),
                t("c", "i", false),
                t("_", "i", true),
                t(" ", "", false),
                t("`", "c", true),
                t("d*e", "c", false),
                t("`", "c", true),
            ]
        );
    }

    #[test]
    fn marks_nest_and_links_keep_their_address_as_markup() {
        assert_eq!(
            show("**_both_** [site](https://x.y) ~~gone~~"),
            vec![
                t("**", "b", true),
                t("_", "bi", true),
                t("both", "bi", false),
                t("_", "bi", true),
                t("**", "b", true),
                t(" ", "", false),
                t("[", "l", true),
                t("site", "l", false),
                t("](https://x.y)", "l", true),
                t(" ", "", false),
                t("~~", "s", true),
                t("gone", "s", false),
                t("~~", "s", true),
            ]
        );
    }

    #[test]
    fn what_isnt_markdown_stays_one_plain_span() {
        for text in [
            "snake_case and 2 * 3 and [not a link] and **open",
            "[](x) and [a](has space)",
            "** spaced **",
        ] {
            assert_eq!(show(text), vec![t(text, "", false)], "{text:?}");
        }
        assert!(spans("", 0).is_empty());
    }

    #[test]
    fn spans_cover_the_text_in_order_and_take_an_offset() {
        let text = "é **ü** `ö`";
        let s = spans(text, 10);
        assert_eq!(s.first().unwrap().range.start, 10);
        assert_eq!(s.last().unwrap().range.end, 10 + text.len());
        for w in s.windows(2) {
            assert_eq!(w[0].range.end, w[1].range.start);
        }
    }
}
