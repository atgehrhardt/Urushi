//! Wire ⇄ service for one Jira issue in full (protocol 1.13): its details, the fields a
//! new issue has, changes, attachments' contents, and the people and issue searches.

use urushi_core::board::Person;
use urushi_proto::convert;
use urushi_proto::v1::{self, request::Body, response::Outcome};

use super::handlers::{invalid, parse};
use crate::jira::details::{Field, FieldKind, FieldOption, FieldValue, IssueDetails};
use crate::service::{AttachmentData, IssueChange, Service, ServiceResult};

fn user(p: &Person) -> v1::JiraUser {
    v1::JiraUser {
        account_id: p.account_id.clone(),
        name: p.name.clone(),
    }
}

fn kind_to_proto(kind: FieldKind) -> i32 {
    use v1::JiraFieldKind as K;
    match kind {
        FieldKind::Text => K::Text,
        FieldKind::RichText => K::RichText,
        FieldKind::Number => K::Number,
        FieldKind::Date => K::Date,
        FieldKind::DateTime => K::DateTime,
        FieldKind::Option => K::Option,
        FieldKind::Options => K::Options,
        FieldKind::User => K::User,
        FieldKind::Users => K::Users,
        FieldKind::Labels => K::Labels,
        FieldKind::Issue => K::Issue,
        FieldKind::Sprint => K::Sprint,
        FieldKind::Other => K::Other,
    }
    .into()
}

fn option(o: &FieldOption) -> v1::JiraOption {
    v1::JiraOption {
        id: o.id.clone(),
        name: o.name.clone(),
    }
}

fn value_to_proto(v: &FieldValue) -> v1::JiraFieldValue {
    v1::JiraFieldValue {
        text: v.text.clone(),
        options: v.options.iter().map(option).collect(),
    }
}

/// A field value from the wire (unset: no value, which clears the field).
pub(super) fn value_from_proto(v: Option<&v1::JiraFieldValue>) -> FieldValue {
    v.map(|v| FieldValue {
        text: v.text.clone(),
        options: v
            .options
            .iter()
            .map(|o| FieldOption {
                id: o.id.clone(),
                name: o.name.clone(),
            })
            .collect(),
    })
    .unwrap_or_default()
}

/// Field edits from the wire.
pub(super) fn edits_from_proto(edits: &[v1::JiraFieldEdit]) -> Vec<(String, FieldValue)> {
    edits
        .iter()
        .map(|e| (e.field_id.clone(), value_from_proto(e.value.as_ref())))
        .collect()
}

fn field(f: &Field) -> v1::JiraField {
    v1::JiraField {
        id: f.id.clone(),
        name: f.name.clone(),
        kind: kind_to_proto(f.kind),
        required: f.required,
        allowed: f.allowed.iter().map(option).collect(),
        value: Some(value_to_proto(&f.value)),
    }
}

fn ms(t: urushi_core::Timestamp) -> i64 {
    i64::try_from(t.unix_millis()).unwrap_or_default()
}

/// Details on the wire; `me` (the signed-in account's id) marks its own comments, and
/// `site` gives linked issues their addresses.
fn details(task_id: String, d: IssueDetails, me: &str, site: &str) -> v1::JiraIssueDetails {
    v1::JiraIssueDetails {
        task_id,
        fields: d.fields.iter().map(field).collect(),
        transitions: d
            .transitions
            .iter()
            .map(|t| v1::JiraTransition {
                id: t.id.clone(),
                name: t.name.clone(),
                to_status: t.to_status.clone(),
                to_category: convert::category_to_proto(t.to_category),
            })
            .collect(),
        comments: d
            .comments
            .iter()
            .map(|c| v1::JiraComment {
                id: c.id.clone(),
                author: Some(user(&c.author)),
                body: c.body.clone(),
                created_ms: ms(c.created),
                updated_ms: ms(c.updated),
                mine: !me.is_empty() && c.author.account_id == me,
            })
            .collect(),
        attachments: d
            .attachments
            .iter()
            .map(|a| v1::JiraAttachment {
                id: a.id.clone(),
                name: a.name.clone(),
                media_type: a.media_type.clone(),
                size: a.size,
                created_ms: ms(a.created),
                author: a.author.as_ref().map(user),
            })
            .collect(),
        links: d
            .links
            .iter()
            .map(|l| v1::JiraLink {
                id: l.id.clone(),
                relation: l.relation.clone(),
                issue: Some(issue_ref(&l.issue, site)),
            })
            .collect(),
        link_types: d
            .link_types
            .iter()
            .map(|t| v1::JiraLinkType {
                name: t.name.clone(),
                inward: t.inward.clone(),
                outward: t.outward.clone(),
            })
            .collect(),
        watchers: d.watchers.iter().map(user).collect(),
        watching: d.watching,
        worklogs: d
            .worklogs
            .iter()
            .map(|w| v1::JiraWorklog {
                id: w.id.clone(),
                author: Some(user(&w.author)),
                seconds: w.seconds,
                started_ms: ms(w.started),
                comment: w.comment.clone(),
            })
            .collect(),
        original_estimate: d.time.original,
        remaining_estimate: d.time.remaining,
        time_spent: d.time.spent,
    }
}

