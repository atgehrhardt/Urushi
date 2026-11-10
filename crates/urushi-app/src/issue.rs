//! The task editor's Jira side (protocol 1.13): the issue in full as the daemon read
//! it (`GetJiraIssue`), the field edits waiting for Save, the open picker, and the
//! comment, link and work-log forms. It lives in the editor (`Dialog::Task`), so
//! going down to a subtask and Back keeps it.
//!
//! Field edits are drafts, sent with Save as one `ChangeJiraIssue` (nothing reaches
//! Jira before Save, as with the title and notes). Status changes, comments,
//! attachments, links, watching and work are immediate, as in Jira, each with its own
//! button.

use std::collections::BTreeMap;

use urushi_client::v1;
use urushi_ui::TextField;

/// First protocol minor version with Jira issues in full in the editor.
pub const ISSUES_SINCE_MINOR: u32 = 13;

/// The pseudo field id of the status picker.
pub const STATUS: &str = "__status";
/// The pseudo field id of the "link an issue" picker.
pub const LINK: &str = "__link";

/// Which part of the issue shows under the notes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IssueTab {
    /// Comments, and a box to add one.
    #[default]
    Comments,
    /// Attachments.
    Files,
    /// Links to other issues.
    Links,
    /// The work log and time tracking.
    Work,
}

/// The editor's Jira side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IssueEditor {
    /// The issue in full; `None` until the daemon answers.
    pub details: Option<v1::JiraIssueDetails>,
    /// Asked for and not answered yet.
    pub loading: bool,
    /// New values for fields (by id), sent with Save.
    pub drafts: BTreeMap<String, v1::JiraFieldValue>,
    /// Text-like fields being typed in (by id), sent with Save when changed.
    pub texts: BTreeMap<String, TextField>,
    /// The field whose picker is open (or [`STATUS`], [`LINK`]).
    pub open: Option<String>,
    /// The picker's search.
    pub search: TextField,
    /// People found for the open picker.
    pub people: Vec<v1::JiraUser>,
    /// Issues found for the open picker.
    pub issues: Vec<v1::JiraIssueRef>,
    /// The part shown under the notes.
    pub tab: IssueTab,
    /// A new comment.
    pub comment: TextField,
    /// A comment being changed: its id and text.
    pub editing: Option<(String, TextField)>,
    /// The kind of link to add (an index into the link types), and whether this issue
    /// is on its outward side.
    pub link: (usize, bool),
    /// Time to log ("1h 30m").
    pub work: TextField,
}

impl IssueEditor {
    /// The fields, once read.
    pub fn fields(&self) -> &[v1::JiraField] {
        self.details.as_ref().map_or(&[], |d| d.fields.as_slice())
    }

    /// A field's value as the editor shows it: its draft, else Jira's.
    pub fn value(&self, field: &v1::JiraField) -> v1::JiraFieldValue {
        if let Some(text) = self.texts.get(&field.id) {
            return v1::JiraFieldValue {
                text: text.text().to_owned(),
                options: Vec::new(),
            };
        }
        self.drafts
            .get(&field.id)
            .cloned()
            .or_else(|| field.value.clone())
            .unwrap_or_default()
    }

    /// The text field typed into for field number `i`, if one is open.
    pub fn text_at(&self, i: u16) -> Option<&TextField> {
        let id = &self.fields().get(usize::from(i))?.id;
        self.texts.get(id)
    }

    /// The same, to edit; it starts from the field's value.
    pub fn text_at_mut(&mut self, i: u16) -> Option<&mut TextField> {
        let field = self.fields().get(usize::from(i))?.clone();
        let start = field.value.map(|v| v.text).unwrap_or_default();
        let multiline = field.kind == i32::from(v1::JiraFieldKind::RichText);
        Some(self.texts.entry(field.id).or_insert_with(|| {
            if multiline {
                TextField::multiline(start)
            } else {
                TextField::new(start)
            }
        }))
    }

