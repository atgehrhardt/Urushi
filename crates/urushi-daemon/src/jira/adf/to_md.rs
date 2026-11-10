//! ADF → Markdown.

use serde_json::Value;

use super::MAX_DEPTH;

/// A description or comment (ADF, or plain text from older APIs) as Markdown.
pub fn to_markdown(description: &Value) -> String {
    match description {
        Value::String(text) => text.clone(),
        Value::Object(_) => {
            let mut out = Vec::new();
            blocks(description, 0, &mut out);
            out.join("\n\n")
        }
        _ => String::new(),
    }
}

fn kind(node: &Value) -> &str {
    node["type"].as_str().unwrap_or_default()
}

fn children(node: &Value) -> &[Value] {
    node["content"].as_array().map_or(&[], Vec::as_slice)
}

/// Appends the text of each block under `node`, one entry per block.
fn blocks(node: &Value, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_DEPTH {
        return;
    }
    for block in children(node) {
        let text = block_text(block, depth + 1);
        if !text.trim().is_empty() {
            out.push(text);
        }
    }
}

fn block_text(block: &Value, depth: usize) -> String {
    if depth > MAX_DEPTH {
        return String::new();
    }
    match kind(block) {
        "paragraph" => inline(block),
        "heading" => {
            let level = block["attrs"]["level"].as_u64().unwrap_or(1).clamp(1, 6);
            format!(
                "{} {}",
                "#".repeat(usize::try_from(level).unwrap_or(1)),
                inline(block)
            )
        }
        "bulletList" => list(block, depth, |_| "- ".into()),
        "orderedList" => {
            let start = block["attrs"]["order"].as_u64().unwrap_or(1);
            list(block, depth, |i| format!("{}. ", start + i as u64))
        }
        "taskList" => children(block)
            .iter()
            .map(|item| {
                let done = item["attrs"]["state"].as_str() == Some("DONE");
                format!("- [{}] {}", if done { "x" } else { " " }, inline(item))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "codeBlock" => {
            let language = block["attrs"]["language"].as_str().unwrap_or_default();
            format!("```{language}\n{}\n```", plain(block))
        }
        "blockquote" => nested(block, depth)
            .lines()
            .map(|l| format!("> {l}"))
            .collect::<Vec<_>>()
            .join("\n"),
        "rule" => "---".into(),
        "table" => children(block)
            .iter()
            .map(|row| {
                children(row)
                    .iter()
                    .map(|cell| nested(cell, depth).replace('\n', " "))
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        "mediaSingle" | "mediaGroup" | "media" => "[attachment]".into(),
        // Panels, expands, layouts…: their content.
        _ if block["content"].is_array() => nested(block, depth),
        _ => inline(block),
    }
}

/// The blocks inside `node` as text, separated by line breaks.
fn nested(node: &Value, depth: usize) -> String {
    let mut out = Vec::new();
    blocks(node, depth, &mut out);
    out.join("\n")
}

fn list(block: &Value, depth: usize, marker: impl Fn(usize) -> String) -> String {
    children(block)
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let text = nested(item, depth);
            let mut lines = text.lines();
            let first = format!("{}{}", marker(i), lines.next().unwrap_or_default());
            std::iter::once(first)
                .chain(lines.map(|l| format!("  {l}")))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text of inline nodes, without marks (inside code blocks).
fn plain(node: &Value) -> String {
    children(node)
        .iter()
        .map(|n| match kind(n) {
            "hardBreak" => "\n",
            _ => n["text"].as_str().unwrap_or_default(),
        })
        .collect()
}

/// A text node with its marks as Markdown.
fn marked(n: &Value) -> String {
    let whole = n["text"].as_str().unwrap_or_default();
    let marks = n["marks"].as_array().map_or(&[][..], Vec::as_slice);
    // Marks hug the text: spaces at either end stay outside them.
    let core = whole.trim();
    if marks.is_empty() || core.is_empty() {
        return whole.to_owned();
    }
    let before = &whole[..whole.len() - whole.trim_start().len()];
    let after = &whole[whole.trim_end().len()..];
    let mut text = core.to_owned();
    // Code first: nothing inside it is marked up.
    if marks.iter().any(|m| kind(m) == "code") {
        return format!("{before}`{text}`{after}");
    }
    // The innermost (last) mark first, so the outermost wraps the rest.
    for m in marks.iter().rev() {
        text = match kind(m) {
            "strong" => format!("**{text}**"),
            "em" => format!("_{text}_"),
            "strike" => format!("~~{text}~~"),
            "link" => format!(
                "[{text}]({})",
                m["attrs"]["href"].as_str().unwrap_or_default()
            ),
            _ => text,
        };
    }
    format!("{before}{text}{after}")
}

/// The Markdown of inline nodes.
fn inline(node: &Value) -> String {
    let mut out = String::new();
    for n in children(node) {
        let attrs = &n["attrs"];
        match kind(n) {
            "text" => out.push_str(&marked(n)),
            "hardBreak" => out.push('\n'),
            "mention" => {
                let name = attrs["text"].as_str().unwrap_or_default();
                if !name.starts_with('@') {
                    out.push('@');
                }
                out.push_str(name);
            }
            "status" => out.push_str(attrs["text"].as_str().unwrap_or_default()),
            "emoji" => out.push_str(
                attrs["text"]
                    .as_str()
                    .or_else(|| attrs["shortName"].as_str())
                    .unwrap_or_default(),
            ),
            "inlineCard" | "blockCard" => out.push_str(attrs["url"].as_str().unwrap_or_default()),
            "date" => out.push_str(&date(attrs["timestamp"].as_str().unwrap_or_default())),
            _ => out.push_str(&inline(n)),
        }
    }
    out
}

/// A date node's `timestamp` (milliseconds) as `YYYY-MM-DD`.
fn date(millis: &str) -> String {
    let Ok(ms) = millis.parse::<i64>() else {
        return millis.to_owned();
    };
    let days = ms.div_euclid(86_400_000);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn rich_text_reads_as_plain_text() {
        let doc = json!({
            "type": "doc", "version": 1, "content": [
                { "type": "heading", "attrs": { "level": 2 }, "content": [{ "type": "text", "text": "Goal" }] },
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "Ask " },
                    { "type": "mention", "attrs": { "text": "@Ada" } },
                    { "type": "text", "text": " by ", "marks": [{ "type": "strong" }] },
                    { "type": "date", "attrs": { "timestamp": "1735689600000" } },
                    { "type": "hardBreak" },
                    { "type": "text", "text": "then ship" }
                ]},
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "one" }] }] },
                    { "type": "listItem", "content": [
                        { "type": "paragraph", "content": [{ "type": "text", "text": "two" }] },
                        { "type": "orderedList", "content": [
                            { "type": "listItem", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "deep" }] }] }
                        ]}
                    ]}
                ]},
                { "type": "codeBlock", "content": [{ "type": "text", "text": "cargo test" }] },
                { "type": "panel", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "note" }] }] },
                { "type": "mediaSingle", "content": [{ "type": "media" }] }
            ]
        });
        assert_eq!(
            to_markdown(&doc),
            "## Goal\n\nAsk @Ada **by** 2025-01-01\nthen ship\n\n- one\n- two\n  1. deep\n\n```\ncargo test\n```\n\nnote\n\n[attachment]"
        );
    }

    #[test]
    fn marks_and_task_lists_read_as_markdown() {
        let text = |t: &str, marks: Value| json!({ "type": "text", "text": t, "marks": marks });
        let doc = json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [
                text("bold", json!([{ "type": "strong" }])),
                { "type": "text", "text": " and " },
                text("site", json!([{ "type": "link", "attrs": { "href": "https://x.y" } }])),
                { "type": "text", "text": " " },
                text("a*b", json!([{ "type": "code" }, { "type": "strong" }])),
            ]},
            { "type": "taskList", "content": [
                { "type": "taskItem", "attrs": { "state": "DONE" }, "content": [{ "type": "text", "text": "done" }] },
                { "type": "taskItem", "attrs": { "state": "TODO" }, "content": [{ "type": "text", "text": "todo" }] }
            ]},
            { "type": "codeBlock", "attrs": { "language": "rust" }, "content": [{ "type": "text", "text": "let x = 1;" }] }
        ]});
        assert_eq!(
            to_markdown(&doc),
            "**bold** and [site](https://x.y) `a*b`\n\n- [x] done\n- [ ] todo\n\n```rust\nlet x = 1;\n```"
        );
    }

    #[test]
    fn odd_input_reads_as_little_as_possible() {
        assert_eq!(to_markdown(&Value::Null), "");
        assert_eq!(to_markdown(&json!("wiki *text*")), "wiki *text*");
        let mut deep = json!({ "type": "paragraph", "content": [{ "type": "text", "text": "x" }] });
        for _ in 0..100 {
            deep = json!({ "type": "blockquote", "content": [deep] });
        }
        let doc = json!({ "type": "doc", "content": [deep] });
        assert_eq!(to_markdown(&doc), "");
    }
}
