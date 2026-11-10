//! Quick filters a board keeps: evaluating their rules, building one from what is
//! chosen, and how they combine with the rest of the filter.

use urushi_client::{TaskRef, v1};
use urushi_ui::TextField;

use super::*;
use crate::filter::Context;

fn task(title: &str, kind: v1::TaskKind, tags: &[&str]) -> v1::Task {
    v1::Task {
        id: title.into(),
        title: title.into(),
        notes: "Pay with saved cards".into(),
        kind: kind.into(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        ..Default::default()
    }
}

fn rule(search: &str, kinds: &[v1::TaskKind], tags: &[&str]) -> v1::QuickFilterRule {
    v1::QuickFilterRule {
        search: search.into(),
        kinds: kinds.iter().map(|k| i32::from(*k)).collect(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        jql: String::new(),
    }
}

#[test]
fn a_rule_needs_every_criterion_it_gives() {
    let bug = task("Login crash", v1::TaskKind::Bug, &["UI"]);
    let t = TaskRef::Card(&bug);
    assert!(passes(&rule("crash", &[], &[]), t));
    assert!(
        passes(&rule("SAVED CARDS", &[], &[]), t),
        "notes, ignoring case"
    );
    assert!(!passes(&rule("checkout", &[], &[]), t));
    assert!(passes(
        &rule("", &[v1::TaskKind::Story, v1::TaskKind::Bug], &[]),
        t
    ));
    assert!(!passes(&rule("", &[v1::TaskKind::Story], &[]), t));
    assert!(
        passes(&rule("", &[], &["ui", "api"]), t),
        "any tag, ignoring case"
    );
    assert!(!passes(&rule("crash", &[v1::TaskKind::Bug], &["api"]), t));
    let jql = v1::QuickFilterRule {
        jql: "type = Bug".into(),
        ..Default::default()
    };
    assert!(passes(&jql, t), "Jira decides JQL");
}

#[test]
fn kept_filters_on_narrow_the_board_and_keep_a_card_with_a_passing_subtask() {
    let mut card = task("Checkout", v1::TaskKind::Story, &[]);
    card.subtasks = vec![v1::Subtask {
        id: "s".into(),
        title: "Card form".into(),
        tags: vec!["ui".into()],
        ..Default::default()
    }];
    let other = task("Invoices", v1::TaskKind::Task, &[]);
    let kept = [v1::QuickFilter {
        id: "k".into(),
        name: "UI".into(),
        rule: Some(rule("", &[], &["ui"])),
    }];
    let cx = Context {
        me: None,
        now_ms: 0,
        kept: &kept,
    };
    let mut f = BoardFilter::default();
    f.saved.insert("k".into());
    assert!(f.active());
    assert_eq!(f.count(), 1);
    assert!(!f.needs_jira(&kept), "Urushi evaluates it");
    assert!(f.shows(&card, cx));
    assert!(!f.shows(&other, cx));
    // A filter deleted meanwhile hides nothing.
    let gone = Context { kept: &[], ..cx };
    assert!(f.shows(&other, gone));
}

#[test]
fn the_selection_becomes_a_rule_with_tags_as_the_tasks_spell_them() {
    let board = v1::Board {
        columns: vec![v1::BoardColumn {
            tasks: vec![task("A", v1::TaskKind::Bug, &["Payments"])],
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut f = BoardFilter::default();
    assert_eq!(rule_from_selection(&f, &board), None);
    f.search = TextField::new(" login ");
    f.toggle(Dim::Type, "Bug");
    f.toggle(Dim::Label, "payments");
    let r = rule_from_selection(&f, &board).unwrap();
    assert_eq!(r, rule("login", &[v1::TaskKind::Bug], &["Payments"]));
    assert_eq!(
        describe(&r),
        "Search \"login\" · Type: Bug · Tags: Payments"
    );
}

#[test]
fn references_name_their_filter_and_know_whether_it_is_on() {
    let board = v1::Board {
        jira: Some(Box::new(v1::JiraBoardSource {
            quick_filters: vec![v1::JiraQuickFilter {
                id: 4,
                name: "Bugs".into(),
                jql: "type = Bug".into(),
            }],
            ..Default::default()
        })),
        quick_filters: vec![v1::QuickFilter {
            id: "k".into(),
            name: "Blocked".into(),
            rule: None,
        }],
        ..Default::default()
    };
    let mut f = BoardFilter::default();
    f.quick.insert(4);
    assert_eq!(QuickRef::Jira(4).name(&board), Some("Bugs"));
    assert_eq!(QuickRef::Kept("k".into()).name(&board), Some("Blocked"));
    assert_eq!(QuickRef::Jira(5).name(&board), None);
    assert!(QuickRef::Jira(4).is_on(&f));
    assert!(!QuickRef::Kept("k".into()).is_on(&f));
}
