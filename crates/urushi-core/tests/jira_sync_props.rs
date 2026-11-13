//! Property tests: merging any sequence of Jira snapshots keeps a board consistent.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]

use std::collections::{HashMap, HashSet};

use proptest::prelude::*;
use urushi_core::board::{
    Issue, JiraBoard, JiraColumn, MAX_SYNCED_ISSUES, StatusCategory, SyncState, SyncedIssue,
};
use urushi_core::{Board, BoardId, ColumnId, TaskId, TaskKind, ThreadId, Timestamp};

const STATUSES: [&str; 4] = ["1", "2", "3", "unmapped"];

#[derive(Clone, Debug)]
struct Shot {
    n: u32,
    status: usize,
    parent: Option<u32>,
    epic: bool,
}

fn shot() -> impl Strategy<Value = Shot> {
    (
        0..30u32,
        0..4usize,
        prop::option::weighted(0.3, 0..30u32),
        prop::bool::weighted(0.1),
    )
        .prop_map(|(n, status, parent, epic)| Shot {
            n,
            status,
            // Jira epics have no parent (a story's epic is `Issue.epic`, not `parent`).
            parent: parent.filter(|p| *p != n && !epic),
            epic,
        })
}

fn synced(s: &Shot) -> SyncedIssue {
    SyncedIssue {
        issue: Issue {
            id: s.n.to_string(),
            key: format!("P-{}", s.n),
            status_id: STATUSES[s.status].into(),
            category: if s.status == 2 {
                StatusCategory::Done
            } else {
                StatusCategory::ToDo
            },
            rank: format!("0|{:04}", (s.n * 7) % 31),
            parent: s.parent.map(|p| p.to_string()),
            ..Issue::default()
        },
        title: format!("t{}", s.n),
        notes: String::new(),
        labels: Vec::new(),
        kind: if s.epic {
            TaskKind::Epic
        } else if s.parent.is_some() {
            TaskKind::Subtask
        } else {
            TaskKind::Task
        },
        epic_summary: None,
        created: Timestamp::from_unix_millis(0),
    }
}

fn board() -> Board {
    let mut next = 0;
    Board::new_jira(
        BoardId::from_u128(1),
        JiraBoard {
            site: "https://x.atlassian.net".into(),
            account: String::new(),
            board_id: 1,
            name: "b".into(),
            project_key: "P".into(),
            scrum: false,
            rank_field: None,
            points_field: None,
            sprint_field: None,
            quick_filters: Vec::new(),
            sync: SyncState::default(),
        },
        &STATUSES[..3]
            .iter()
            .map(|s| JiraColumn {
                name: format!("c{s}"),
                statuses: vec![(*s).to_owned()],
            })
            .collect::<Vec<_>>(),
        Timestamp::from_unix_millis(0),
        || {
            next += 1;
            ColumnId::from_u128(next)
        },
    )
    .unwrap()
}

/// Issue id → task id of every task on the board; fails on a repeated issue.
fn issues(board: &Board) -> HashMap<String, TaskId> {
    let mut out = HashMap::new();
    for t in board.all_tasks() {
        let id = t
            .issue
            .as_ref()
            .expect("only issues on a Jira board")
            .id
            .clone();
        assert!(out.insert(id, t.id).is_none(), "an issue shows twice");
    }
    out
}

proptest! {
    #[test]
    fn merging_never_duplicates_and_keeps_tasks_and_links(
        merges in prop::collection::vec((prop::collection::vec(shot(), 0..25), any::<bool>()), 1..8)
    ) {
        let mut b = board();
        let mut next = 1000u128;
        let mut links: HashMap<String, ThreadId> = HashMap::new();
        for (shots, full) in merges {
            // Jira reports each issue once.
            let mut seen = HashSet::new();
            let snapshot: Vec<_> = shots.iter().filter(|s| seen.insert(s.n)).map(synced).collect();
            let before = issues(&b);
            // Issues kept while Jira's search lags (UR-71): some on the board, left out.
            let keep: HashSet<String> = before
                .keys()
                .filter(|id| id.parse::<u32>().is_ok_and(|n| n % 3 == 0))
                .filter(|id| !snapshot.iter().any(|s| &s.issue.id == *id))
                .cloned()
                .collect();
            let kept_before: HashMap<String, Option<urushi_core::board::Task>> = keep
                .iter()
                .map(|id| (id.clone(), before.get(id).and_then(|t| b.task(*t).cloned())))
                .collect();
            b.merge_issues_keeping(snapshot.clone(), full, &keep, || { next += 1; TaskId::from_u128(next) });
            let after = issues(&b);
            // A kept card is still there, unchanged. (A kept subtask follows its parent,
            // which may leave the board.)
            for (id, task) in &kept_before {
                let now = after.get(id);
                let card = task.as_ref().is_some_and(|t| t.issue.as_ref().is_some_and(|i| i.parent.is_none()));
                prop_assert!(now.is_some() || !card, "kept {} went", id);
                if let (Some(now), Some(task)) = (now, task) {
                    prop_assert_eq!(*now, task.id);
                    prop_assert_eq!(&b.task(*now).unwrap().issue, &task.issue);
                }
            }

            prop_assert!(b.all_tasks().count() <= MAX_SYNCED_ISSUES);
            for (issue, task) in &after {
                // An issue that stayed keeps its task.
                if let Some(old) = before.get(issue) {
                    prop_assert_eq!(old, task);
                }
                if full && !keep.contains(issue) {
                    prop_assert!(seen.contains(&issue.parse::<u32>().unwrap()));
                }
                // So does its link.
                if let Some(thread) = links.get(issue) {
                    prop_assert_eq!(b.task(*task).unwrap().thread, Some(*thread));
                }
            }
            // Epics are cards (ADR 0027), and named for their stories' chips.
            let cards: HashSet<TaskId> = b.columns.iter().flat_map(|c| &c.tasks).map(|t| t.id).collect();
            for s in snapshot.iter().filter(|s| s.kind == TaskKind::Epic) {
                if let Some(task) = after.get(&s.issue.id) {
                    prop_assert!(cards.contains(task), "an epic became a subtask");
                    prop_assert!(b.epic(&s.issue.key).is_some(), "an epic card isn't named");
                }
            }
            // Link the first task, to follow it through later merges.
            if let Some((issue, task)) = after.iter().next() {
                let thread = ThreadId::from_u128(u128::from(issue.parse::<u32>().unwrap()) + 1);
                b.link_thread(*task, thread).unwrap();
                links.insert(issue.clone(), thread);
            }
            links.retain(|issue, _| after.contains_key(issue));

            // Merging the same snapshot again changes nothing.
            let again = b.clone();
            let report = b.merge_issues_keeping(snapshot, full, &keep, || TaskId::from_u128(1));
            prop_assert!(!report.changed);
            prop_assert_eq!(&b, &again);
        }
    }
}
