//! One Jira issue in full, end to end against an in-memory Jira: the editor's fields
//! and conversation, changing any field, comments, attachments, links, watching, work
//! log, searches, and new issues with the fields their project requires.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

mod fake_jira;

use std::sync::Arc;

use common::{Daemon, temp_home};
use fake_jira::{FakeJira, TEAM, TOKEN};
use urushi_client::v1::change_jira_issue_request::Change;
use urushi_client::{Client, v1};
use urushi_daemon::DaemonOptions;
use urushi_daemon::tailscale::StaticTailscale;

async fn signed_in(home: &std::path::Path) -> (Daemon, Client, Arc<FakeJira>, v1::Board) {
    let jira = FakeJira::new();
    let mut options = DaemonOptions::new(home.join("home"));
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.jira = jira.clone();
    let daemon = Daemon::start_options(options).await;
    let (client, _events) = daemon.connect().await;
    client.connect_jira("acme", "a@b.c", TOKEN).await.unwrap();
    let imported = client.import_jira_board(7).await.unwrap();
    client.sync_board(&imported.id).await.unwrap();
    let w = client.list_workspace(false).await.unwrap();
    let board = w.boards.into_iter().find(|b| b.jira.is_some()).unwrap();
    (daemon, client, jira, board)
}

fn task_id(board: &v1::Board, key: &str) -> String {
    board
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .find(|t| t.issue.as_ref().is_some_and(|i| i.key == key))
        .unwrap()
        .id
        .clone()
}

