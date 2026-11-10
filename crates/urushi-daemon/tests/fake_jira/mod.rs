//! An in-memory Jira for the end-to-end tests: one kanban board (id 7) with a small
//! workflow (To Do ⇄ In Progress ⇄ Done), a quick filter, and a log of the calls it
//! got.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers"
)]

use std::sync::{Arc, Mutex};

use urushi_core::TaskKind;
use urushi_core::board::{Issue, JiraColumn, QuickFilter, StatusCategory, SyncedIssue};
use urushi_daemon::jira::{
    Account, BoardConfig, BoardInfo, Fetched, IssueEdit, IssueFields, IssueQuery, JiraApi,
    JiraError, JiraResult, Me, NewIssue, Rank, Transition,
};

pub const TOKEN: &str = "s3cret-t0ken";
pub const TODO: &str = "1";
pub const DOING: &str = "3";
pub const DONE: &str = "10001";

#[derive(Clone, Debug)]
pub struct Remote {
    pub id: u32,
    pub summary: String,
    pub notes: String,
    pub status: &'static str,
    pub kind: TaskKind,
    pub parent: Option<u32>,
    pub epic: Option<u32>,
    pub labels: Vec<String>,
    pub rank: String,
}

/// A Jira with one kanban board, a small workflow and a call log.
pub struct FakeJira {
    pub issues: Mutex<Vec<Remote>>,
    calls: Mutex<Vec<String>>,
}

/// The JQL of the board's one quick filter.
pub const BUGS: &str = "type = Bug";

pub fn key(id: u32) -> String {
    format!("ACME-{id}")
}

fn status(id: &str) -> (&'static str, StatusCategory) {
    match id {
        TODO => ("To Do", StatusCategory::ToDo),
        DOING => ("In Progress", StatusCategory::InProgress),
        _ => ("Done", StatusCategory::Done),
    }
}

impl FakeJira {
    pub fn new() -> Arc<Self> {
        let issue = |id, summary: &str, status, kind, parent, epic| Remote {
            id,
            summary: summary.into(),
            notes: String::new(),
            status,
            kind,
            parent,
            epic,
            labels: Vec::new(),
            rank: format!("0|{id:04}"),
        };
        Arc::new(Self {
            issues: Mutex::new(vec![
                issue(9, "Billing", TODO, TaskKind::Epic, None, None),
                issue(1, "Checkout", TODO, TaskKind::Story, None, Some(9)),
                issue(2, "Invoices", DOING, TaskKind::Bug, None, None),
                issue(3, "Write tests", TODO, TaskKind::Subtask, Some(2), None),
                issue(4, "Old idea", TODO, TaskKind::Task, None, None),
            ]),
            calls: Mutex::default(),
        })
    }

    fn log(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    pub fn with(&self, id: u32, f: impl FnOnce(&mut Remote)) {
        let mut issues = self.issues.lock().unwrap();
        f(issues.iter_mut().find(|i| i.id == id).unwrap());
    }

    pub fn remote(&self, key: &str) -> Remote {
        let issues = self.issues.lock().unwrap();
        issues
            .iter()
            .find(|i| self::key(i.id) == key)
            .unwrap()
            .clone()
    }

    fn check(account: &Account) -> JiraResult<()> {
        if account.token == TOKEN {
            Ok(())
        } else {
            Err(JiraError::Unauthorized)
        }
    }

    fn synced(&self, r: &Remote) -> SyncedIssue {
        let issues = self.issues.lock().unwrap();
        let (name, category) = status(r.status);
        let epic = r.epic.and_then(|e| issues.iter().find(|i| i.id == e));
        SyncedIssue {
            issue: Issue {
                id: format!("100{}", r.id),
                key: key(r.id),
                type_name: r.kind.name().into(),
                status_id: r.status.into(),
                status: name.into(),
                category,
                assignee: Some("Ada".into()),
                assignee_id: Some("acc-ada".into()),
                priority: Some("High".into()),
                priority_id: Some("2".into()),
                epic: epic.map(|e| key(e.id)),
                parent: r.parent.map(|p| format!("100{p}")),
                rank: r.rank.clone(),
                ..Issue::default()
            },
            title: r.summary.clone(),
            notes: r.notes.clone(),
            labels: r.labels.clone(),
            kind: r.kind,
            epic_summary: epic.map(|e| e.summary.clone()),
            created: urushi_core::Timestamp::from_unix_millis(1),
        }
    }

    fn id_of(key: &str) -> u32 {
        key.trim_start_matches("ACME-").parse().unwrap()
    }

    /// The workflow: To Do ⇄ In Progress ⇄ Done, never To Do → Done.
    fn allowed(from: &str) -> &'static [&'static str] {
        if from == DOING {
            &[TODO, DONE]
        } else {
            &[DOING]
        }
    }
}

impl JiraApi for FakeJira {
    fn myself(&self, account: &Account) -> JiraResult<Me> {
        Self::check(account)?;
        Ok(Me {
            display_name: "Ada Lovelace".into(),
            account_id: "acc-ada".into(),
        })
    }

