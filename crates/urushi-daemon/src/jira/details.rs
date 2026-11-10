//! An issue as the editor shows it: every field the account may edit (from Jira's edit
//! metadata, so a site's own fields come too), its comments, attachments, links,
//! watchers and work log. These are read when an issue is opened, not kept on the
//! board: the workspace only carries what cards and filters need.
//!
//! A field's [`FieldKind`] says how its value is shown and how an edit is written
//! back ([`FieldValue::to_json`]).

use serde_json::{Value, json};
use urushi_core::Timestamp;
use urushi_core::board::Person;

use super::adf;

/// How a field holds its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// One line of text.
    Text,
    /// Rich text, edited as Markdown.
    RichText,
    /// A number.
    Number,
    /// A date (`YYYY-MM-DD`).
    Date,
    /// A date and time (RFC 3339).
    DateTime,
    /// One of the allowed values (priority, type, a select list, a version…).
    Option,
    /// Any of the allowed values (components, fix versions, a multi-select…).
    Options,
    /// A person.
    User,
    /// People.
    Users,
    /// Free labels.
    Labels,
    /// An issue (the parent), by key.
    Issue,
    /// A sprint of the board.
    Sprint,
    /// Something Urushi can't edit: shown as text.
    Other,
}

/// A value Jira names: an option, a person (`id` is the account id), a sprint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldOption {
    /// Its id.
    pub id: String,
    /// What it is called.
    pub name: String,
}

/// A field's value: `text` for text-like kinds (an issue's key for [`FieldKind::Issue`]),
/// `options` for the others. Empty means no value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldValue {
    /// The text.
    pub text: String,
    /// The chosen values.
    pub options: Vec<FieldOption>,
}

/// A field of an issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    /// Its id (`priority`, `customfield_10016`).
    pub id: String,
    /// Its name.
    pub name: String,
    /// How it holds its value.
    pub kind: FieldKind,
    /// Jira refuses the issue without it.
    pub required: bool,
    /// The values it may take, for [`FieldKind::Option`] and [`FieldKind::Options`].
    pub allowed: Vec<FieldOption>,
    /// Its value now.
    pub value: FieldValue,
}

/// A comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    /// Its id.
    pub id: String,
    /// Who wrote it.
    pub author: Person,
    /// What it says, as Markdown.
    pub body: String,
    /// When it was written.
    pub created: Timestamp,
    /// When it last changed.
    pub updated: Timestamp,
}

/// A file attached to an issue in Jira.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JiraAttachment {
    /// Its id.
    pub id: String,
    /// Its file name.
    pub name: String,
    /// Its media type.
    pub media_type: String,
    /// Its size in bytes.
    pub size: u64,
    /// When it was added.
    pub created: Timestamp,
    /// Who added it.
    pub author: Option<Person>,
    /// Where its contents are.
    pub content_url: String,
}

/// Another issue, as a link or a search shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueRef {
    /// Its id.
    pub id: String,
    /// Its key.
    pub key: String,
    /// Its summary.
    pub summary: String,
    /// Its status's name.
    pub status: String,
    /// Its type's name.
    pub type_name: String,
}

/// A link to another issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// The link's id.
    pub id: String,
    /// What the link says from this issue's side ("blocks", "is blocked by").
    pub relation: String,
    /// The other issue.
    pub issue: IssueRef,
}

/// A kind of link the site has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkType {
    /// Its name ("Blocks").
    pub name: String,
    /// What it says from the inward issue's side ("is blocked by").
    pub inward: String,
    /// What it says from the outward issue's side ("blocks").
    pub outward: String,
}

/// Time logged on an issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worklog {
    /// Its id.
    pub id: String,
    /// Who logged it.
    pub author: Person,
    /// How long, in seconds.
    pub seconds: u64,
    /// When the work started.
    pub started: Timestamp,
    /// What was done, as Markdown.
    pub comment: String,
}

/// An issue's time tracking, as Jira words it ("3h 20m").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimeTracking {
    /// The original estimate.
    pub original: String,
    /// The remaining estimate.
    pub remaining: String,
    /// The time spent.
    pub spent: String,
}

/// Everything the editor shows of an issue beyond the board's fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueDetails {
    /// The fields the account may edit, in Jira's order.
    pub fields: Vec<Field>,
    /// The transitions it can take now.
    pub transitions: Vec<super::Transition>,
    /// Its comments, oldest first.
    pub comments: Vec<Comment>,
    /// Its attachments.
    pub attachments: Vec<JiraAttachment>,
    /// Its links.
    pub links: Vec<Link>,
    /// The kinds of link the site has.
    pub link_types: Vec<LinkType>,
    /// Who watches it.
    pub watchers: Vec<Person>,
    /// Whether the signed-in account does.
    pub watching: bool,
    /// Its work log.
    pub worklogs: Vec<Worklog>,
    /// Its time tracking.
    pub time: TimeTracking,
}

