#![allow(clippy::unwrap_used, reason = "test code")]

use super::*;
use crate::board::{BoardSource, JiraBoard};
use crate::{BoardId, ColumnId, Timestamp};

fn board() -> Board {
    Board::new(
        BoardId::from_u128(1),
        "B",
        Timestamp::from_unix_millis(0),
        [1, 2, 3].map(ColumnId::from_u128),
    )
    .unwrap()
}

fn jira_board() -> Board {
    let mut b = board();
    b.source = Some(BoardSource::Jira(JiraBoard {
        site: "https://acme.atlassian.net".into(),
        board_id: 7,
        name: "ACME".into(),
        project_key: "ACME".into(),
        scrum: false,
        rank_field: None,
        points_field: None,
        sprint_field: None,
        quick_filters: Vec::new(),
        sync: crate::board::SyncState::default(),
    }));
    b
}

fn fields(search: &str, kinds: &[TaskKind], tags: &[&str]) -> QuickRule {
    QuickRule::Fields {
        search: search.into(),
        kinds: kinds.to_vec(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
    }
}

fn jql(q: &str) -> QuickRule {
    QuickRule::Jql { jql: q.into() }
}

const A: QuickFilterId = QuickFilterId::from_u128(10);
const B: QuickFilterId = QuickFilterId::from_u128(11);

#[test]
fn rules_are_cleaned_up_and_must_filter_by_something() {
    assert_eq!(
        fields(
            "  login ",
            &[TaskKind::Bug, TaskKind::Bug],
            &["UI", "ui", " "]
        )
        .checked()
        .unwrap(),
        fields("login", &[TaskKind::Bug], &["UI"])
    );
    assert_eq!(
        fields(" ", &[], &[]).checked(),
        Err(BoardError::EmptyQuickFilter)
    );
    assert_eq!(jql("  ").checked(), Err(BoardError::EmptyQuickFilter));
    assert_eq!(jql(" type = Bug ").checked().unwrap(), jql("type = Bug"));
    assert!(matches!(
        jql(&"x".repeat(MAX_JQL + 1)).checked(),
        Err(BoardError::TooLong { max: MAX_JQL, .. })
    ));
    assert!(matches!(
        fields(&"x".repeat(MAX_SEARCH + 1), &[], &[]).checked(),
        Err(BoardError::TooLong {
            max: MAX_SEARCH,
            ..
        })
    ));
}

#[test]
fn filters_are_added_renamed_changed_and_removed() {
    let mut b = board();
    b.add_quick_filter(A, "  My   bugs ", fields("", &[TaskKind::Bug], &[]))
        .unwrap();
    b.add_quick_filter(B, "UI", fields("", &[], &["ui"]))
        .unwrap();
    assert_eq!(b.quick_filter(A).unwrap().name, "My bugs");
    assert_eq!(
        b.quick_filters.iter().map(|q| q.id).collect::<Vec<_>>(),
        [A, B]
    );

    b.update_quick_filter(A, Some("Bugs"), None).unwrap();
    b.update_quick_filter(A, None, Some(fields("crash", &[], &[])))
        .unwrap();
    let a = b.quick_filter(A).unwrap();
    assert_eq!(
        (a.name.as_str(), &a.rule),
        ("Bugs", &fields("crash", &[], &[]))
    );

    assert_eq!(b.remove_quick_filter(A).unwrap().name, "Bugs");
    assert_eq!(
        b.remove_quick_filter(A),
        Err(BoardError::NoSuchQuickFilter(A))
    );
    assert_eq!(b.quick_filters.len(), 1);
}

#[test]
fn a_rule_must_suit_the_board() {
    let mut local = board();
    assert!(matches!(
        local.add_quick_filter(A, "J", jql("type = Bug")),
        Err(BoardError::WrongQuickRule(_))
    ));
    let mut jira = jira_board();
    assert!(matches!(
        jira.add_quick_filter(A, "F", fields("x", &[], &[])),
        Err(BoardError::WrongQuickRule(_))
    ));
    jira.add_quick_filter(A, "Bugs", jql("type = Bug")).unwrap();
    assert_eq!(jira.quick_filter(A).unwrap().rule.jql(), Some("type = Bug"));
    assert!(local.quick_filters.is_empty());
}

#[test]
fn failed_changes_change_nothing() {
    let mut b = board();
    b.add_quick_filter(A, "Bugs", fields("", &[TaskKind::Bug], &[]))
        .unwrap();
    let before = b.clone();
    assert_eq!(
        b.update_quick_filter(A, Some(" "), Some(fields("x", &[], &[]))),
        Err(BoardError::EmptyFilterName)
    );
    assert_eq!(
        b.update_quick_filter(A, Some("New"), Some(fields("", &[], &[]))),
        Err(BoardError::EmptyQuickFilter)
    );
    assert_eq!(
        b.update_quick_filter(B, Some("New"), None),
        Err(BoardError::NoSuchQuickFilter(B))
    );
    assert_eq!(
        b.add_quick_filter(B, "", fields("x", &[], &[])),
        Err(BoardError::EmptyFilterName)
    );
    assert_eq!(b, before);
}

#[test]
fn a_board_keeps_at_most_its_bound() {
    let mut b = board();
    for i in 0..MAX_QUICK_FILTERS {
        b.add_quick_filter(
            QuickFilterId::from_u128(100 + i as u128),
            "F",
            fields("x", &[], &[]),
        )
        .unwrap();
    }
    assert_eq!(
        b.add_quick_filter(B, "One more", fields("x", &[], &[])),
        Err(BoardError::TooManyQuickFilters)
    );
}

#[test]
fn old_documents_have_none_and_rules_survive_serde() {
    let mut b = board();
    let json = serde_json::to_string(&b).unwrap();
    assert!(!json.contains("quick_filters"));
    b.add_quick_filter(A, "Bugs", fields("", &[TaskKind::Bug], &["ui"]))
        .unwrap();
    let back: Board = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
    assert_eq!(back, b);
    let jira: QuickRule = serde_json::from_str(r#"{"kind":"jql","jql":"type = Bug"}"#).unwrap();
    assert_eq!(jira, self::jql("type = Bug"));
}
