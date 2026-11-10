//! Quick filters end to end (protocol 1.23, ADR 0037): a board of Urushi's own keeps
//! them; a Jira board's are written to Jira and stay through syncs, and one Jira won't
//! take is kept here as JQL Jira still evaluates.
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
use fake_jira::{BAD_JQL, BUGS, FakeJira, TOKEN};
use urushi_client::{Client, ClientError, jira_filter, kept_filter, v1};
use urushi_daemon::DaemonOptions;
use urushi_daemon::jira::JiraError;
use urushi_daemon::tailscale::StaticTailscale;

async fn start(home: &std::path::Path, jira: Arc<FakeJira>) -> Daemon {
    let mut options = DaemonOptions::new(home.to_path_buf());
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.jira = jira;
    Daemon::start_options(options).await
}

async fn board(client: &Client, id: &str) -> v1::Board {
    let w = client.list_workspace(false).await.unwrap();
    w.boards.into_iter().find(|b| b.id == id).unwrap()
}

fn code(error: &ClientError) -> Option<v1::ErrorCode> {
    error.code()
}

fn fields(search: &str, kinds: &[v1::TaskKind], tags: &[&str]) -> v1::QuickFilterRule {
    v1::QuickFilterRule {
        search: search.into(),
        kinds: kinds.iter().map(|k| (*k).into()).collect(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        jql: String::new(),
    }
}

fn jql(q: &str) -> v1::QuickFilterRule {
    v1::QuickFilterRule {
        jql: q.into(),
        ..Default::default()
    }
}

fn kept_id(saved: &v1::QuickFilterSaved) -> String {
    match saved.filter.as_ref().and_then(|f| f.filter.as_ref()) {
        Some(v1::quick_filter_ref::Filter::Id(id)) => id.clone(),
        other => panic!("not kept here: {other:?}"),
    }
}

fn jira_id(saved: &v1::QuickFilterSaved) -> u64 {
    match saved.filter.as_ref().and_then(|f| f.filter.as_ref()) {
        Some(v1::quick_filter_ref::Filter::JiraId(id)) => *id,
        other => panic!("not in Jira: {other:?}"),
    }
}

fn jira_names(b: &v1::Board) -> Vec<String> {
    b.jira
        .as_ref()
        .unwrap()
        .quick_filters
        .iter()
        .map(|q| q.name.clone())
        .collect()
}

/// A daemon signed in to the fake Jira, with its board imported and synced.
async fn signed_in(home: &std::path::Path) -> (Daemon, Client, Arc<FakeJira>, String) {
    let jira = FakeJira::new();
    let daemon = start(home, jira.clone()).await;
    let (client, _events) = daemon.connect().await;
    client.connect_jira("acme", "a@b.c", TOKEN).await.unwrap();
    let imported = client.import_jira_board(7).await.unwrap();
    client.sync_board(&imported.id).await.unwrap();
    (daemon, client, jira, imported.id)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_board_of_its_own_keeps_its_quick_filters_across_restarts() {
    let home = temp_home();
    let dir = home.path().join("home");
    let daemon = start(&dir, FakeJira::new()).await;
    let (client, _events) = daemon.connect().await;
    let b = client.create_board("Work").await.unwrap();

    let saved = client
        .create_quick_filter(
            &b.id,
            " My  bugs ",
            fields("login", &[v1::TaskKind::Bug], &["ui"]),
        )
        .await
        .unwrap();
    assert!(saved.kept_here.is_empty(), "a board of its own isn't Jira");
    let id = kept_id(&saved);
    let shown = board(&client, &b.id).await;
    assert_eq!(shown.quick_filters.len(), 1);
    assert_eq!(shown.quick_filters[0].name, "My bugs");
    let rule = shown.quick_filters[0].rule.as_ref().unwrap();
    assert_eq!(
        (rule.search.as_str(), rule.tags.as_slice()),
        ("login", ["ui".to_owned()].as_slice())
    );

    // Refused: JQL here, a rule filtering by nothing, a blank name, an unknown filter.
    for (name, rule) in [
        ("J", jql("type = Bug")),
        ("E", fields(" ", &[], &[])),
        (" ", fields("x", &[], &[])),
    ] {
        let err = client
            .create_quick_filter(&b.id, name, rule)
            .await
            .unwrap_err();
        assert_eq!(code(&err), Some(v1::ErrorCode::InvalidArgument), "{err:?}");
    }
    let unknown = kept_filter(&urushi_core::QuickFilterId::from_u128(9).to_string());
    let err = client
        .update_quick_filter(&b.id, unknown.clone(), "X", None)
        .await
        .unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::NotFound));
    let err = client
        .delete_quick_filter(&b.id, unknown)
        .await
        .unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::NotFound));
    assert_eq!(board(&client, &b.id).await.quick_filters.len(), 1);

    client
        .update_quick_filter(
            &b.id,
            kept_filter(&id),
            "Crashes",
            Some(fields("crash", &[], &[])),
        )
        .await
        .unwrap();
    drop(client);
    daemon.stop().await;

    let daemon = start(&dir, FakeJira::new()).await;
    let (client, _events) = daemon.connect().await;
    let shown = board(&client, &b.id).await;
    assert_eq!(shown.quick_filters[0].name, "Crashes");
    assert_eq!(
        shown.quick_filters[0].rule.as_ref().unwrap().search,
        "crash"
    );
    client
        .delete_quick_filter(&b.id, kept_filter(&id))
        .await
        .unwrap();
    assert!(board(&client, &b.id).await.quick_filters.is_empty());
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_jira_boards_quick_filters_are_made_changed_and_deleted_in_jira() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(&home.path().join("home")).await;

    let saved = client
        .create_quick_filter(&b, "Stories", jql("type = Story"))
        .await
        .unwrap();
    assert!(saved.kept_here.is_empty());
    let id = jira_id(&saved);
    assert!(
        jira.calls()
            .contains(&"create_quick_filter 7 Stories".to_owned())
    );
    let shown = board(&client, &b).await;
    assert_eq!(jira_names(&shown), ["Bugs", "Stories"]);
    assert!(shown.quick_filters.is_empty(), "nothing is kept here");
    // The JQL travels, for editing.
    assert_eq!(
        shown.jira.as_ref().unwrap().quick_filters[1].jql,
        "type = Story"
    );
    // Jira evaluates it like any of its own.
    assert_eq!(client.query_jira_filter(&b, &[id]).await.unwrap(), ["1001"]);

    // A full sync reads the same list back: nothing is lost.
    client.sync_board(&b).await.unwrap();
    assert_eq!(jira_names(&board(&client, &b).await), ["Bugs", "Stories"]);

    client
        .update_quick_filter(&b, jira_filter(id), "Only stories", None)
        .await
        .unwrap();
    assert_eq!(
        jira_names(&board(&client, &b).await),
        ["Bugs", "Only stories"]
    );
    assert_eq!(jira.quick_filters.lock().unwrap()[1].jql, "type = Story");

    // A filter made in Jira itself can be changed and deleted too.
    client
        .delete_quick_filter(&b, jira_filter(1))
        .await
        .unwrap();
    assert_eq!(jira_names(&board(&client, &b).await), ["Only stories"]);
    assert!(jira.calls().contains(&"delete_quick_filter 7 1".to_owned()));
    let err = client
        .delete_quick_filter(&b, jira_filter(1))
        .await
        .unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::NotFound));
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn jql_jira_cant_read_and_rules_without_jql_are_refused_before_writing() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(&home.path().join("home")).await;
    for rule in [jql(BAD_JQL), fields("x", &[], &[])] {
        let err = client
            .create_quick_filter(&b, "Bad", rule)
            .await
            .unwrap_err();
        assert_eq!(code(&err), Some(v1::ErrorCode::InvalidArgument), "{err:?}");
    }
    let err = client
        .update_quick_filter(&b, jira_filter(1), "", Some(jql(BAD_JQL)))
        .await
        .unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::InvalidArgument), "{err:?}");
    assert!(!jira.calls().iter().any(|c| c.contains("_quick_filter")));
    let shown = board(&client, &b).await;
    assert_eq!(jira_names(&shown), ["Bugs"]);
    assert_eq!(shown.jira.as_ref().unwrap().quick_filters[0].jql, BUGS);
    assert!(shown.quick_filters.is_empty());
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_filter_jira_wont_take_is_kept_here_and_jira_still_evaluates_it() {
    let home = temp_home();
    let (daemon, client, jira, b) = signed_in(&home.path().join("home")).await;
    for refusal in [
        JiraError::Forbidden("you need to be a board admin".into()),
        JiraError::NotFound("quickfilters".into()),
    ] {
        jira.refuse_quick_filter_writes(Some(refusal));
        let saved = client
            .create_quick_filter(&b, "Stories", jql("type = Story"))
            .await
            .unwrap();
        assert!(!saved.kept_here.is_empty());
        let id = kept_id(&saved);
        let shown = board(&client, &b).await;
        assert_eq!(jira_names(&shown), ["Bugs"]);
        assert_eq!(shown.quick_filters.len(), 1);
        assert_eq!(
            shown.quick_filters[0].rule.as_ref().unwrap().jql,
            "type = Story"
        );
        // It survives a full sync, which replaces Jira's own list.
        client.sync_board(&b).await.unwrap();
        assert_eq!(board(&client, &b).await.quick_filters.len(), 1);
        let ids = client
            .query_quick_filters(&b, &[], std::slice::from_ref(&id))
            .await
            .unwrap();
        assert_eq!(ids, ["1001"]);
        // Changing and deleting it never asks Jira.
        let before = jira.calls().len();
        client
            .update_quick_filter(&b, kept_filter(&id), "S", Some(jql("type = Bug")))
            .await
            .unwrap();
        client
            .delete_quick_filter(&b, kept_filter(&id))
            .await
            .unwrap();
        assert!(
            jira.calls()[before..]
                .iter()
                .all(|c| !c.contains("_quick_filter"))
        );
        assert!(board(&client, &b).await.quick_filters.is_empty());
    }

    // Jira out of reach is an error, not a reason to keep it here.
    jira.refuse_quick_filter_writes(Some(JiraError::Unavailable("timeout".into())));
    let err = client
        .create_quick_filter(&b, "Stories", jql("type = Story"))
        .await
        .unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::Unavailable), "{err:?}");
    // And edits Jira refuses change nothing.
    jira.refuse_quick_filter_writes(Some(JiraError::Forbidden("no".into())));
    let err = client
        .update_quick_filter(&b, jira_filter(1), "Mine", None)
        .await
        .unwrap_err();
    assert_eq!(code(&err), Some(v1::ErrorCode::PermissionDenied), "{err:?}");
    let shown = board(&client, &b).await;
    assert_eq!(jira_names(&shown), ["Bugs"]);
    assert!(shown.quick_filters.is_empty());
    daemon.stop().await;
}
