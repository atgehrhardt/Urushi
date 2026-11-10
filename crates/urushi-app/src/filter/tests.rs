//! The board filter: each dimension, how they combine, and subtasks keeping their card.

use urushi_client::{TaskRef, v1};

use super::*;

fn issue(id: &str, assignee: (&str, &str), status: &str) -> Box<v1::JiraIssue> {
    Box::new(v1::JiraIssue {
        id: id.into(),
        key: format!("ACME-{id}"),
        type_name: "Story".into(),
        assignee_id: assignee.0.into(),
        assignee: assignee.1.into(),
        status: status.into(),
        priority: "High".into(),
        updated_ms: 1_000,
        sprint: Some(v1::JiraNamed {
            id: "6".into(),
            name: "Sprint 6".into(),
        }),
        ..Default::default()
    })
}

/// "Checkout" (Ada's, tagged payments) with a subtask of Grace's.
fn card() -> v1::Task {
    v1::Task {
        id: "t1".into(),
        title: "Checkout".into(),
        notes: "Pay with saved cards".into(),
        tags: vec!["Payments".into()],
        issue: Some(issue("1", ("acc-ada", "Ada"), "To Do")),
        subtasks: vec![v1::Subtask {
            id: "t2".into(),
            title: "Card form".into(),
            issue: Some(issue("2", ("acc-grace", "Grace"), "In Progress")),
            ..Default::default()
        }],
        ..Default::default()
    }
}

const CX: Context<'static> = Context {
    me: Some("acc-ada"),
    now_ms: 2_000,
};

#[test]
fn no_filter_shows_everything() {
    assert!(!BoardFilter::default().active());
    assert!(BoardFilter::default().shows(&card(), CX));
}

#[test]
fn search_matches_title_notes_and_key_ignoring_case() {
    let mut f = BoardFilter::default();
    for (text, shown) in [
        ("checkout", true),
        ("SAVED", true),
        ("acme-2", true),
        ("nope", false),
    ] {
        f.search = TextField::new(text.to_owned());
        assert_eq!(f.shows(&card(), CX), shown, "{text}");
    }
}

#[test]
fn a_subtask_that_passes_keeps_its_card() {
    let mut f = BoardFilter::default();
    f.toggle(Dim::Assignee, "acc-grace");
    assert!(f.shows(&card(), CX));
    assert!(!f.passes(TaskRef::Card(&card()), CX));
    f.toggle(Dim::Assignee, "acc-grace");
    f.toggle(Dim::Assignee, "acc-nobody");
    assert!(!f.shows(&card(), CX));
}

#[test]
fn values_widen_a_dimension_and_dimensions_narrow_each_other() {
    let mut f = BoardFilter::default();
    f.toggle(Dim::Status, "Done");
    f.toggle(Dim::Status, "To Do");
    assert!(f.passes(TaskRef::Card(&card()), CX), "either status");
    f.toggle(Dim::Label, "payments");
    assert!(f.passes(TaskRef::Card(&card()), CX), "labels ignore case");
    f.toggle(Dim::Priority, "Low");
    assert!(!f.shows(&card(), CX), "and a priority it hasn't");
    assert_eq!(f.count(), 4);
    f.clear();
    assert!(!f.active());
}

#[test]
fn only_my_issues_recent_ones_and_quick_filters() {
    let mut f = BoardFilter {
        mine: true,
        ..BoardFilter::default()
    };
    assert!(f.passes(TaskRef::Card(&card()), CX));
    assert!(
        !f.shows(&card(), Context { me: None, ..CX }),
        "nobody signed in"
    );
    f.mine = false;
    f.recent = true;
    assert!(f.shows(&card(), CX));
    assert!(!f.shows(
        &card(),
        Context {
            now_ms: 1_000 + RECENT_MS + 1,
            ..CX
        }
    ));
    f.recent = false;
    f.quick.insert(4);
    assert!(
        f.shows(&card(), CX),
        "until Jira answers, nothing is hidden"
    );
    f.quick_matches = Some(["2".to_owned()].into());
    assert!(f.shows(&card(), CX), "its subtask matches");
    f.quick_matches = Some(HashSet::new());
    assert!(!f.shows(&card(), CX));
}

#[test]
fn a_boards_values_come_from_every_depth() {
    let board = v1::Board {
        columns: vec![v1::BoardColumn {
            tasks: vec![card()],
            ..Default::default()
        }],
        jira: Some(Box::default()),
        ..Default::default()
    };
    let people = board_values(&board, Dim::Assignee);
    assert_eq!(
        people,
        [
            ("acc-ada".to_owned(), "Ada".to_owned()),
            ("acc-grace".to_owned(), "Grace".to_owned())
        ]
    );
    assert_eq!(board_values(&board, Dim::Sprint)[0].1, "Sprint 6");
    let dims = board_dims(&board);
    assert!(dims.contains(&Dim::Assignee) && !dims.contains(&Dim::Component));
    let local = v1::Board {
        jira: None,
        ..board
    };
    assert_eq!(board_dims(&local), [Dim::Type, Dim::Label]);
}
