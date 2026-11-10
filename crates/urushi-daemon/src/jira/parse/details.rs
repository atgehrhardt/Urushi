//! Jira's answers about one issue → [`crate::jira::details`]: its edit metadata, the
//! values of those fields, comments, attachments, links, watchers, work log, and the
//! people and issues pickers search for. Lenient, like the rest of `parse`.

use serde_json::Value;
use urushi_core::board::Person;

use super::{adf, person, some_text, text, time};
use crate::jira::details::{
    Comment, Field, FieldKind, FieldOption, FieldValue, IssueRef, JiraAttachment, Link, LinkType,
    TimeTracking, Worklog,
};

/// Fields the editor shows in their own places (title, notes, tags, status, the tabs),
/// or that Urushi doesn't change.
const OWN_PLACE: &[&str] = &[
    "summary",
    "description",
    "labels",
    "attachment",
    "issuelinks",
    "comment",
    "worklog",
    "timetracking",
    "project",
    "watches",
    "status",
];

/// Fields shown first, in this order; the rest follow by name.
const FIRST: &[&str] = &[
    "issuetype",
    "assignee",
    "reporter",
    "priority",
    "parent",
    "duedate",
    "components",
    "fixVersions",
    "versions",
];

/// The sprint field's custom type.
const SPRINT: &str = "com.pyxis.greenhopper.jira:gh-sprint";

/// How a field with this schema holds its value.
fn kind(schema: &Value) -> FieldKind {
    let custom = schema["custom"].as_str().unwrap_or_default();
    let system = schema["system"].as_str().unwrap_or_default();
    match (
        schema["type"].as_str().unwrap_or_default(),
        schema["items"].as_str(),
    ) {
        _ if custom == SPRINT => FieldKind::Sprint,
        ("string", _) if matches!(system, "environment") || custom.ends_with(":textarea") => {
            FieldKind::RichText
        }
        ("string", _) => FieldKind::Text,
        ("number", _) => FieldKind::Number,
        ("date", _) => FieldKind::Date,
        ("datetime", _) => FieldKind::DateTime,
        ("user", _) => FieldKind::User,
        ("issuelink", _) if system == "parent" => FieldKind::Issue,
        (
            "option" | "priority" | "issuetype" | "resolution" | "version" | "component"
            | "securitylevel",
            _,
        ) => FieldKind::Option,
        ("array", Some("string")) if system == "labels" || custom.ends_with(":labels") => {
            FieldKind::Labels
        }
        ("array", Some("option" | "component" | "version")) => FieldKind::Options,
        ("array", Some("user")) => FieldKind::Users,
        _ => FieldKind::Other,
    }
}

fn option(v: &Value) -> Option<FieldOption> {
    let id = some_text(&v["id"])
        .or_else(|| v["id"].as_u64().map(|n| n.to_string()))
        .or_else(|| some_text(&v["accountId"]))?;
    let name = some_text(&v["name"])
        .or_else(|| some_text(&v["value"]))
        .or_else(|| some_text(&v["displayName"]))
        .unwrap_or_default();
    Some(FieldOption { id, name })
}

fn options(v: &Value) -> Vec<FieldOption> {
    v.as_array()
        .map(|a| a.iter().filter_map(option).collect())
        .unwrap_or_default()
}

