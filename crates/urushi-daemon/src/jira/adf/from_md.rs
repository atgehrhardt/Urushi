//! Markdown → ADF: the Markdown [`super::to_markdown`] writes, and what people type.
//!
//! Blocks: paragraphs (lines kept as line breaks), `#` headings, `-`/`*`/`+` and `1.`
//! lists (nested by indentation), `- [ ]` task lists, `>` quotes, fenced code blocks and
//! `---` rules. Inline: `**bold**`, `_italic_`/`*italic*`, `~~strike~~`, `` `code` `` and
//! `[text](url)`. Anything else is text.

use serde_json::{Value, json};

use super::MAX_DEPTH;

/// Markdown as an ADF document; `None` (no description) when blank.
pub fn from_markdown(text: &str) -> Option<Value> {
    if text.trim().is_empty() {
        return None;
    }
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let mut tasks = 0;
    Some(json!({ "type": "doc", "version": 1, "content": blocks(&lines, 0, &mut tasks) }))
}

/// A list line: its indentation, whether it is numbered (and from what), and its text.
struct Item<'a> {
    indent: usize,
    ordered: Option<u64>,
    text: &'a str,
}

fn list_item(line: &str) -> Option<Item<'_>> {
    let indent = line.len() - line.trim_start().len();
    let rest = line.trim_start();
    if let Some(text) = ["- ", "* ", "+ "].iter().find_map(|m| rest.strip_prefix(m)) {
        return Some(Item {
            indent,
            ordered: None,
            text,
        });
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let after = rest.get(digits..)?;
    let text = after.strip_prefix(". ")?;
    Some(Item {
        indent,
        ordered: Some(rest[..digits].parse().ok()?),
        text,
    })
    .filter(|_| digits > 0)
}

fn fence(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("```")
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    let text = line.get(hashes..)?.strip_prefix(' ')?;
    (1..=6).contains(&hashes).then_some((hashes, text))
}

fn is_rule(line: &str) -> bool {
    matches!(line.trim(), "---" | "***" | "___")
}

/// Whether `line` starts a block other than a paragraph.
fn starts_block(line: &str) -> bool {
    fence(line).is_some()
        || heading(line).is_some()
        || is_rule(line)
        || line.trim_start().starts_with('>')
        || list_item(line).is_some()
}

fn blocks(lines: &[&str], depth: usize, tasks: &mut usize) -> Vec<Value> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
        } else if let Some(language) = fence(line) {
            let end = lines[i + 1..]
                .iter()
                .position(|l| fence(l).is_some())
                .map_or(lines.len(), |p| i + 1 + p);
            let code = lines[i + 1..end].join("\n");
            let mut block = json!({ "type": "codeBlock", "content": text_nodes(&code) });
            if !language.trim().is_empty() {
                block["attrs"] = json!({ "language": language.trim() });
            }
            out.push(block);
            i = end + 1;
        } else if let Some((level, text)) = heading(line) {
            out.push(
                json!({ "type": "heading", "attrs": { "level": level }, "content": inline(text) }),
            );
            i += 1;
        } else if is_rule(line) {
            out.push(json!({ "type": "rule" }));
            i += 1;
        } else if line.trim_start().starts_with('>') {
            let end = lines[i..]
                .iter()
                .position(|l| !l.trim_start().starts_with('>'))
                .map_or(lines.len(), |p| i + p);
            let inner: Vec<&str> = lines[i..end]
                .iter()
                .map(|l| {
                    let l = l.trim_start().trim_start_matches('>');
                    l.strip_prefix(' ').unwrap_or(l)
                })
                .collect();
            if depth < MAX_DEPTH {
                out.push(
                    json!({ "type": "blockquote", "content": blocks(&inner, depth + 1, tasks) }),
                );
            }
            i = end;
        } else if list_item(line).is_some() {
            let end = lines[i..]
                .iter()
                .position(|l| l.trim().is_empty() || (list_item(l).is_none() && starts_block(l)))
                .map_or(lines.len(), |p| i + p);
            out.extend(list(&lines[i..end], depth, tasks));
            i = end;
        } else {
            let end = lines[i..]
                .iter()
                .position(|l| l.trim().is_empty() || starts_block(l))
                .map_or(lines.len(), |p| i + p);
            out.push(json!({ "type": "paragraph", "content": inline(&lines[i..end].join("\n")) }));
            i = end;
        }
    }
    out
}

