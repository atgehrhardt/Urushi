//! What a thread links to outside Urushi (ADR 0036): the Jira issue of a task started
//! in it, and that task's pull request. Clients show both on the thread's card and
//! open them in a browser. Several tasks can be linked to one thread (ADR 0018); the
//! first one that has each link, parents first, gives it.

use std::collections::HashMap;

use urushi_core::board::PrUrl;
use urushi_proto::v1;

use crate::{TaskRef, WorkspaceMirror};

/// A thread's Jira issue and pull request, when it has them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadLinks {
    /// The Jira issue of a task linked to the thread.
    pub issue: Option<IssueLink>,
    /// The pull request of a task linked to the thread.
    pub pull_request: Option<PrLink>,
}

/// A Jira issue to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueLink {
    /// Its key, e.g. `UR-24`.
    pub key: String,
    /// Its page on the Jira site.
    pub url: String,
}

/// A pull request to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrLink {
    /// How to name it: `#12`, or its URL when that ends in no number.
    pub label: String,
    /// Its title, when Jira reported one.
    pub title: String,
    /// Its address (always `https://`).
    pub url: String,
}

impl ThreadLinks {
    /// Whether there is nothing to show.
    pub fn is_empty(&self) -> bool {
        self.issue.is_none() && self.pull_request.is_none()
    }
}

/// The pull request a task names: a local task's own link, else the best of its Jira
/// issue's (the first open one).
fn pull_request(task: TaskRef<'_>) -> Option<PrLink> {
    if let Ok(url) = PrUrl::parse(task.pr_url()) {
        return Some(PrLink {
            label: url.label(),
            title: String::new(),
            url: url.as_str().to_owned(),
        });
    }
    let prs = &task.issue()?.pull_requests;
    let open = v1::PullRequestStatus::Open as i32;
    prs.iter()
        .filter(|p| PrUrl::parse(&p.url).is_ok())
        .find(|p| p.status == open)
        .or_else(|| prs.iter().find(|p| PrUrl::parse(&p.url).is_ok()))
        .and_then(|p| {
            let url = PrUrl::parse(&p.url).ok()?;
            Some(PrLink {
                label: url.label(),
                title: p.title.clone(),
                url: p.url.clone(),
            })
        })
}

impl WorkspaceMirror {
    /// Every linked thread's links, by thread id, read in one pass over the boards.
    pub fn thread_links(&self) -> HashMap<String, ThreadLinks> {
        let mut links: HashMap<String, ThreadLinks> = HashMap::new();
        for task in self.all_tasks() {
            if task.thread_id().is_empty() {
                continue;
            }
            let entry = links.entry(task.thread_id().to_owned()).or_default();
            if entry.issue.is_none() {
                entry.issue = task
                    .issue()
                    .filter(|i| !i.key.is_empty() && !i.url.is_empty())
                    .map(|i| IssueLink {
                        key: i.key.clone(),
                        url: i.url.clone(),
                    });
            }
            if entry.pull_request.is_none() {
                entry.pull_request = pull_request(task);
            }
        }
        links.retain(|_, l| !l.is_empty());
        links
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(n: u32, status: v1::PullRequestStatus) -> v1::PullRequest {
        v1::PullRequest {
            url: format!("https://github.com/o/r/pull/{n}"),
            title: format!("PR {n}"),
            status: status as i32,
        }
    }

    fn mirror(tasks: Vec<v1::Task>) -> WorkspaceMirror {
        WorkspaceMirror::new(v1::Workspace {
            boards: vec![v1::Board {
                id: "b".into(),
                columns: vec![v1::BoardColumn {
                    id: "c".into(),
                    tasks,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    #[test]
    fn a_thread_shows_its_tasks_issue_and_pull_request() {
        let issue = v1::JiraIssue {
            key: "UR-3".into(),
            url: "https://acme.atlassian.net/browse/UR-3".into(),
            pull_requests: vec![
                pr(1, v1::PullRequestStatus::Merged),
                pr(2, v1::PullRequestStatus::Open),
            ],
            ..Default::default()
        };
        let m = mirror(vec![
            v1::Task {
                id: "1".into(),
                thread_id: "jira".into(),
                issue: Some(Box::new(issue)),
                ..Default::default()
            },
            v1::Task {
                id: "2".into(),
                thread_id: "local".into(),
                subtasks: vec![v1::Subtask {
                    id: "3".into(),
                    thread_id: "sub".into(),
                    pr_url: "https://gitlab.com/g/p/-/merge_requests/9".into(),
                    ..Default::default()
                }],
                pr_url: "not a url".into(),
                ..Default::default()
            },
            v1::Task {
                id: "4".into(),
                ..Default::default()
            },
        ]);
        let links = m.thread_links();
        let jira = &links["jira"];
        assert_eq!(jira.issue.as_ref().unwrap().key, "UR-3");
        let open = jira.pull_request.as_ref().unwrap();
        assert_eq!((open.label.as_str(), open.title.as_str()), ("#2", "PR 2"));
        let sub = links["sub"].pull_request.as_ref().unwrap();
        assert_eq!(sub.label, "#9");
        assert!(links["sub"].issue.is_none());
        assert!(
            !links.contains_key("local"),
            "an invalid link and no issue show nothing"
        );
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn the_first_linked_task_with_a_link_gives_it() {
        let m = mirror(vec![
            v1::Task {
                id: "1".into(),
                thread_id: "t".into(),
                ..Default::default()
            },
            v1::Task {
                id: "2".into(),
                thread_id: "t".into(),
                pr_url: "https://github.com/o/r/pull/5".into(),
                ..Default::default()
            },
            v1::Task {
                id: "3".into(),
                thread_id: "t".into(),
                pr_url: "https://github.com/o/r/pull/6".into(),
                ..Default::default()
            },
        ]);
        let links = m.thread_links();
        assert_eq!(links["t"].pull_request.as_ref().unwrap().label, "#5");
    }
}
