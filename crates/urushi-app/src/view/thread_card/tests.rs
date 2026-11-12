//! The card's links, unread mark, notes and project stripe.

use std::collections::HashMap;

use urushi_client::v1;
use urushi_ui::{CellMetrics, Point, Rect, Role, TextSystem, Theme, build};

use crate::config::ConnectTarget;
use crate::msg::{Effect, Msg};
use crate::state::AppState;

const ISSUE: &str = "https://acme.atlassian.net/browse/ACME-1";
const PR: &str = "https://github.com/acme/app/pull/7";
const LOCAL_PR: &str = "https://github.com/acme/app/pull/9";

/// Threads `jira` (started from Jira issue ACME-1, which has an open pull request),
/// `local` (from a local task with a pull request link) and `plain` (no task).
fn state() -> AppState {
    let mut state = AppState::new(
        ConnectTarget::Local(None),
        CellMetrics {
            width: 8.0,
            height: 16.0,
        },
    );
    let thread = |id: &str| v1::Thread {
        id: id.into(),
        project_id: "r".into(),
        title: format!("Thread {id}"),
        primary_branch: format!("urushi/{id}"),
        state: v1::ThreadState::Ready.into(),
        ..Default::default()
    };
    let issue = v1::JiraIssue {
        key: "ACME-1".into(),
        url: ISSUE.into(),
        pull_requests: vec![v1::PullRequest {
            url: PR.into(),
            title: "ACME-1 fix it".into(),
            status: v1::PullRequestStatus::Open.into(),
        }],
        ..Default::default()
    };
    state.update(Msg::Connected {
        welcome: v1::Welcome::default(),
        workspace: v1::Workspace {
            projects: vec![v1::Project {
                id: "r".into(),
                name: "api".into(),
                ..Default::default()
            }],
            threads: vec![thread("jira"), thread("local"), thread("plain")],
            boards: vec![v1::Board {
                id: "b".into(),
                columns: vec![v1::BoardColumn {
                    id: "c".into(),
                    tasks: vec![
                        v1::Task {
                            id: "t1".into(),
                            thread_id: "jira".into(),
                            issue: Some(Box::new(issue)),
                            ..Default::default()
                        },
                        v1::Task {
                            id: "t2".into(),
                            thread_id: "local".into(),
                            pr_url: LOCAL_PR.into(),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    });
    state
}

fn render(state: &AppState) -> urushi_ui::Frame<Msg> {
    let mut text = TextSystem::bundled_only();
    build(
        &crate::view::view(state, &Theme::dark(), &HashMap::new()),
        Rect::new(0.0, 0.0, 1200.0, 800.0),
        &mut text,
        None::<Point>,
    )
}

fn labels(frame: &urushi_ui::Frame<Msg>) -> Vec<&str> {
    frame
        .access
        .iter()
        .filter(|a| {
            a.role == Role::Button
                && (a.label.starts_with("Open pull request") || a.label.ends_with("in Jira"))
        })
        .map(|a| a.label.as_str())
        .collect()
}

#[test]
fn cards_show_the_marks_of_their_threads_issue_and_pull_request() {
    let frame = render(&state());
    assert_eq!(
        labels(&frame),
        [
            "Open ACME-1 in Jira",
            "Open pull request #7: ACME-1 fix it",
            "Open pull request #9",
        ]
    );
    assert!(
        frame.scene.dump().matches("image").count() >= 3,
        "{}",
        frame.scene.dump()
    );
}

#[test]
fn clicking_a_mark_opens_its_page_and_not_the_thread() {
    let mut state = state();
    let frame = render(&state);
    let mark = frame
        .hits
        .iter()
        .find(|h| matches!(&h.on_click, Some(Msg::OpenUrl(u)) if u == ISSUE))
        .expect("the Jira mark");
    let center = Point::new(
        mark.rect.x + mark.rect.width / 2.0,
        mark.rect.y + mark.rect.height / 2.0,
    );
    let msg = frame.click(center).expect("a click");
    assert!(matches!(&msg, Msg::OpenUrl(u) if u == ISSUE), "{msg:?}");
    let selected = state.selected_thread.clone();
    let fx = state.update(msg);
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::OpenUrl(u) if u == ISSUE))
    );
    assert_eq!(state.selected_thread, selected, "the thread isn't opened");
    assert!(
        frame
            .hits
            .iter()
            .any(|h| matches!(&h.on_click, Some(Msg::OpenUrl(u)) if u == LOCAL_PR))
    );
}
