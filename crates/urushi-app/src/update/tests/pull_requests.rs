//! A local task's pull request link (ADR 0036): edited in the task editor, sent with
//! Save only when it changed, and not offered on Jira boards (whose pull requests come
//! from Jira) or by older daemons.

use urushi_client::v1::{self, request::Body};
use urushi_ui::TextField;

use super::{attached, bodies};
use crate::board::PR_LINKS_SINCE_MINOR;
use crate::msg::Msg;
use crate::state::{AppState, Dialog, Field};

const URL: &str = "https://github.com/acme/app/pull/7";

/// A daemon at `minor` with a local board (tasks "a", "b"; "b" links [`URL`]) and a
/// Jira board, and the panel open.
fn with_boards(minor: u32) -> AppState {
    let mut s = attached();
    s.daemon_minor = minor;
    let mut workspace = super::workspace();
    let mut local = super::board::board();
    local.columns[0].tasks[1].pr_url = URL.into();
    workspace.boards = vec![local, super::jira::jira_board()];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s
}

fn open_task(s: &mut AppState, id: &str) {
    s.update(Msg::PressTask(id.into()));
    s.update(Msg::PressEnded);
}

fn set_pr_field(s: &mut AppState, text: &str) {
    if let Some(Dialog::Task { labels, .. }) = &mut s.dialog {
        labels.pr_url = TextField::new(text.to_owned());
    }
}

/// The pull request link a Save sent, if it sent one.
fn sent_pr_url(fx: &[crate::msg::Effect]) -> Option<String> {
    bodies(fx).into_iter().find_map(|b| match b {
        Body::UpdateTask(u) => u.pr_url.clone(),
        _ => None,
    })
}

#[test]
fn a_local_task_links_a_pull_request_on_save_only_when_it_changed() {
    let mut s = with_boards(PR_LINKS_SINCE_MINOR);
    open_task(&mut s, "a");
    assert!(s.shows_pr_field());
    assert!(s.dialog_fields().contains(&Field::TaskPrUrl));
    // Typed into its field, as the user would.
    s.update(Msg::FocusField(Field::TaskPrUrl));
    s.update(Msg::Paste(format!("  {URL} ")));
    let fx = s.update(Msg::Submit);
    assert!(
        matches!(
            bodies(&fx)[..],
            [Body::UpdateTask(v1::UpdateTaskRequest { task_id, title: None, notes: None, color: None, tags: None, kind: None, pr_url: Some(url) })]
                if task_id == "a" && url == URL
        ),
        "{fx:?}"
    );

    // (The editor waits for the daemon's answer; close it as that would.)
    s.dialog = None;

    // "b" already links it: the editor shows it, and saving unchanged sends nothing.
    open_task(&mut s, "b");
    assert!(matches!(&s.dialog, Some(Dialog::Task { labels, .. }) if labels.pr_url.text() == URL));
    let fx = s.update(Msg::Submit);
    assert!(bodies(&fx).is_empty(), "{fx:?}");
    // Emptying the field unlinks it.
    open_task(&mut s, "b");
    set_pr_field(&mut s, "");
    assert_eq!(sent_pr_url(&s.update(Msg::Submit)), Some(String::new()));
}

#[test]
fn a_changed_link_counts_as_an_unsaved_edit() {
    let mut s = with_boards(PR_LINKS_SINCE_MINOR);
    open_task(&mut s, "a");
    set_pr_field(&mut s, URL);
    assert!(s.editor_dirty());
}

#[test]
fn jira_issues_new_tasks_and_older_daemons_have_no_link_field() {
    let mut s = with_boards(PR_LINKS_SINCE_MINOR);
    open_task(&mut s, "j1");
    assert!(
        !s.shows_pr_field(),
        "Jira says which pull requests an issue has"
    );
    assert!(!s.dialog_fields().contains(&Field::TaskPrUrl));
    set_pr_field(&mut s, URL);
    assert_eq!(sent_pr_url(&s.update(Msg::Submit)), None);

    let mut old = with_boards(PR_LINKS_SINCE_MINOR - 1);
    open_task(&mut old, "a");
    assert!(!old.shows_pr_field());
    set_pr_field(&mut old, URL);
    assert_eq!(sent_pr_url(&old.update(Msg::Submit)), None);
}