    /// The field edits Save would send: drafts, and typed text that differs from
    /// Jira's.
    pub fn edits(&self) -> Vec<v1::JiraFieldEdit> {
        let mut out: Vec<v1::JiraFieldEdit> = self
            .drafts
            .iter()
            .map(|(id, value)| v1::JiraFieldEdit {
                field_id: id.clone(),
                value: Some(value.clone()),
            })
            .collect();
        for field in self.fields() {
            let Some(typed) = self.texts.get(&field.id) else {
                continue;
            };
            let was = field.value.as_ref().map_or("", |v| v.text.as_str());
            if typed.text().trim_end() != was.trim_end() {
                out.push(v1::JiraFieldEdit {
                    field_id: field.id.clone(),
                    value: Some(v1::JiraFieldValue {
                        text: typed.text().trim_end().to_owned(),
                        options: Vec::new(),
                    }),
                });
            }
        }
        out
    }

    /// Forgets the drafts (they were sent).
    pub fn saved(&mut self) {
        self.drafts.clear();
        self.texts.clear();
    }
}

/// A duration as people type it ("1h 30m", "2d", "45m", "1.5h"; a bare number is
/// minutes) in seconds, with Jira's working day (8 h) and week (5 days). `None` when it
/// doesn't read as one, or is zero.
pub fn duration(text: &str) -> Option<u64> {
    const UNITS: [(char, f64); 5] = [
        ('w', 5.0 * 8.0 * 3600.0),
        ('d', 8.0 * 3600.0),
        ('h', 3600.0),
        ('m', 60.0),
        ('s', 1.0),
    ];
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    if let Ok(minutes) = text.parse::<f64>() {
        return seconds(minutes * 60.0);
    }
    let mut total = 0.0;
    for part in text.split_whitespace() {
        let unit = part.chars().last()?;
        let (_, scale) = UNITS.iter().find(|(u, _)| *u == unit)?;
        let n: f64 = part[..part.len() - unit.len_utf8()].parse().ok()?;
        if !n.is_finite() || n < 0.0 {
            return None;
        }
        total += n * scale;
    }
    seconds(total)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "checked finite, positive and rounded"
)]
fn seconds(s: f64) -> Option<u64> {
    (s.is_finite() && (1.0..1e9).contains(&s)).then(|| s.round() as u64)
}

/// A duration in seconds as Jira words it: "1h 30m".
pub fn words(seconds: u64) -> String {
    let (h, m) = (seconds / 3600, seconds % 3600 / 60);
    match (h, m) {
        (0, 0) => format!("{seconds}s"),
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_jira_counts_them() {
        assert_eq!(duration("1h 30m"), Some(5400));
        assert_eq!(duration("2d"), Some(57_600));
        assert_eq!(duration("1w"), Some(144_000));
        assert_eq!(duration("1.5h"), Some(5400));
        assert_eq!(duration("45"), Some(2700));
        for bad in ["", "soon", "3x", "-1h", "0m"] {
            assert_eq!(duration(bad), None, "{bad}");
        }
        assert_eq!(words(5400), "1h 30m");
        assert_eq!(words(3600), "1h");
        assert_eq!(words(120), "2m");
    }

    #[test]
    fn edits_are_drafts_and_changed_text() {
        let field = |id: &str, text: &str| v1::JiraField {
            id: id.into(),
            kind: v1::JiraFieldKind::Text.into(),
            value: Some(v1::JiraFieldValue {
                text: text.into(),
                options: Vec::new(),
            }),
            ..Default::default()
        };
        let mut e = IssueEditor {
            details: Some(v1::JiraIssueDetails {
                fields: vec![field("a", "same"), field("b", "old")],
                ..Default::default()
            }),
            ..IssueEditor::default()
        };
        assert!(e.edits().is_empty());
        e.text_at_mut(0);
        e.text_at_mut(1).unwrap().insert(" new");
        e.drafts
            .insert("priority".into(), v1::JiraFieldValue::default());
        let edits = e.edits();
        let ids: Vec<&str> = edits.iter().map(|x| x.field_id.as_str()).collect();
        assert_eq!(ids, ["priority", "b"]);
        assert_eq!(e.value(&e.fields()[1].clone()).text, "old new");
        e.saved();
        assert!(e.edits().is_empty());
    }
}