    fn boards(&self, account: &Account, _query: &str) -> JiraResult<Vec<BoardInfo>> {
        Self::check(account)?;
        Ok(vec![self.board(account, 7)?.info])
    }

    fn board(&self, account: &Account, id: u64) -> JiraResult<BoardConfig> {
        Self::check(account)?;
        if id != 7 {
            return Err(JiraError::NotFound(format!("board {id}")));
        }
        let column = |name: &str, s: &str| JiraColumn {
            name: name.into(),
            statuses: vec![s.into()],
        };
        Ok(BoardConfig {
            info: BoardInfo {
                id: 7,
                name: "ACME board".into(),
                scrum: false,
                project_key: "ACME".into(),
                project_name: "Acme".into(),
            },
            columns: vec![
                column("To Do", TODO),
                column("In Progress", DOING),
                column("Done", DONE),
            ],
            rank_field: Some("customfield_10019".into()),
            points_field: Some("customfield_10016".into()),
            sprint_field: Some("customfield_10020".into()),
            quick_filters: vec![QuickFilter {
                id: 1,
                name: "Bugs".into(),
                jql: BUGS.into(),
            }],
            filter_id: 10003,
            sub_query: None,
        })
    }

    fn issues(&self, account: &Account, query: &IssueQuery) -> JiraResult<Fetched> {
        Self::check(account)?;
        self.log(format!(
            "issues full={}",
            query.updated_within_minutes.is_none()
        ));
        let remote = self.issues.lock().unwrap().clone();
        let mut issues: Vec<_> = remote.iter().map(|r| self.synced(r)).collect();
        issues.sort_by(|a, b| a.issue.rank.cmp(&b.issue.rank));
        Ok(Fetched {
            issues,
            more: false,
        })
    }

    fn issue_ids(&self, account: &Account, query: &IssueQuery) -> JiraResult<Vec<String>> {
        Self::check(account)?;
        self.log(format!("issue_ids {:?}", query.extra_jql));
        let remote = self.issues.lock().unwrap().clone();
        Ok(remote
            .iter()
            .filter(|r| {
                query
                    .extra_jql
                    .iter()
                    .all(|q| q == BUGS && r.kind == TaskKind::Bug)
            })
            .map(|r| self.synced(r).issue.id)
            .collect())
    }

    fn issue(
        &self,
        account: &Account,
        key: &str,
        _fields: &IssueFields,
    ) -> JiraResult<SyncedIssue> {
        Self::check(account)?;
        Ok(self.synced(&self.remote(key)))
    }

    fn transitions(&self, account: &Account, key: &str) -> JiraResult<Vec<Transition>> {
        Self::check(account)?;
        Ok(Self::allowed(self.remote(key).status)
            .iter()
            .map(|to| Transition {
                id: format!("t{to}"),
                name: status(to).0.into(),
                to_status: (*to).into(),
                to_category: status(to).1,
            })
            .collect())
    }

    fn transition(&self, account: &Account, key: &str, transition: &str) -> JiraResult<()> {
        Self::check(account)?;
        self.log(format!("transition {key} {transition}"));
        let to = transition.trim_start_matches('t');
        let to = [TODO, DOING, DONE].into_iter().find(|s| *s == to).unwrap();
        assert!(
            Self::allowed(self.remote(key).status).contains(&to),
            "the workflow allows it"
        );
        self.with(Self::id_of(key), |r| r.status = to);
        Ok(())
    }

    fn edit(&self, account: &Account, key: &str, edit: &IssueEdit) -> JiraResult<()> {
        Self::check(account)?;
        self.log(format!("edit {key}"));
        self.with(Self::id_of(key), |r| {
            if let Some(s) = &edit.summary {
                r.summary.clone_from(s);
            }
            if let Some(n) = &edit.notes {
                r.notes.clone_from(n);
            }
            r.labels.retain(|l| !edit.labels_remove.contains(l));
            r.labels.extend(edit.labels_add.iter().cloned());
        });
        Ok(())
    }

    fn create(&self, account: &Account, issue: &NewIssue) -> JiraResult<String> {
        Self::check(account)?;
        let mut issues = self.issues.lock().unwrap();
        let id = issues.iter().map(|i| i.id).max().unwrap() + 1;
        issues.push(Remote {
            id,
            summary: issue.summary.clone(),
            notes: issue.notes.clone(),
            status: TODO,
            kind: issue.kind,
            parent: issue.parent_key.as_deref().map(Self::id_of),
            epic: None,
            labels: issue.labels.clone(),
            rank: format!("0|{id:04}"),
        });
        drop(issues);
        self.log(format!("create {} {}", key(id), issue.kind.name()));
        Ok(key(id))
    }

    fn rank(&self, account: &Account, key: &str, rank: &Rank) -> JiraResult<()> {
        Self::check(account)?;
        self.log(format!("rank {key} {rank:?}"));
        Ok(())
    }
}
