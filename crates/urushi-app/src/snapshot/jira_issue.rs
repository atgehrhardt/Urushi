//! A Jira issue in full, as the daemon would send it for the `jira-issue` scenarios:
//! Invoice PDFs (ACME-139) with its fields, a conversation, a file and a link.

use urushi_client::v1;

fn user(id: &str, name: &str) -> v1::JiraUser {
    v1::JiraUser {
        account_id: id.into(),
        name: name.into(),
    }
}

fn option(id: &str, name: &str) -> v1::JiraOption {
    v1::JiraOption {
        id: id.into(),
        name: name.into(),
    }
}

fn field(
    (id, name): (&str, &str),
    kind: v1::JiraFieldKind,
    allowed: Vec<v1::JiraOption>,
    (text, options): (&str, Vec<v1::JiraOption>),
) -> v1::JiraField {
    v1::JiraField {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        required: false,
        allowed,
        value: Some(v1::JiraFieldValue {
            text: text.into(),
            options,
        }),
    }
}

/// People on the site.
pub(super) fn people() -> Vec<v1::JiraUser> {
    vec![
        user("acc-ada-lovelace", "Ada Lovelace"),
        user("acc-grace-hopper", "Grace Hopper"),
        user("acc-alan", "Alan Turing"),
    ]
}

/// Its fields, one of each kind the editor draws.
fn fields() -> Vec<v1::JiraField> {
    use v1::JiraFieldKind as K;
    vec![
        field(
            ("issuetype", "Issue Type"),
            K::Option,
            vec![
                option("1", "Story"),
                option("2", "Bug"),
                option("3", "Task"),
            ],
            ("", vec![option("1", "Story")]),
        ),
        field(
            ("assignee", "Assignee"),
            K::User,
            Vec::new(),
            ("", vec![option("acc-grace-hopper", "Grace Hopper")]),
        ),
        field(
            ("priority", "Priority"),
            K::Option,
            vec![
                option("1", "Highest"),
                option("2", "High"),
                option("3", "Medium"),
            ],
            ("", vec![option("3", "Medium")]),
        ),
        field(
            ("parent", "Parent"),
            K::Issue,
            Vec::new(),
            ("ACME-8", vec![option("ACME-8", "Billing")]),
        ),
        field(
            ("duedate", "Due date"),
            K::Date,
            Vec::new(),
            ("2026-12-01", Vec::new()),
        ),
        field(
            ("components", "Components"),
            K::Options,
            vec![option("10", "API"), option("11", "Web")],
            ("", vec![option("10", "API")]),
        ),
        field(
            ("customfield_10020", "Sprint"),
            K::Sprint,
            vec![option("6", "Sprint 6"), option("7", "Sprint 7")],
            ("", vec![option("6", "Sprint 6")]),
        ),
        field(
            ("customfield_10016", "Story points"),
            K::Number,
            Vec::new(),
            ("5", Vec::new()),
        ),
    ]
}

/// The issue in full.
pub(super) fn details() -> v1::JiraIssueDetails {
    v1::JiraIssueDetails {
        task_id: "acme-139".into(),
        fields: fields(),
        transitions: vec![v1::JiraTransition {
            id: "31".into(),
            name: "Done".into(),
            to_status: "Done".into(),
            to_category: v1::JiraStatusCategory::Done.into(),
        }],
        comments: vec![
            v1::JiraComment {
                id: "1".into(),
                author: Some(user("acc-grace-hopper", "Grace Hopper")),
                body: "The template is in. Emails go out from the billing service.".into(),
                created_ms: 1_794_219_630_123,
                ..Default::default()
            },
            v1::JiraComment {
                id: "2".into(),
                author: Some(user("acc-ada-lovelace", "Ada Lovelace")),
                body: "Thanks! I'll check the **PDF fonts** before we ship.".into(),
                created_ms: 1_794_306_030_123,
                mine: true,
                ..Default::default()
            },
        ],
        attachments: vec![v1::JiraAttachment {
            id: "900".into(),
            name: "invoice-sample.pdf".into(),
            media_type: "application/pdf".into(),
            size: 48_213,
            ..Default::default()
        }],
        links: vec![v1::JiraLink {
            id: "77".into(),
            relation: "blocks".into(),
            issue: Some(v1::JiraIssueRef {
                id: "10151".into(),
                key: "ACME-151".into(),
                summary: "Crash when the cart is empty".into(),
                status: "To Do".into(),
                ..Default::default()
            }),
        }],
        link_types: vec![v1::JiraLinkType {
            name: "Blocks".into(),
            inward: "is blocked by".into(),
            outward: "blocks".into(),
        }],
        watchers: vec![user("acc-ada-lovelace", "Ada Lovelace")],
        watching: true,
        ..Default::default()
    }
}