/// A run of list lines (and their continuation lines) as one or more lists.
fn list(lines: &[&str], depth: usize, tasks: &mut usize) -> Vec<Value> {
    let Some(first) = lines.first().and_then(|l| list_item(l)) else {
        return Vec::new();
    };
    if depth >= MAX_DEPTH {
        return Vec::new();
    }
    // Items at this list's indentation, each with the lines under it.
    let mut items: Vec<(Item<'_>, Vec<&str>)> = Vec::new();
    for line in lines {
        match list_item(line) {
            Some(item) if item.indent <= first.indent => items.push((item, Vec::new())),
            _ => {
                if let Some((_, under)) = items.last_mut() {
                    under.push(line);
                }
            }
        }
    }
    if items
        .iter()
        .all(|(item, under)| task(item.text).is_some() && under.is_empty())
    {
        let content: Vec<Value> = items
            .iter()
            .filter_map(|(item, _)| task(item.text))
            .map(|(done, text)| {
                *tasks += 1;
                json!({
                    "type": "taskItem",
                    "attrs": { "localId": format!("task-{tasks}"), "state": if done { "DONE" } else { "TODO" } },
                    "content": inline(text),
                })
            })
            .collect();
        *tasks += 1;
        return vec![
            json!({ "type": "taskList", "attrs": { "localId": format!("list-{tasks}") }, "content": content }),
        ];
    }
    let content: Vec<Value> = items
        .iter()
        .map(|(item, under)| {
            // Lines up to its first nested item continue its paragraph.
            let split = under
                .iter()
                .position(|l| list_item(l).is_some())
                .unwrap_or(under.len());
            let (text, sub) = under.split_at(split);
            let mut paragraph = vec![item.text];
            paragraph.extend(text.iter().map(|l| l.trim()));
            let mut content =
                vec![json!({ "type": "paragraph", "content": inline(&paragraph.join("\n")) })];
            if !sub.is_empty() {
                content.extend(list(sub, depth + 1, tasks));
            }
            json!({ "type": "listItem", "content": content })
        })
        .collect();
    let mut block = if first.ordered.is_some() {
        json!({ "type": "orderedList", "content": content })
    } else {
        json!({ "type": "bulletList", "content": content })
    };
    if let Some(start) = first.ordered.filter(|s| *s != 1) {
        block["attrs"] = json!({ "order": start });
    }
    vec![block]
}

/// A task list item's text: whether it is done, and the rest.
fn task(text: &str) -> Option<(bool, &str)> {
    text.strip_prefix("[ ] ").map(|t| (false, t)).or_else(|| {
        text.strip_prefix("[x] ")
            .or_else(|| text.strip_prefix("[X] "))
            .map(|t| (true, t))
    })
}

/// Plain text as text nodes with line breaks (no marks).
fn text_nodes(text: &str) -> Vec<Value> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![json!({ "type": "text", "text": text })]
}

/// Inline Markdown as ADF text nodes with marks and line breaks.
fn inline(text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push(json!({ "type": "hardBreak" }));
        }
        spans(line, &[], 0, &mut out);
    }
    out
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

fn push_text(text: &str, marks: &[Value], out: &mut Vec<Value>) {
    if text.is_empty() {
        return;
    }
    let mut node = json!({ "type": "text", "text": text });
    if !marks.is_empty() {
        node["marks"] = Value::Array(marks.to_vec());
    }
    out.push(node);
}

