//! Several Jira sign-ins (UR-61, ADR 0051), end to end against an in-memory Jira with
//! two sites: each board is read and written with its own sign-in, signing out of one
//! leaves the other's boards syncing, and a sign-in and boards from before there could
//! be several keep working.
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
use fake_jira::{FakeJira, TOKEN};
use urushi_client::{Client, ClientError, v1};
use urushi_daemon::DaemonOptions;
use urushi_daemon::tailscale::StaticTailscale;

const ACME: &str = "https://acme.atlassian.net";
const GLOBEX: &str = "https://globex.atlassian.net";
const GLOBEX_TOKEN: &str = "g10bex-t0ken";

async fn start(home: &std::path::Path, jira: Arc<FakeJira>) -> Daemon {
    let mut options = DaemonOptions::new(home.join("home"));
    options.tailscale = Arc::new(StaticTailscale::not_installed());
    options.jira = jira;
    Daemon::start_options(options).await
}

fn code(e: &ClientError) -> Option<v1::ErrorCode> {
    e.code()
}

async fn boards(client: &Client) -> Vec<v1::Board> {
    let w = client.list_workspace(false).await.unwrap();
    w.boards.into_iter().filter(|b| b.jira.is_some()).collect()
}

async fn board(client: &Client, id: &str) -> v1::Board {
    boards(client)
        .await
        .into_iter()
        .find(|b| b.id == id)
        .unwrap()
}

/// A daemon signed in to both sites, with board 7 of each imported and synced.
async fn two_sites(home: &std::path::Path) -> (Daemon, Client, Arc<FakeJira>, [v1::Board; 2]) {
    let jira = FakeJira::new();
    jira.add_site(GLOBEX, GLOBEX_TOKEN);
    let daemon = start(home, jira.clone()).await;
    let (client, _events) = daemon.connect().await;
    let acme = client.connect_jira("acme", "a@b.c", TOKEN).await.unwrap();
    let globex = client
        .connect_jira("globex", "A@B.C", GLOBEX_TOKEN)
        .await
        .unwrap();
    assert_eq!(acme.key, "acme.atlassian.net/a@b.c");
    assert_eq!(globex.key, "globex.atlassian.net/a@b.c");
    let a = client.import_jira_board(&acme.key, 7).await.unwrap();
    let g = client.import_jira_board(&globex.key, 7).await.unwrap();
    assert_ne!(a.id, g.id, "the same board id on two sites is two boards");
    client.sync_board(&a.id).await.unwrap();
    client.sync_board(&g.id).await.unwrap();
    (daemon, client, jira, [a, g])
}

