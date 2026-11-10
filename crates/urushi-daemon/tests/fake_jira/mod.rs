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

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

use serde_json::Value;
use urushi_core::board::{
    Issue, JiraColumn, Person, PullRequest, QuickFilter, StatusCategory, SyncedIssue,
};
use urushi_core::{TaskKind, Timestamp};
use urushi_daemon::jira::details::{
    Comment, Field, FieldKind, FieldOption, FieldValue, IssueDetails, IssueRef, JiraAttachment,
    Link, LinkType, TimeTracking, Worklog,
};
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
    /// What the editor shows beyond the board.
    pub extra: Extra,
    /// Bumped by every change: the issue's `updated`.
    pub version: u64,
}

/// An issue's fields beyond the board's, and its comments, files, links…
#[derive(Clone, Debug)]
pub struct Extra {
    /// `(account id, name)`.
    pub assignee: Option<(String, String)>,
    /// `(id, name)`.
    pub priority: (String, String),
    pub due: Option<String>,
    /// The "Team" option's id.
    pub team: Option<String>,
    /// `(id, author's account id, body)`.
    pub comments: Vec<(String, String, String)>,
    /// `(id, name, contents)`.
    pub attachments: Vec<(String, String, Vec<u8>)>,
    /// `(id, link type, other issue, this one is outward)`.
    pub links: Vec<(String, String, u32, bool)>,
    pub watching: bool,
    /// `(id, seconds, comment)`.
    pub worklogs: Vec<(String, u64, String)>,
}

impl Default for Extra {
    fn default() -> Self {
        Self {
            assignee: Some(("acc-ada".into(), "Ada".into())),
            priority: ("2".into(), "High".into()),
            due: None,
            team: None,
            comments: Vec::new(),
            attachments: Vec::new(),
            links: Vec::new(),
            watching: false,
            worklogs: Vec::new(),
        }
    }
}

/// The people on the site: `(account id, name)`.
pub const PEOPLE: [(&str, &str); 2] = [("acc-ada", "Ada"), ("acc-grace", "Grace")];
/// The priorities: `(id, name)`.
pub const PRIORITIES: [(&str, &str); 3] = [("1", "Highest"), ("2", "High"), ("3", "Low")];
/// The custom "Team" field: required when creating, its options `(id, name)`.
pub const TEAM: &str = "customfield_10100";
pub const TEAMS: [(&str, &str); 2] = [("t1", "Payments"), ("t2", "Mobile")];