fn issue_ref(i: &crate::jira::details::IssueRef, site: &str) -> v1::JiraIssueRef {
    v1::JiraIssueRef {
        id: i.id.clone(),
        key: i.key.clone(),
        summary: i.summary.clone(),
        status: i.status.clone(),
        type_name: i.type_name.clone(),
        url: format!("{site}/browse/{}", i.key),
    }
}

fn change(c: v1::change_jira_issue_request::Change) -> IssueChange {
    use v1::change_jira_issue_request::Change as C;
    match c {
        C::Edit(e) => IssueChange::Edit(edits_from_proto(&e.edits)),
        C::TransitionId(t) => IssueChange::Transition(t),
        C::AddComment(body) => IssueChange::AddComment(body),
        C::UpdateComment(e) => IssueChange::UpdateComment {
            id: e.comment_id,
            body: e.body,
        },
        C::DeleteCommentId(id) => IssueChange::DeleteComment(id),
        C::Attach(a) => IssueChange::Attach {
            name: a.name,
            data: a.data,
        },
        C::DeleteAttachmentId(id) => IssueChange::DeleteAttachment(id),
        C::Link(l) => IssueChange::Link {
            kind: l.link_type,
            key: l.issue_key,
            outward: l.outward,
        },
        C::UnlinkId(id) => IssueChange::Unlink(id),
        C::Watch(w) => IssueChange::Watch(w),
        C::LogWork(w) => IssueChange::LogWork {
            seconds: w.seconds,
            started: urushi_core::Timestamp::from_unix_millis(
                u64::try_from(w.started_ms).unwrap_or_default(),
            ),
            comment: w.comment,
        },
    }
}

fn attachment_data(id: String, data: AttachmentData) -> v1::AttachmentData {
    match data {
        AttachmentData::File { media_type, bytes } => v1::AttachmentData {
            attachment_id: id,
            media_type,
            data: bytes,
            width: 0,
            height: 0,
        },
        AttachmentData::Thumbnail {
            width,
            height,
            rgba,
        } => v1::AttachmentData {
            attachment_id: id,
            media_type: "image/x-rgba".into(),
            data: rgba,
            width,
            height,
        },
    }
}

/// Handles the requests about one Jira issue.
pub(super) async fn route(service: &Service, body: Body) -> ServiceResult<Outcome> {
    let me = service
        .jira_sign_in()
        .map(|s| (s.account_id, s.site))
        .unwrap_or_default();
    Ok(match body {
        Body::GetJiraIssue(r) => {
            let d = service
                .jira_issue_details(parse(&r.task_id, "task_id")?)
                .await?;
            Outcome::JiraIssueDetails(details(r.task_id, d, &me.0, &me.1))
        }
        Body::GetJiraCreateFields(r) => {
            let kind = convert::kind_from_proto(r.kind).unwrap_or(urushi_core::TaskKind::Task);
            let fields = service
                .jira_create_fields(parse(&r.board_id, "board_id")?, kind)
                .await?;
            Outcome::JiraIssueDetails(v1::JiraIssueDetails {
                fields: fields.iter().map(field).collect(),
                ..v1::JiraIssueDetails::default()
            })
        }
        Body::ChangeJiraIssue(r) => {
            let Some(c) = r.change else {
                return Ok(invalid("no change"));
            };
            let d = service
                .change_jira_issue(parse(&r.task_id, "task_id")?, change(c))
                .await?;
            Outcome::JiraIssueDetails(details(r.task_id, d, &me.0, &me.1))
        }
        Body::GetJiraAttachment(r) => {
            let data = service
                .jira_attachment(parse(&r.task_id, "task_id")?, &r.attachment_id, r.thumbnail)
                .await?;
            Outcome::AttachmentData(attachment_data(r.attachment_id, data))
        }
        Body::SearchJiraUsers(r) => {
            let task = if r.task_id.is_empty() {
                None
            } else {
                Some(parse(&r.task_id, "task_id")?)
            };
            let users = service
                .search_jira_users(task, &r.query, r.assignable)
                .await?;
            Outcome::JiraUsers(v1::JiraUsers {
                users: users.iter().map(user).collect(),
            })
        }
        Body::SearchJiraIssues(r) => {
            let issues = service
                .search_jira_issues(parse(&r.board_id, "board_id")?, &r.query)
                .await?;
            Outcome::JiraIssueRefs(v1::JiraIssueRefs {
                issues: issues.iter().map(|i| issue_ref(i, &me.1)).collect(),
            })
        }
        _ => invalid("not a Jira issue request"),
    })
}
