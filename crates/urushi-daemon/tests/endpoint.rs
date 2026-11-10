//! Clients find the local daemon from its home as they spell it. The daemon canonicalizes
//! its home for everything else, but must listen where a client would look: on Windows
//! the pipe's name is derived from the path's text, so a canonical spelling (short names
//! expanded, `..` removed, no `\\?\` prefix) would be a different pipe.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

mod common;

use common::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_listens_where_its_home_as_given_points() {
    let temp = temp_home();
    std::fs::create_dir_all(temp.path().join("sub")).unwrap();
    let home = temp.path().join("sub").join("..").join("home");
    // `start` connects through `local_endpoint(home)`, as a client would.
    let daemon = Daemon::start(&home).await;
    let (client, _events) = daemon.connect().await;
    client.list_workspace(false).await.unwrap();
    daemon.stop().await;
}