/// A Jira with one kanban board, a small workflow and a call log.
pub struct FakeJira {
    pub issues: Mutex<Vec<Remote>>,
    calls: Mutex<Vec<String>>,
    /// While `true`, asking for an issue's transitions waits (see
    /// [`FakeJira::hold_transitions`]).
    transitions_held: (Mutex<bool>, Condvar),
    /// What development info answers for an issue, by its number (none: no pull
    /// requests).
    pub pull_requests: Mutex<HashMap<u32, JiraResult<Vec<PullRequest>>>>,
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
            extra: Extra::default(),
            version: 1,
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
            transitions_held: (Mutex::new(false), Condvar::new()),
            pull_requests: Mutex::default(),
        })
    }

    /// Makes Jira slow to answer which transitions an issue has, until
    /// [`FakeJira::release_transitions`]: lets a test see what the board shows while a
    /// move waits on Jira.
    pub fn hold_transitions(&self) {
        *self.transitions_held.0.lock().unwrap() = true;
    }

    /// Answers the transition lookups held by [`FakeJira::hold_transitions`].
    pub fn release_transitions(&self) {
        *self.transitions_held.0.lock().unwrap() = false;
        self.transitions_held.1.notify_all();
    }

    fn log(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    pub fn with(&self, id: u32, f: impl FnOnce(&mut Remote)) {
        let mut issues = self.issues.lock().unwrap();
        let remote = issues.iter_mut().find(|i| i.id == id).unwrap();
        f(remote);
        remote.version += 1;
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
                assignee: r.extra.assignee.as_ref().map(|a| a.1.clone()),
                assignee_id: r.extra.assignee.as_ref().map(|a| a.0.clone()),
                priority: Some(r.extra.priority.1.clone()),
                priority_id: Some(r.extra.priority.0.clone()),
                due: r.extra.due.clone(),
                links: u32::try_from(r.extra.links.len()).unwrap(),
                epic: epic.map(|e| key(e.id)),
                parent: r.parent.map(|p| format!("100{p}")),
                rank: r.rank.clone(),
                updated: Timestamp::from_unix_millis(r.version),
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
        // A sync reads in full only the issues whose time changed: `id in (…)`.
        let only: Option<Vec<String>> = query.extra_jql.iter().find_map(|q| {
            let ids = q.strip_prefix("id in (")?.strip_suffix(')')?;
            Some(ids.split(", ").map(str::to_owned).collect())
        });
        let remote = self.issues.lock().unwrap().clone();
        let mut issues: Vec<_> = remote
            .iter()
            .map(|r| self.synced(r))
            .filter(|i| only.as_ref().is_none_or(|ids| ids.contains(&i.issue.id)))
            .collect();
        match &only {
            Some(_) => self.log(format!(
                "issues of {}",
                issues
                    .iter()
                    .map(|i| i.issue.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            None => self.log(format!(
                "issues full={}",
                query.updated_within_minutes.is_none()
            )),
        }
        issues.sort_by(|a, b| a.issue.rank.cmp(&b.issue.rank));
        Ok(Fetched {
            issues,
            more: false,
        })
    }

    fn changed(
        &self,
        account: &Account,
        query: &IssueQuery,
    ) -> JiraResult<Vec<(String, Timestamp)>> {
        Self::check(account)?;
        self.log("changed".into());
        let remote = self.issues.lock().unwrap().clone();
        Ok(remote
            .iter()
            .map(|r| {
                (
                    format!("100{}", r.id),
                    Timestamp::from_unix_millis(r.version),
                )
            })
            .take(query.limit)
            .collect())
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
        let (held, released) = &self.transitions_held;
        drop(
            released
                .wait_while(held.lock().unwrap(), |held| *held)
                .unwrap(),
        );
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
        for (field, value) in &edit.fields {
            self.set_field(key, field, value)?;
        }
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
            extra: Extra::default(),
            version: 1,
        });
        drop(issues);
        for (field, value) in &issue.fields {
            self.set_field(&key(id), field, value)?;
        }
        self.log(format!("create {} {}", key(id), issue.kind.name()));
        Ok(key(id))
    }

    fn rank(&self, account: &Account, key: &str, rank: &Rank) -> JiraResult<()> {
        Self::check(account)?;
        self.log(format!("rank {key} {rank:?}"));
        Ok(())
    }

    fn details(&self, account: &Account, key: &str, _board: u64) -> JiraResult<IssueDetails> {
        Self::check(account)?;
        self.log(format!("details {key}"));
        let r = self.remote(key);
        let fields = fields_of(&r);
        let others = self.issues.lock().unwrap().clone();
        let issue_ref = |id: u32| {
            let o = others.iter().find(|i| i.id == id).unwrap();
            IssueRef {
                id: format!("100{id}"),
                key: self::key(id),
                summary: o.summary.clone(),
                status: status(o.status).0.into(),
                type_name: o.kind.name().into(),
            }
        };
        Ok(IssueDetails {
            fields,
            transitions: self.transitions(account, key)?,
            comments: r
                .extra
                .comments
                .iter()
                .map(|(id, author, body)| Comment {
                    id: id.clone(),
                    author: person(author),
                    body: body.clone(),
                    created: Timestamp::from_unix_millis(1),
                    updated: Timestamp::from_unix_millis(1),
                })
                .collect(),
            attachments: r
                .extra
                .attachments
                .iter()
                .map(|(id, name, data)| JiraAttachment {
                    id: id.clone(),
                    name: name.clone(),
                    media_type: "text/plain".into(),
                    size: data.len() as u64,
                    created: Timestamp::from_unix_millis(1),
                    author: Some(person("acc-ada")),
                    content_url: format!("https://acme.atlassian.net/attachment/{id}"),
                })
                .collect(),
            links: r
                .extra
                .links
                .iter()
                .map(|(id, kind, other, outward)| Link {
                    id: id.clone(),
                    relation: match (kind.as_str(), outward) {
                        ("Blocks", true) => "blocks".into(),
                        ("Blocks", false) => "is blocked by".into(),
                        _ => "relates to".into(),
                    },
                    issue: issue_ref(*other),
                })
                .collect(),
            link_types: vec![LinkType {
                name: "Blocks".into(),
                inward: "is blocked by".into(),
                outward: "blocks".into(),
            }],
            watchers: if r.extra.watching {
                vec![person("acc-ada")]
            } else {
                Vec::new()
            },
            watching: r.extra.watching,
            worklogs: r
                .extra
                .worklogs
                .iter()
                .map(|(id, seconds, comment)| Worklog {
                    id: id.clone(),
                    author: person("acc-ada"),
                    seconds: *seconds,
                    started: Timestamp::from_unix_millis(1),
                    comment: comment.clone(),
                })
                .collect(),
            time: TimeTracking::default(),
        })
    }

    fn create_fields(
        &self,
        account: &Account,
        _project: &str,
        _kind: TaskKind,
    ) -> JiraResult<Vec<Field>> {
        Self::check(account)?;
        Ok(vec![Field {
            id: TEAM.into(),
            name: "Team".into(),
            kind: FieldKind::Option,
            required: true,
            allowed: TEAMS
                .into_iter()
                .map(|(id, name)| FieldOption {
                    id: id.into(),
                    name: name.into(),
                })
                .collect(),
            value: FieldValue::default(),
        }])
    }

    fn users(&self, account: &Account, query: &str, _key: Option<&str>) -> JiraResult<Vec<Person>> {
        Self::check(account)?;
        Ok(PEOPLE
            .into_iter()
            .filter(|(_, name)| name.to_lowercase().contains(&query.to_lowercase()))
            .map(|(id, name)| Person {
                account_id: id.into(),
                name: name.into(),
            })
            .collect())
    }

    fn search(
        &self,
        account: &Account,
        _project: &str,
        query: &str,
        limit: usize,
    ) -> JiraResult<Vec<IssueRef>> {
        Self::check(account)?;
        let issues = self.issues.lock().unwrap().clone();
        Ok(issues
            .iter()
            .filter(|i| {
                i.summary.to_lowercase().contains(&query.to_lowercase()) || self::key(i.id) == query
            })
            .take(limit)
            .map(|i| IssueRef {
                id: format!("100{}", i.id),
                key: self::key(i.id),
                summary: i.summary.clone(),
                status: status(i.status).0.into(),
                type_name: i.kind.name().into(),
            })
            .collect())
    }

    fn add_comment(&self, account: &Account, key: &str, body: &str) -> JiraResult<()> {
        Self::check(account)?;
        self.log(format!("add_comment {key}"));
        let id = format!("c{}", self.calls().len());
        self.with(Self::id_of(key), |r| {
            r.extra
                .comments
                .push((id, account.account_id.clone(), body.into()));
        });
        Ok(())
    }

    fn update_comment(&self, account: &Account, key: &str, id: &str, body: &str) -> JiraResult<()> {
        Self::check(account)?;
        let mut found = false;
        self.with(Self::id_of(key), |r| {
            for c in r.extra.comments.iter_mut().filter(|c| c.0 == id) {
                c.2 = body.into();
                found = true;
            }
        });
        if found {
            Ok(())
        } else {
            Err(JiraError::NotFound(format!("comment {id}")))
        }
    }

    fn delete_comment(&self, account: &Account, key: &str, id: &str) -> JiraResult<()> {
        Self::check(account)?;
        self.with(Self::id_of(key), |r| r.extra.comments.retain(|c| c.0 != id));
        Ok(())
    }

    fn attach(&self, account: &Account, key: &str, name: &str, data: &[u8]) -> JiraResult<()> {
        Self::check(account)?;
        let id = format!("a{}", self.calls().len());
        self.log(format!("attach {key} {name}"));
        self.with(Self::id_of(key), |r| {
            r.extra.attachments.push((id, name.into(), data.to_vec()));
        });
        Ok(())
    }

    fn delete_attachment(&self, account: &Account, id: &str) -> JiraResult<()> {
        Self::check(account)?;
        let mut issues = self.issues.lock().unwrap();
        for r in issues.iter_mut() {
            r.extra.attachments.retain(|a| a.0 != id);
        }
        Ok(())
    }

    fn download(&self, account: &Account, url: &str) -> JiraResult<Vec<u8>> {
        Self::check(account)?;
        let id = url.rsplit('/').next().unwrap_or_default();
        let issues = self.issues.lock().unwrap();
        issues
            .iter()
            .flat_map(|r| &r.extra.attachments)
            .find(|a| a.0 == id)
            .map(|a| a.2.clone())
            .ok_or_else(|| JiraError::NotFound(format!("attachment {id}")))
    }

    fn link(&self, account: &Account, kind: &str, outward: &str, inward: &str) -> JiraResult<()> {
        Self::check(account)?;
        let (out_id, in_id) = (Self::id_of(outward), Self::id_of(inward));
        let id = format!("l{}", self.calls().len());
        self.log(format!("link {outward} {kind} {inward}"));
        self.with(out_id, |r| {
            r.extra.links.push((id.clone(), kind.into(), in_id, true));
        });
        self.with(in_id, |r| {
            r.extra.links.push((id, kind.into(), out_id, false));
        });
        Ok(())
    }

    fn unlink(&self, account: &Account, id: &str) -> JiraResult<()> {
        Self::check(account)?;
        let mut issues = self.issues.lock().unwrap();
        for r in issues.iter_mut() {
            r.extra.links.retain(|l| l.0 != id);
        }
        Ok(())
    }

    fn watch(&self, account: &Account, key: &str, watch: bool) -> JiraResult<()> {
        Self::check(account)?;
        self.with(Self::id_of(key), |r| r.extra.watching = watch);
        Ok(())
    }

    fn log_work(
        &self,
        account: &Account,
        key: &str,
        seconds: u64,
        started: &str,
        comment: &str,
    ) -> JiraResult<()> {
        Self::check(account)?;
        self.log(format!("log_work {key} {seconds} {started}"));
        let id = format!("w{}", self.calls().len());
        self.with(Self::id_of(key), |r| {
            r.extra.worklogs.push((id, seconds, comment.into()));
        });
        Ok(())
    }

    fn pull_requests(&self, account: &Account, issue_id: &str) -> JiraResult<Vec<PullRequest>> {
        Self::check(account)?;
        let n: u32 = issue_id.trim_start_matches("100").parse().unwrap();
        self.log(format!("pull_requests {}", key(n)));
        self.pull_requests
            .lock()
            .unwrap()
            .get(&n)
            .cloned()
            .unwrap_or(Ok(Vec::new()))
    }
}

impl FakeJira {
    /// Sets a field as Jira's edit API would, refusing what it doesn't know.
    fn set_field(&self, key: &str, field: &str, value: &Value) -> JiraResult<()> {
        let id = Self::id_of(key);
        match field {
            "assignee" => {
                let person = value["accountId"].as_str().map(|a| {
                    let name = PEOPLE.iter().find(|p| p.0 == a).map_or("?", |p| p.1);
                    (a.to_owned(), name.to_owned())
                });
                self.with(id, |r| r.extra.assignee = person);
            }
            "priority" => {
                let p = PRIORITIES
                    .iter()
                    .find(|p| value["id"].as_str() == Some(p.0))
                    .ok_or_else(|| JiraError::Rejected("priority: not allowed".into()))?;
                self.with(id, |r| r.extra.priority = (p.0.into(), p.1.into()));
            }
            "duedate" => self.with(id, |r| r.extra.due = value.as_str().map(str::to_owned)),
            TEAM => self.with(id, |r| {
                r.extra.team = value["id"].as_str().map(str::to_owned);
            }),
            other => return Err(JiraError::Rejected(format!("{other}: not on the screen"))),
        }
        self.log(format!("set {key} {field}"));
        Ok(())
    }
}

/// The fields the editor shows of `r`: assignee, priority and due date.
fn fields_of(r: &Remote) -> Vec<Field> {
    let option = |(id, name): (&str, &str)| FieldOption {
        id: id.into(),
        name: name.into(),
    };
    let value = |options: Vec<FieldOption>| FieldValue {
        text: String::new(),
        options,
    };

    vec![
        Field {
            id: "assignee".into(),
            name: "Assignee".into(),
            kind: FieldKind::User,
            required: false,
            allowed: Vec::new(),
            value: value(
                r.extra
                    .assignee
                    .iter()
                    .map(|(id, name)| option((id, name)))
                    .collect(),
            ),
        },
        Field {
            id: "priority".into(),
            name: "Priority".into(),
            kind: FieldKind::Option,
            required: false,
            allowed: PRIORITIES.into_iter().map(option).collect(),
            value: value(vec![option((&r.extra.priority.0, &r.extra.priority.1))]),
        },
        Field {
            id: "duedate".into(),
            name: "Due date".into(),
            kind: FieldKind::Date,
            required: false,
            allowed: Vec::new(),
            value: FieldValue {
                text: r.extra.due.clone().unwrap_or_default(),
                options: Vec::new(),
            },
        },
    ]
}

/// A person on the site, by account id.
fn person(id: &str) -> Person {
    Person {
        account_id: id.into(),
        name: PEOPLE
            .iter()
            .find(|p| p.0 == id)
            .map_or("?", |p| p.1)
            .into(),
    }
}