/// The fields of an issue's edit (or create) metadata, without values. `meta` is the
/// map of field id → `{name, required, schema, allowedValues}`; or, from create
/// metadata, an array of those with `fieldId`.
pub(crate) fn fields(meta: &Value) -> Vec<Field> {
    let entries: Vec<(String, &Value)> = match meta {
        Value::Object(map) => map.iter().map(|(id, f)| (id.clone(), f)).collect(),
        Value::Array(list) => list
            .iter()
            .filter_map(|f| {
                Some((
                    some_text(&f["fieldId"]).or_else(|| some_text(&f["key"]))?,
                    f,
                ))
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut out: Vec<Field> = entries
        .into_iter()
        .filter(|(id, _)| !OWN_PLACE.contains(&id.as_str()))
        .map(|(id, f)| Field {
            kind: kind(&f["schema"]),
            name: some_text(&f["name"]).unwrap_or_else(|| id.clone()),
            required: f["required"].as_bool() == Some(true),
            allowed: options(&f["allowedValues"]),
            value: FieldValue::default(),
            id,
        })
        // Ranking is the board's order, changed by moving cards.
        .filter(|f| !(f.kind == FieldKind::Other && f.name.eq_ignore_ascii_case("rank")))
        .collect();
    let place = |f: &Field| {
        FIRST
            .iter()
            .position(|id| *id == f.id)
            .unwrap_or(FIRST.len())
    };
    out.sort_by(|a, b| place(a).cmp(&place(b)).then_with(|| a.name.cmp(&b.name)));
    out
}

/// A field's value in an issue's `fields`.
pub(crate) fn value(kind: FieldKind, v: &Value) -> FieldValue {
    let text_value = |t: String| FieldValue {
        text: t,
        options: Vec::new(),
    };
    let options_value = |o: Vec<FieldOption>| FieldValue {
        text: String::new(),
        options: o,
    };
    match kind {
        FieldKind::Text | FieldKind::Date | FieldKind::DateTime => text_value(text(v)),
        FieldKind::RichText => text_value(adf::to_markdown(v)),
        FieldKind::Number => text_value(v.as_f64().map(|n| n.to_string()).unwrap_or_default()),
        FieldKind::Option | FieldKind::User => options_value(option(v).into_iter().collect()),
        FieldKind::Options | FieldKind::Users => options_value(options(v)),
        FieldKind::Labels => options_value(
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(some_text)
                        .map(|l| FieldOption {
                            id: l.clone(),
                            name: l,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        FieldKind::Issue => FieldValue {
            text: text(&v["key"]),
            options: some_text(&v["key"])
                .map(|key| FieldOption {
                    id: key,
                    name: text(&v["fields"]["summary"]),
                })
                .into_iter()
                .collect(),
        },
        FieldKind::Sprint => {
            let sprints = v.as_array().map_or(&[][..], Vec::as_slice);
            let state = |s: &&Value| s["state"].as_str() == Some("active");
            options_value(
                sprints
                    .iter()
                    .find(state)
                    .or_else(|| sprints.last())
                    .and_then(option)
                    .into_iter()
                    .collect(),
            )
        }
        FieldKind::Other => text_value(match v {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => option(other).map_or_else(|| other.to_string(), |o| o.name),
        }),
    }
}

/// An issue's comments (`GET issue/{key}/comment`).
pub(crate) fn comments(v: &Value) -> Vec<Comment> {
    v["comments"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    Some(Comment {
                        id: some_text(&c["id"])?,
                        author: person(&c["author"]).unwrap_or_default(),
                        body: adf::to_markdown(&c["body"]),
                        created: time(&c["created"]),
                        updated: time(&c["updated"]),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// An issue's `attachment` field.
pub(crate) fn attachments(v: &Value) -> Vec<JiraAttachment> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    Some(JiraAttachment {
                        id: some_text(&f["id"])?,
                        name: text(&f["filename"]),
                        media_type: text(&f["mimeType"]),
                        size: f["size"].as_u64().unwrap_or_default(),
                        created: time(&f["created"]),
                        author: person(&f["author"]),
                        content_url: text(&f["content"]),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Another issue as Jira nests it (in a link, a search).
pub(crate) fn issue_ref(v: &Value) -> Option<IssueRef> {
    let f = &v["fields"];
    Some(IssueRef {
        id: some_text(&v["id"])?,
        key: some_text(&v["key"])?,
        summary: text(&f["summary"]),
        status: text(&f["status"]["name"]),
        type_name: text(&f["issuetype"]["name"]),
    })
}

/// An issue's `issuelinks` field, each from this issue's side.
pub(crate) fn links(v: &Value) -> Vec<Link> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|l| {
                    let (relation, other) = if l["outwardIssue"].is_object() {
                        (text(&l["type"]["outward"]), &l["outwardIssue"])
                    } else {
                        (text(&l["type"]["inward"]), &l["inwardIssue"])
                    };
                    Some(Link {
                        id: some_text(&l["id"])?,
                        relation,
                        issue: issue_ref(other)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The site's kinds of link (`GET issueLinkType`).
pub(crate) fn link_types(v: &Value) -> Vec<LinkType> {
    v["issueLinkTypes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    Some(LinkType {
                        name: some_text(&t["name"])?,
                        inward: text(&t["inward"]),
                        outward: text(&t["outward"]),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// People in a list (`watchers`, user searches).
pub(crate) fn people(v: &Value) -> Vec<Person> {
    v.as_array()
        .map(|a| a.iter().filter_map(person).collect())
        .unwrap_or_default()
}

/// An issue's work log (`GET issue/{key}/worklog`).
pub(crate) fn worklogs(v: &Value) -> Vec<Worklog> {
    v["worklogs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|w| {
                    Some(Worklog {
                        id: some_text(&w["id"])?,
                        author: person(&w["author"]).unwrap_or_default(),
                        seconds: w["timeSpentSeconds"].as_u64().unwrap_or_default(),
                        started: time(&w["started"]),
                        comment: adf::to_markdown(&w["comment"]),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// An issue's `timetracking` field.
pub(crate) fn time_tracking(v: &Value) -> TimeTracking {
    TimeTracking {
        original: text(&v["originalEstimate"]),
        remaining: text(&v["remainingEstimate"]),
        spent: text(&v["timeSpent"]),
    }
}

/// A board's open and future sprints (`GET board/{id}/sprint`).
pub(crate) fn sprints(v: &Value) -> Vec<FieldOption> {
    options(&v["values"])
}

#[cfg(test)]
mod tests;
