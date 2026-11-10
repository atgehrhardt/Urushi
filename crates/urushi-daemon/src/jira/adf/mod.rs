//! Atlassian Document Format (Jira's rich-text JSON) ⇄ Markdown, for descriptions and
//! comments.
//!
//! Reading ([`to_markdown`]) keeps the structure as Markdown: headings (`## `), bold,
//! italic, strikethrough, code, links, bullet, numbered and task lists, quotes, code
//! blocks and rules; tables become `a | b` rows, mentions `@Name`, and media
//! `[attachment]`. Writing ([`from_markdown`]) turns that Markdown back into the same
//! nodes, so editing a description in Urushi keeps its formatting. What Markdown can't
//! say (colors, panels, mentions as links, media) reads as text and is written back as
//! text, which is why notes are only written when the user changed them.

mod from_md;
mod to_md;

pub use from_md::from_markdown;
pub use to_md::to_markdown;

/// Deepest nesting read; deeper content is skipped (a guard against hostile input).
const MAX_DEPTH: usize = 32;