fn value(options: &[(&str, &str)], text: &str) -> v1::JiraFieldValue {
    v1::JiraFieldValue {
        text: text.into(),
        options: options
            .iter()
            .map(|(id, name)| v1::JiraOption {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect(),
    }
}

fn edit(field: &str, value: v1::JiraFieldValue) -> Change {
    Change::Edit(v1::JiraFieldEdits {
        edits: vec![v1::JiraFieldEdit {
            field_id: field.into(),
            value: Some(value),
        }],
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn any_field_changes_in_jira_and_the_card_follows() {
    let home = temp_home();
    let (daemon, client, _jira, b) = signed_in(home.path()).await;
    let id = task_id(&b, "ACME-1");
    let d = client.jira_issue(&id).await.unwrap();
    let names: Vec<&str> = d.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Assignee", "Priority", "Due date"]);
    assert_eq!(d.fields[1].allowed.len(), 3);
    // Reassign, reprioritize and give it a due date.
    client
        .change_jira_issue(&id, edit("assignee", value(&[("acc-grace", "Grace")], "")))
        .await
        .unwrap();
    client
        .change_jira_issue(&id, edit("priority", value(&[("1", "Highest")], "")))
        .await
        .unwrap();
    let d = client
        .change_jira_issue(&id, edit("duedate", value(&[], "2026-12-01")))
        .await
        .unwrap();
    assert_eq!(d.fields[2].value.as_ref().unwrap().text, "2026-12-01");
    let w = client.list_workspace(false).await.unwrap();
    let card = w
        .boards
        .iter()
        .flat_map(|b| &b.columns)
        .flat_map(|c| &c.tasks)
        .find(|t| t.id == id)
        .unwrap();
    let issue = card.issue.as_ref().unwrap();
    assert_eq!(
        (issue.assignee.as_str(), issue.assignee_id.as_str()),
        ("Grace", "acc-grace")
    );
    assert_eq!(
        (issue.priority.as_str(), issue.due.as_str()),
        ("Highest", "2026-12-01")
    );
    // Unassign: an empty value clears.
    client
        .change_jira_issue(&id, edit("assignee", value(&[], "")))
        .await
        .unwrap();
    // What Jira (or Urushi) can't take changes nothing.
    let bad_date = client
        .change_jira_issue(&id, edit("duedate", value(&[], "next week")))
        .await
        .unwrap_err();
    assert_eq!(bad_date.code(), Some(v1::ErrorCode::InvalidArgument));
    let unknown = client
        .change_jira_issue(&id, edit("customfield_404", value(&[], "x")))
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), Some(v1::ErrorCode::InvalidArgument));
    let d = client.jira_issue(&id).await.unwrap();
    assert!(d.fields[0].value.as_ref().unwrap().options.is_empty());
    assert_eq!(d.fields[2].value.as_ref().unwrap().text, "2026-12-01");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn comments_and_files_are_kept_in_jira() {
    let home = temp_home();
    let (daemon, client, _jira, b) = signed_in(home.path()).await;
    let id = task_id(&b, "ACME-1");
    let d = client
        .change_jira_issue(&id, Change::AddComment("Looks **good**".into()))
        .await
        .unwrap();
    assert_eq!(d.comments.len(), 1);
    assert!(d.comments[0].mine, "written by the signed-in account");
    let comment = d.comments[0].id.clone();
    let d = client
        .change_jira_issue(
            &id,
            Change::UpdateComment(v1::JiraCommentEdit {
                comment_id: comment.clone(),
                body: "Ship it".into(),
            }),
        )
        .await
        .unwrap();
    assert_eq!(d.comments[0].body, "Ship it");
    let d = client
        .change_jira_issue(&id, Change::DeleteCommentId(comment))
        .await
        .unwrap();
    assert!(d.comments.is_empty());

    let d = client
        .change_jira_issue(
            &id,
            Change::Attach(v1::JiraNewAttachment {
                name: "plan.txt".into(),
                data: b"step one".to_vec(),
            }),
        )
        .await
        .unwrap();
    let file = d.attachments[0].clone();
    assert_eq!(file.name, "plan.txt");
    let data = client.jira_attachment(&id, &file.id, false).await.unwrap();
    assert_eq!(data.data, b"step one");
    let not_image = client
        .jira_attachment(&id, &file.id, true)
        .await
        .unwrap_err();
    assert_eq!(not_image.code(), Some(v1::ErrorCode::FailedPrecondition));
    let path = client
        .change_jira_issue(
            &id,
            Change::Attach(v1::JiraNewAttachment {
                name: "../../x.txt".into(),
                data: Vec::new(),
            }),
        )
        .await
        .unwrap();
    assert_eq!(path.attachments[1].name, "x.txt", "only a path's last part");
    let blank = client
        .change_jira_issue(
            &id,
            Change::Attach(v1::JiraNewAttachment {
                name: "..".into(),
                data: Vec::new(),
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(blank.code(), Some(v1::ErrorCode::InvalidArgument));

    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn links_watching_and_work_are_kept_in_jira() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(home.path()).await;
    let id = task_id(&b, "ACME-1");
    let found = client.search_jira_issues(&b.id, "Invoices").await.unwrap();
    assert_eq!(found[0].key, "ACME-2");
    let d = client
        .change_jira_issue(
            &id,
            Change::Link(v1::JiraNewLink {
                link_type: "Blocks".into(),
                issue_key: "ACME-2".into(),
                outward: true,
            }),
        )
        .await
        .unwrap();
    assert_eq!(d.links[0].relation, "blocks");
    assert_eq!(d.links[0].issue.as_ref().unwrap().key, "ACME-2");
    assert!(
        jira.calls()
            .contains(&"link ACME-1 Blocks ACME-2".to_owned())
    );
    let other = client.jira_issue(&task_id(&b, "ACME-2")).await.unwrap();
    assert_eq!(other.links[0].relation, "is blocked by");
    let d = client
        .change_jira_issue(&id, Change::UnlinkId(d.links[0].id.clone()))
        .await
        .unwrap();
    assert!(d.links.is_empty());

    let d = client
        .change_jira_issue(&id, Change::Watch(true))
        .await
        .unwrap();
    assert!(d.watching && d.watchers.len() == 1);
    let d = client
        .change_jira_issue(
            &id,
            Change::LogWork(v1::JiraNewWorklog {
                seconds: 5400,
                started_ms: 1_794_219_630_123,
                comment: "pairing".into(),
            }),
        )
        .await
        .unwrap();
    assert_eq!(d.worklogs[0].seconds, 5400);
    assert!(
        jira.calls()
            .contains(&"log_work ACME-1 5400 2026-11-09T10:20:30.123+0000".to_owned())
    );
    let people = client.search_jira_users(&id, "gra", true).await.unwrap();
    assert_eq!(people[0].account_id, "acc-grace");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn new_issues_take_the_fields_their_project_requires() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(home.path()).await;
    let fields = client
        .jira_create_fields(&b.id, v1::TaskKind::Story)
        .await
        .unwrap();
    assert!(fields[0].required && fields[0].id == TEAM);
    let task = client
        .create_task_with(v1::CreateTaskRequest {
            board_id: b.id.clone(),
            column_id: b.columns[0].id.clone(),
            title: "With a team".into(),
            kind: v1::TaskKind::Story.into(),
            jira_fields: vec![v1::JiraFieldEdit {
                field_id: TEAM.into(),
                value: Some(value(&[("t2", "Mobile")], "")),
            }],
            ..Default::default()
        })
        .await
        .unwrap();
    let key = task.issue.as_ref().unwrap().key.clone();
    assert_eq!(jira.remote(&key).extra.team.as_deref(), Some("t2"));
    // Not a Jira issue: refused before asking Jira.
    let w = client.list_workspace(false).await.unwrap();
    assert!(w.boards.iter().all(|b| b.jira.is_some()));
    daemon.stop().await;
}