impl FieldValue {
    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.options.is_empty()
    }

    /// The value as Jira's edit API takes it for a field of `kind` (`null` clears it).
    pub fn to_json(&self, kind: FieldKind) -> Result<Value, String> {
        let text = self.text.trim();
        let first = self.options.first();
        let ids = || -> Vec<Value> { self.options.iter().map(|o| json!({ "id": o.id })).collect() };
        Ok(match kind {
            _ if self.is_empty()
                && kind != FieldKind::Options
                && kind != FieldKind::Users
                && kind != FieldKind::Labels =>
            {
                Value::Null
            }
            FieldKind::Text | FieldKind::DateTime => json!(text),
            FieldKind::RichText => adf::from_markdown(&self.text).unwrap_or(Value::Null),
            FieldKind::Number => json!(
                text.parse::<f64>()
                    .map_err(|_| format!("{text:?} isn't a number"))?
            ),
            FieldKind::Date => {
                if !is_date(text) {
                    return Err(format!("{text:?} isn't a date (YYYY-MM-DD)"));
                }
                json!(text)
            }
            FieldKind::Option => first.map_or(Value::Null, |o| json!({ "id": o.id })),
            FieldKind::Options => Value::Array(ids()),
            FieldKind::User => first.map_or(Value::Null, |o| json!({ "accountId": o.id })),
            FieldKind::Users => Value::Array(
                self.options
                    .iter()
                    .map(|o| json!({ "accountId": o.id }))
                    .collect(),
            ),
            FieldKind::Labels => Value::Array(self.options.iter().map(|o| json!(o.name)).collect()),
            FieldKind::Issue => json!({ "key": text }),
            FieldKind::Sprint => first
                .and_then(|o| o.id.parse::<u64>().ok())
                .map_or(Value::Null, |id| json!(id)),
            FieldKind::Other => return Err("Urushi can't change that field".into()),
        })
    }
}

/// Whether `text` is a `YYYY-MM-DD` date.
fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    matches!(parts.as_slice(), [y, m, d]
        if y.len() == 4 && m.len() == 2 && d.len() == 2
            && [y, m, d].iter().all(|p| p.chars().all(|c| c.is_ascii_digit()))
            && (1..=12).contains(&m.parse::<u32>().unwrap_or(0))
            && (1..=31).contains(&d.parse::<u32>().unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(id: &str) -> FieldOption {
        FieldOption {
            id: id.into(),
            name: format!("name {id}"),
        }
    }

    #[test]
    fn values_are_written_as_jira_takes_them() {
        let text = |t: &str| FieldValue {
            text: t.into(),
            options: Vec::new(),
        };
        let options = |o: Vec<FieldOption>| FieldValue {
            text: String::new(),
            options: o,
        };
        assert_eq!(text("3.5").to_json(FieldKind::Number), Ok(json!(3.5)));
        assert!(text("lots").to_json(FieldKind::Number).is_err());
        assert_eq!(
            text("2026-12-01").to_json(FieldKind::Date),
            Ok(json!("2026-12-01"))
        );
        assert!(text("12/01/2026").to_json(FieldKind::Date).is_err());
        assert_eq!(text("").to_json(FieldKind::Date), Ok(Value::Null));
        assert_eq!(
            options(vec![opt("2")]).to_json(FieldKind::Option),
            Ok(json!({ "id": "2" }))
        );
        assert_eq!(
            options(vec![opt("1"), opt("3")]).to_json(FieldKind::Options),
            Ok(json!([{ "id": "1" }, { "id": "3" }]))
        );
        assert_eq!(
            options(Vec::new()).to_json(FieldKind::Options),
            Ok(json!([]))
        );
        assert_eq!(
            options(vec![opt("acc-1")]).to_json(FieldKind::User),
            Ok(json!({ "accountId": "acc-1" }))
        );
        assert_eq!(
            options(Vec::new()).to_json(FieldKind::User),
            Ok(Value::Null)
        );
        assert_eq!(
            options(vec![opt("12")]).to_json(FieldKind::Sprint),
            Ok(json!(12))
        );
        assert_eq!(
            text("ACME-7").to_json(FieldKind::Issue),
            Ok(json!({ "key": "ACME-7" }))
        );
        assert_eq!(
            text("**hi**").to_json(FieldKind::RichText).unwrap()["type"],
            json!("doc")
        );
        assert!(text("x").to_json(FieldKind::Other).is_err());
    }
}