/// A marked-up span starting somewhere in a line.
enum Span<'a> {
    /// `` `code` ``: nothing inside is marked up.
    Code(&'a str),
    /// Bold, italic, strikethrough or a link, around more inline Markdown.
    Marked(&'a str, Value),
}

/// The span that opens at `i`, if one does (and closes), and where it ends.
fn span_at(text: &str, i: usize) -> Option<(Span<'_>, usize)> {
    let rest = &text[i..];
    if rest.starts_with('`') {
        let end = closing(text, i, "`")?;
        return Some((Span::Code(&text[i + 1..end]), end + 1));
    }
    for (delim, mark) in [("**", "strong"), ("~~", "strike")] {
        if rest.starts_with(delim) {
            let end = closing(text, i, delim)?;
            return Some((
                Span::Marked(&text[i + 2..end], json!({ "type": mark })),
                end + 2,
            ));
        }
    }
    if rest.starts_with('*') || (rest.starts_with('_') && word_edge(text, i, true)) {
        let delim = &rest[..1];
        let end = closing(text, i, delim)?;
        if delim == "_" && !word_edge(text, end, false) {
            return None;
        }
        return Some((
            Span::Marked(&text[i + 1..end], json!({ "type": "em" })),
            end + 1,
        ));
    }
    if rest.starts_with('[') {
        let (label, href, end) = link(text, i)?;
        let mark = json!({ "type": "link", "attrs": { "href": href } });
        return Some((Span::Marked(label, mark), end));
    }
    None
}

fn spans(text: &str, marks: &[Value], depth: usize, out: &mut Vec<Value>) {
    if depth > MAX_DEPTH {
        push_text(text, marks, out);
        return;
    }
    let mut plain = 0;
    let mut i = 0;
    while i < text.len() {
        match span_at(text, i) {
            Some((span, end)) => {
                push_text(&text[plain..i], marks, out);
                match span {
                    Span::Code(inner) => {
                        let mut m = marks.to_vec();
                        m.push(json!({ "type": "code" }));
                        push_text(inner, &m, out);
                    }
                    Span::Marked(inner, mark) => {
                        let mut m = marks.to_vec();
                        m.push(mark);
                        spans(inner, &m, depth + 1, out);
                    }
                }
                i = end;
                plain = end;
            }
            None => i += text[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
    push_text(&text[plain..], marks, out);
}

/// `[label](href)` at `at`: the label, the address, and where it ends.
fn link(text: &str, at: usize) -> Option<(&str, &str, usize)> {
    let close = at + text[at..].find("](")?;
    let end = close + 2 + text[close + 2..].find(')')?;
    let label = &text[at + 1..close];
    let href = &text[close + 2..end];
    (!label.is_empty() && !href.is_empty() && !href.contains(' ')).then_some((label, href, end + 1))
}

#[cfg(test)]
mod tests {
    use super::super::to_markdown;
    use super::*;

    /// Markdown that reads back the same after a trip through ADF.
    fn round_trip(md: &str) {
        let doc = from_markdown(md).unwrap();
        assert_eq!(to_markdown(&doc), md, "{doc:#}");
    }

    #[test]
    fn blocks_survive_the_round_trip() {
        round_trip(
            "## Goal\n\nfirst line\nsecond\n\n- one\n- two\n  1. deep\n\n> quoted\n\n---\n\n```rust\nlet x = 1;\n\nlet y = 2;\n```",
        );
        round_trip("3. three\n4. four");
        round_trip("- [x] done\n- [ ] todo");
    }

    #[test]
    fn marks_survive_the_round_trip() {
        round_trip("**bold** and _it_ and ~~gone~~ and `a*b` and [site](https://x.y)");
        round_trip("**bold** and **_both_**");
    }

    #[test]
    fn what_isnt_markdown_stays_text() {
        let doc = from_markdown("snake_case and 2 * 3 and [not a link] and **open").unwrap();
        assert_eq!(doc["content"][0]["content"].as_array().unwrap().len(), 1);
        assert_eq!(
            to_markdown(&doc),
            "snake_case and 2 * 3 and [not a link] and **open"
        );
        assert_eq!(from_markdown("  \n "), None);
    }

    #[test]
    fn line_breaks_are_kept() {
        let doc = from_markdown("first line\nsecond\n\nnext paragraph\n").unwrap();
        assert_eq!(doc["content"].as_array().unwrap().len(), 2);
        assert_eq!(doc["content"][0]["content"][1]["type"], "hardBreak");
    }
}
