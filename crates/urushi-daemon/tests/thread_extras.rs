//! End-to-end: a thread's unread mark and notes (`UpdateThread`, protocol 1.27), the
//! notes' color (1.32) and a project's color (`SetFolderColor`): set and cleared over the wire, validated all or
//! nothing, kept across a daemon restart, and never touching the worktree.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use common::*;
use urushi_client::{Client, ClientError, v1};

async fn thread(client: &Client, id: &str) -> v1::Thread {
    let w = client.list_workspace(false).await.unwrap();
    w.threads.into_iter().find(|t| t.id == id).unwrap()
}

async fn folder(client: &Client, id: &str) -> v1::Folder {
    let w = client.list_workspace(false).await.unwrap();
    w.folders.into_iter().find(|f| f.id == id).unwrap()
}

fn is_code(err: &ClientError, wanted: v1::ErrorCode) -> bool {
    matches!(err, ClientError::Remote { code, .. } if *code == wanted)
}

#[tokio::test(flavor = "multi_thread")]
async fn unread_notes_and_project_colors_are_kept() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let created = client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: repo.id.clone(),
            title: "Invoices".into(),
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap();
    let id = created.id.clone();
    let update = |unread: Option<bool>, notes: Option<&str>| v1::UpdateThreadRequest {
        thread_id: id.clone(),
        unread,
        notes: notes.map(str::to_owned),
        ..Default::default()
    };
    let t = thread(&client, &id).await;
    assert!(!t.unread && t.notes.is_empty(), "{t:?}");

    // Unread, then read again.
    client
        .update_thread(update(Some(true), None))
        .await
        .unwrap();
    assert!(thread(&client, &id).await.unread);
    client
        .update_thread(update(Some(false), None))
        .await
        .unwrap();
    assert!(!thread(&client, &id).await.unread);

    // Notes lose trailing whitespace; other fields are left alone.
    client
        .update_thread(update(Some(true), Some("Ask about retries\nthen ship  \n")))
        .await
        .unwrap();
    let t = thread(&client, &id).await;
    assert_eq!(t.notes, "Ask about retries\nthen ship");
    assert!(t.unread);

    // Too long: the whole update is refused, the unread mark too.
    let long = "x".repeat(urushi_core::MAX_THREAD_NOTES + 1);
    let err = client
        .update_thread(update(Some(false), Some(&long)))
        .await
        .unwrap_err();
    assert!(is_code(&err, v1::ErrorCode::InvalidArgument), "{err:?}");
    let t = thread(&client, &id).await;
    assert!(
        t.unread && t.notes == "Ask about retries\nthen ship",
        "{t:?}"
    );

    // Project colors: set, unknown project, and kept for the restart below.
    let project = client.create_project("Billing").await.unwrap();
    assert_eq!(project.color, 0);
    client
        .set_project_color(&project.id, v1::LabelColor::Teal)
        .await
        .unwrap();
    assert_eq!(
        folder(&client, &project.id).await.color,
        v1::LabelColor::Teal as i32
    );
    let other = client.create_project("Ops").await.unwrap();
    client
        .set_project_color(&other.id, v1::LabelColor::Red)
        .await
        .unwrap();
    client
        .set_project_color(&other.id, v1::LabelColor::Unspecified)
        .await
        .unwrap();
    assert_eq!(folder(&client, &other.id).await.color, 0);
    let unknown = urushi_core::ProjectId::from_u128(9).to_string();
    let err = client
        .set_project_color(&unknown, v1::LabelColor::Blue)
        .await
        .unwrap_err();
    assert!(is_code(&err, v1::ErrorCode::NotFound), "{err:?}");

    // None of it touches the worktree.
    assert!(std::path::Path::new(&created.worktree_path).is_dir());

    drop(client);
    let home_dir = daemon.home.clone();
    daemon.stop().await;
    let daemon = Daemon::start(&home_dir).await;
    let (client, _events) = daemon.connect().await;
    let t = thread(&client, &id).await;
    assert!(
        t.unread && t.notes == "Ask about retries\nthen ship",
        "{t:?}"
    );
    assert_eq!(
        folder(&client, &project.id).await.color,
        v1::LabelColor::Teal as i32
    );

    // Clearing the notes.
    client.update_thread(update(None, Some(""))).await.unwrap();
    assert!(thread(&client, &id).await.notes.is_empty());
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_notes_color_is_kept_and_goes_with_the_notes() {
    let home = temp_home();
    let fixture = repo(home.path());
    let daemon = Daemon::start(&home.path().join("home")).await;
    let (client, _events) = daemon.connect().await;
    let repo = client
        .add_repo(fixture.repo.to_str().unwrap(), "")
        .await
        .unwrap();
    let id = client
        .create_thread_with(v1::CreateThreadRequest {
            project_id: repo.id.clone(),
            title: "Invoices".into(),
            skip_initial_tab: true,
            ..Default::default()
        })
        .await
        .unwrap()
        .id;
    let update = |notes: Option<&str>, color: Option<v1::LabelColor>| v1::UpdateThreadRequest {
        thread_id: id.clone(),
        notes: notes.map(str::to_owned),
        notes_color: color.map(Into::into),
        ..Default::default()
    };

    // Set with the notes, changed alone, and back to the default with none.
    client
        .update_thread(update(Some("**Ask** first"), Some(v1::LabelColor::Green)))
        .await
        .unwrap();
    let t = thread(&client, &id).await;
    assert_eq!(
        (t.notes.as_str(), t.notes_color),
        ("**Ask** first", v1::LabelColor::Green as i32)
    );
    client
        .update_thread(update(None, Some(v1::LabelColor::Unspecified)))
        .await
        .unwrap();
    assert_eq!(thread(&client, &id).await.notes_color, 0);
    client
        .update_thread(update(None, Some(v1::LabelColor::Teal)))
        .await
        .unwrap();

    // Kept across a restart.
    drop(client);
    let home_dir = daemon.home.clone();
    daemon.stop().await;
    let daemon = Daemon::start(&home_dir).await;
    let (client, _events) = daemon.connect().await;
    let t = thread(&client, &id).await;
    assert_eq!(t.notes_color, v1::LabelColor::Teal as i32, "{t:?}");

    // Clearing the notes clears their color, and a color without notes isn't kept.
    client.update_thread(update(Some(""), None)).await.unwrap();
    let t = thread(&client, &id).await;
    assert!(t.notes.is_empty() && t.notes_color == 0, "{t:?}");
    client
        .update_thread(update(None, Some(v1::LabelColor::Red)))
        .await
        .unwrap();
    assert_eq!(thread(&client, &id).await.notes_color, 0);
    daemon.stop().await;
}
