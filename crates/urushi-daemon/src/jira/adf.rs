//! Atlassian Document Format (Jira's rich-text JSON) ⇄ the plain text of task notes.
//!
//! Reading keeps what reads well as text: paragraphs and headings, lists as `- ` and
//! `1. ` lines, quotes as `> `, code blocks, tables as `a | b` rows, mentions, emoji,
//! links and dates. Writing makes one paragraph per block of lines, with line breaks
//! kept. The round trip loses formatting (bold, colors, panels…), which is why notes are
//! only written back when the user changed them.

use serde_json::{Value, json};

/// Deepest nesting read; deeper content is skipped (a guard against hostile input).
const MAX_DEPTH: usize = 32;

/// A description (ADF, or plain text from older APIs) as plain text.
pub fn to_text(description: &Value) -> String {
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

/// Plain text as an ADF document; `None` (no description) when blank.
pub fn from_text(text: &str) -> Option<Value> {
    let text = text.trim_end();
    if text.trim().is_empty() {
        return None;
    }
    let paragraphs: Vec<Value> = text
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let mut content = Vec::new();
            for (i, line) in p.lines().enumerate() {
                if i > 0 {
                    content.push(json!({ "type": "hardBreak" }));
                }
                if !line.is_empty() {
                    content.push(json!({ "type": "text", "text": line }));
                }
            }
            json!({ "type": "paragraph", "content": content })
        })
        .collect();
    Some(json!({ "type": "doc", "version": 1, "content": paragraphs }))
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
        "paragraph" | "heading" => inline(block),
        "bulletList" => list(block, depth, |_| "- ".into()),
        "orderedList" => {
            let start = block["attrs"]["order"].as_u64().unwrap_or(1);
            list(block, depth, |i| format!("{}. ", start + i as u64))
        }
        "taskList" => list(block, depth, |_| "[ ] ".into()),
        "codeBlock" => format!("```\n{}\n```", inline(block)),
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

/// The text of inline nodes.
fn inline(node: &Value) -> String {
    let mut out = String::new();
    for n in children(node) {
        let attrs = &n["attrs"];
        match kind(n) {
            "text" => out.push_str(n["text"].as_str().unwrap_or_default()),
            "hardBreak" => out.push('\n'),
            "mention" | "status" => out.push_str(attrs["text"].as_str().unwrap_or_default()),
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
            to_text(&doc),
            "Goal\n\nAsk @Ada by 2025-01-01\nthen ship\n\n- one\n- two\n  1. deep\n\n```\ncargo test\n```\n\nnote\n\n[attachment]"
        );
    }

    #[test]
    fn plain_text_becomes_paragraphs_with_line_breaks() {
        let doc = from_text("first line\nsecond\n\nnext paragraph\n").unwrap();
        assert_eq!(doc["content"].as_array().unwrap().len(), 2);
        assert_eq!(doc["content"][0]["content"][1]["type"], "hardBreak");
        assert_eq!(to_text(&doc), "first line\nsecond\n\nnext paragraph");
        assert_eq!(from_text("  \n "), None);
    }

    #[test]
    fn odd_input_reads_as_little_as_possible() {
        assert_eq!(to_text(&Value::Null), "");
        assert_eq!(to_text(&json!("wiki *text*")), "wiki *text*");
        let mut deep = json!({ "type": "paragraph", "content": [{ "type": "text", "text": "x" }] });
        for _ in 0..100 {
            deep = json!({ "type": "blockquote", "content": [deep] });
        }
        let doc = json!({ "type": "doc", "content": [deep] });
        assert_eq!(to_text(&doc), "");
    }
}