#[tokio::test(flavor = "multi_thread")]
async fn each_board_is_synced_and_written_with_its_own_sign_in() {
    let home = temp_home();
    let (daemon, client, jira, [a, g]) = two_sites(home.path()).await;
    let accounts = client.jira_accounts().await.unwrap();
    let keys: Vec<&str> = accounts.iter().map(|a| a.key.as_str()).collect();
    assert_eq!(
        keys,
        ["acme.atlassian.net/a@b.c", "globex.atlassian.net/a@b.c"]
    );
    assert!(accounts.iter().all(|a| a.connected));
    // Clients before 1.29 see the first.
    assert_eq!(client.jira_account().await.unwrap().key, keys[0]);

    let (a, g) = (board(&client, &a.id).await, board(&client, &g.id).await);
    assert_eq!(a.jira.as_ref().unwrap().account, keys[0]);
    assert_eq!(g.jira.as_ref().unwrap().account, keys[1]);
    assert_eq!(g.jira.as_ref().unwrap().site, GLOBEX);
    assert!(!a.columns.iter().all(|c| c.tasks.is_empty()));
    assert!(!g.columns.iter().all(|c| c.tasks.is_empty()));

    // Reading and writing go to the board's own site.
    let made = |jira: &FakeJira, site: &str, call: &str| {
        jira.site_calls().contains(&format!("{site} {call}"))
    };
    let bugs = format!("issue_ids {:?}", [fake_jira::BUGS]);
    client.query_jira_filter(&g.id, &[1]).await.unwrap();
    assert!(made(&jira, GLOBEX, &bugs) && !made(&jira, ACME, &bugs));
    client.query_jira_filter(&a.id, &[1]).await.unwrap();
    assert!(made(&jira, ACME, &bugs));
    for site in [ACME, GLOBEX] {
        assert!(made(&jira, site, "issues full=true"), "{site} synced");
    }

    // Each site lists its own boards, marked as imported there.
    let listed = client.list_jira_boards(keys[1], "").await.unwrap();
    assert_eq!(listed[0].imported_board_id, g.id);
    let unknown = client
        .list_jira_boards("nowhere.atlassian.net/x@y.z", "")
        .await
        .unwrap_err();
    assert_eq!(code(&unknown), Some(v1::ErrorCode::NotFound));

    // Signing in again with the same site and email replaces, not adds.
    client
        .connect_jira("globex", "a@b.c", GLOBEX_TOKEN)
        .await
        .unwrap();
    assert_eq!(client.jira_accounts().await.unwrap().len(), 2);
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_out_of_one_site_leaves_the_others_boards_syncing() {
    let home = temp_home();
    let (daemon, client, _jira, [a, g]) = two_sites(home.path()).await;
    client
        .disconnect_jira("globex.atlassian.net/a@b.c")
        .await
        .unwrap();
    let accounts = client.jira_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].site, ACME);
    let g = board(&client, &g.id).await;
    assert_eq!(
        g.jira.as_ref().unwrap().error,
        "Not signed in to Jira at globex.atlassian.net"
    );
    let refused = client.sync_board(&g.id).await.unwrap_err();
    assert_eq!(code(&refused), Some(v1::ErrorCode::FailedPrecondition));
    client.sync_board(&a.id).await.unwrap();
    assert_eq!(board(&client, &a.id).await.jira.unwrap().error, "");
    // Signing in again brings the board back.
    client
        .connect_jira("globex", "a@b.c", GLOBEX_TOKEN)
        .await
        .unwrap();
    client.sync_board(&g.id).await.unwrap();
    assert_eq!(board(&client, &g.id).await.jira.unwrap().error, "");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_ins_survive_restarts_and_their_tokens_never_leave_the_daemon() {
    let home = temp_home();
    let (daemon, client, jira, [a, g]) = two_sites(home.path()).await;
    let workspace = format!("{:?}", client.list_workspace(false).await.unwrap());
    let accounts = format!("{:?}", client.jira_accounts().await.unwrap());
    for token in [TOKEN, GLOBEX_TOKEN] {
        assert!(!workspace.contains(token) && !accounts.contains(token));
    }
    drop(client);
    daemon.stop().await;
    let document = std::fs::read_to_string(home.path().join("home/workspace.json")).unwrap();
    assert!(!document.contains(TOKEN) && !document.contains(GLOBEX_TOKEN));

    let daemon = start(home.path(), jira).await;
    let (client, _events) = daemon.connect().await;
    assert_eq!(client.jira_accounts().await.unwrap().len(), 2);
    client.sync_board(&a.id).await.unwrap();
    client.sync_board(&g.id).await.unwrap();
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_and_boards_from_before_several_sign_ins_keep_working() {
    let home = temp_home();
    let jira = FakeJira::new();
    let daemon = start(home.path(), jira.clone()).await;
    let (client, _events) = daemon.connect().await;
    let account = client.connect_jira("acme", "a@b.c", TOKEN).await.unwrap();
    let b = client.import_jira_board("", 7).await.unwrap();
    client.sync_board(&b.id).await.unwrap();
    drop(client);
    daemon.stop().await;

    // As an older daemon wrote them: one bare sign-in, and boards that name none.
    let dir = home.path().join("home");
    let file: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("jira-credentials.json")).unwrap()).unwrap();
    std::fs::write(
        dir.join("jira-credentials.json"),
        serde_json::to_vec(&file["accounts"][0]).unwrap(),
    )
    .unwrap();
    let mut doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("workspace.json")).unwrap()).unwrap();
    for board in doc["boards"].as_array_mut().unwrap() {
        board["source"].as_object_mut().unwrap().remove("account");
    }
    std::fs::write(
        dir.join("workspace.json"),
        serde_json::to_vec(&doc).unwrap(),
    )
    .unwrap();

    let daemon = start(home.path(), jira).await;
    let (client, _events) = daemon.connect().await;
    let accounts = client.jira_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].key, account.key);
    let old = board(&client, &b.id).await;
    assert_eq!(old.jira.as_ref().unwrap().account, "");
    // The board uses the sign-in for its site.
    client.sync_board(&b.id).await.unwrap();
    assert_eq!(board(&client, &b.id).await.jira.unwrap().error, "");
    daemon.stop().await;
}
