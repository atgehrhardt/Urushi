//! Quick filters (protocol 1.21): saving what is chosen as one (JQL on a Jira board, a
//! search, kinds and tags elsewhere), turning it on, changing and deleting it from its
//! chip's menu, errors in the dialog, Jira's refusal in the banner, older daemons.

use urushi_client::v1::{self, request::Body, response::Outcome};

use super::board::board;
use super::jira::jira_board;
use super::{attached, bodies, named};
use crate::filter::Dim;
use crate::filter::quick::QuickRef;
use crate::keys::Named;
use crate::msg::{Effect, FilterMsg, Followup, Msg};
use crate::organize::MenuTarget;
use crate::state::{AppState, Dialog, Field};

/// A 1.21 daemon showing `b` (task "a" is a bug tagged "UI"), the panel open.
fn showing(b: v1::Board) -> AppState {
    let mut s = attached();
    s.daemon_minor = urushi_client::QUICK_FILTERS_SINCE_MINOR;
    let mut workspace = super::workspace();
    let id = b.id.clone();
    workspace.boards = vec![b];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)));
    s.update(Msg::ToggleBoard);
    s.update(Msg::SelectBoard(id));
    s
}

fn local() -> v1::Board {
    let mut b = board();
    b.columns[0].tasks[0].tags = vec!["UI".into()];
    b.columns[0].tasks[0].kind = v1::TaskKind::Bug.into();
    b
}

fn jira() -> v1::Board {
    let mut b = jira_board();
    b.jira.as_mut().unwrap().quick_filters = vec![v1::JiraQuickFilter {
        id: 4,
        name: "Bugs".into(),
        jql: "type = Bug".into(),
    }];
    b
}

fn filter(m: FilterMsg) -> Msg {
    Msg::Filter(m)
}

fn answer(s: &mut AppState, followup: Followup, result: Result<Outcome, String>) -> Vec<Effect> {
    s.update(Msg::Response { followup, result })
}

fn editor(s: &AppState) -> &crate::filter::editor::QuickFilterEditor {
    match &s.dialog {
        Some(Dialog::QuickFilter(e)) => e,
        other => panic!("no quick-filter dialog: {other:?}"),
    }
}

fn saved(filter: v1::quick_filter_ref::Filter, kept_here: &str) -> Outcome {
    Outcome::QuickFilterSaved(v1::QuickFilterSaved {
        filter: Some(v1::QuickFilterRef {
            filter: Some(filter),
        }),
        kept_here: kept_here.into(),
    })
}

/// The workspace now has `b` as the daemon keeps it.
fn changed(s: &mut AppState, b: v1::Board) -> Vec<Effect> {
    let mut workspace = super::workspace();
    workspace.boards = vec![b];
    s.update(Msg::Daemon(v1::event::Body::WorkspaceChanged(workspace)))
}

#[test]
fn a_local_selection_is_saved_as_search_kinds_and_tags_and_turned_on() {
    let mut s = showing(local());
    let b = s.board.board(&s.workspace).unwrap().clone();
    assert!(!s.can_save_quick_filter(&b), "nothing chosen");
    s.update(filter(FilterMsg::Toggle(Dim::Type, "Bug".into())));
    s.update(filter(FilterMsg::Toggle(Dim::Label, "ui".into())));
    s.update(filter(FilterMsg::OpenSave));
    assert_eq!(s.focus, Some(Field::QuickFilterName));
    let e = editor(&s);
    assert!(e.jql.is_none(), "no JQL off Jira");
    assert_eq!(
        e.rule.as_ref().unwrap().tags,
        ["UI"],
        "tags as the tasks spell them"
    );

    // No name: the dialog says so and nothing is sent.
    assert!(bodies(&s.update(Msg::Submit)).is_empty());
    assert!(editor(&s).error.is_some());
    s.update(Msg::Paste("UI bugs".into()));
    let fx = s.update(Msg::Key(named(Named::Enter)));
    let [
        Effect::Request {
            body: Body::CreateQuickFilter(r),
            followup: Followup::QuickFilterSaved(board),
        },
    ] = &fx[..]
    else {
        panic!("{fx:?}");
    };
    assert_eq!(
        (r.board_id.as_str(), r.name.as_str(), board.as_str()),
        ("b", "UI bugs", "b")
    );
    let rule = r.rule.as_ref().unwrap();
    assert_eq!(rule.kinds, [i32::from(v1::TaskKind::Bug)]);
    assert!(rule.jql.is_empty());
    assert!(editor(&s).busy);

    // Saved: the board shows just it, on, and decides it here (no request).
    let mut b = local();
    b.quick_filters = vec![v1::QuickFilter {
        id: "k".into(),
        name: "UI bugs".into(),
        rule: Some(rule.clone()),
    }];
    changed(&mut s, b);
    let fx = answer(
        &mut s,
        Followup::QuickFilterSaved("b".into()),
        Ok(saved(v1::quick_filter_ref::Filter::Id("k".into()), "")),
    );
    assert!(bodies(&fx).is_empty());
    assert!(s.dialog.is_none());
    assert!(
        s.board
            .filter
            .chosen
            .values()
            .all(std::collections::BTreeSet::is_empty)
    );
    assert!(s.board.filter.saved.contains("k"));
    let shown = s.board.board(&s.workspace).unwrap().clone();
    let cx = s.filter_context();
    assert!(s.board.filter.shows(&shown.columns[0].tasks[0], cx));
    assert!(!s.board.filter.shows(&shown.columns[0].tasks[1], cx));
    // Its chip turns it off.
    s.update(filter(FilterMsg::Saved("k".into())));
    assert!(!s.board.filter.active());
}

#[test]
fn a_jira_selection_is_saved_as_editable_jql_and_jira_evaluates_it() {
    let mut s = showing(jira());
    s.update(filter(FilterMsg::Toggle(Dim::Type, "Story".into())));
    s.update(filter(FilterMsg::OpenSave));
    assert_eq!(
        editor(&s).jql.as_ref().unwrap().text(),
        r#"issuetype in ("Story")"#
    );
    s.update(Msg::Paste("Stories".into()));
    s.update(Msg::FocusField(Field::QuickFilterJql));
    // Enter in the JQL starts a line; Cmd/Ctrl+Enter saves.
    assert!(bodies(&s.update(Msg::Key(named(Named::Enter)))).is_empty());
    s.update(Msg::Paste("AND priority = High".into()));
    let fx = s.update(Msg::Submit);
    let [
        Effect::Request {
            body: Body::CreateQuickFilter(r),
            ..
        },
    ] = &fx[..]
    else {
        panic!("{fx:?}");
    };
    let jql = &r.rule.as_ref().unwrap().jql;
    assert!(
        jql.starts_with(r#"issuetype in ("Story")"#) && jql.ends_with("AND priority = High"),
        "{jql}"
    );

    // Jira made it: on, and asked about.
    let mut b = jira();
    b.jira
        .as_mut()
        .unwrap()
        .quick_filters
        .push(v1::JiraQuickFilter {
            id: 5,
            name: "Stories".into(),
            jql: jql.clone(),
        });
    changed(&mut s, b);
    let fx = answer(
        &mut s,
        Followup::QuickFilterSaved("jb".into()),
        Ok(saved(v1::quick_filter_ref::Filter::JiraId(5), "")),
    );
    assert!(s.error.is_none());
    assert!(matches!(
        &bodies(&fx)[..],
        [Body::QueryJiraFilter(q)] if q.quick_filter_ids == [5] && q.kept_quick_filter_ids.is_empty()
    ));
}

#[test]
fn jiras_refusal_is_said_and_the_filter_kept_here_is_asked_about() {
    let mut s = showing(jira());
    s.update(filter(FilterMsg::Recent));
    s.update(filter(FilterMsg::OpenSave));
    s.update(Msg::Paste("Fresh".into()));
    s.update(Msg::Submit);
    // A failure stays in the dialog, which can try again.
    answer(
        &mut s,
        Followup::QuickFilterSaved("jb".into()),
        Err("Jira can't use that JQL".into()),
    );
    let e = editor(&s);
    assert_eq!(e.error.as_deref(), Some("Jira can't use that JQL"));
    assert!(!e.busy && s.error.is_none());
    assert!(!bodies(&s.update(Msg::Submit)).is_empty());

    let mut b = jira();
    b.quick_filters = vec![v1::QuickFilter {
        id: "k".into(),
        name: "Fresh".into(),
        rule: Some(v1::QuickFilterRule {
            jql: "updated >= -1d".into(),
            ..Default::default()
        }),
    }];
    changed(&mut s, b);
    let fx = answer(
        &mut s,
        Followup::QuickFilterSaved("jb".into()),
        Ok(saved(
            v1::quick_filter_ref::Filter::Id("k".into()),
            "you need to be a board admin",
        )),
    );
    assert!(s.error.as_deref().unwrap().contains("board admin"));
    assert!(matches!(
        &bodies(&fx)[..],
        [Body::QueryJiraFilter(q)] if q.quick_filter_ids.is_empty() && q.kept_quick_filter_ids == ["k"]
    ));
}

#[test]
fn a_chips_menu_edits_and_deletes_after_asking() {
    let mut s = showing(jira());
    s.update(filter(FilterMsg::Quick(4)));
    s.update(Msg::OpenMenu(MenuTarget::QuickFilter(
        "jb".into(),
        QuickRef::Jira(4),
    )));
    assert!(s.menu.is_some());
    s.update(filter(FilterMsg::OpenEdit(QuickRef::Jira(4))));
    assert!(s.menu.is_none());
    let e = editor(&s);
    assert_eq!(
        (e.name.text(), e.jql.as_ref().unwrap().text()),
        ("Bugs", "type = Bug")
    );
    s.update(Msg::Paste(" only".into()));
    let fx = s.update(Msg::Submit);
    let [
        Effect::Request {
            body: Body::UpdateQuickFilter(r),
            followup: Followup::QuickFilterChanged,
        },
    ] = &fx[..]
    else {
        panic!("{fx:?}");
    };
    assert_eq!(r.name, "Bugs only");
    assert_eq!(r.filter, Some(urushi_client::jira_filter(4)));
    // Changed while on: Jira is asked again.
    let fx = answer(
        &mut s,
        Followup::QuickFilterChanged,
        Ok(Outcome::Ok(v1::Empty {})),
    );
    assert!(s.dialog.is_none());
    assert!(matches!(&bodies(&fx)[..], [Body::QueryJiraFilter(_)]));

    s.update(filter(FilterMsg::OpenDelete(QuickRef::Jira(4))));
    assert!(matches!(&s.dialog, Some(Dialog::DeleteQuickFilter { name, .. }) if name == "Bugs"));
    assert!(
        bodies(&s.update(Msg::Key(named(Named::Enter)))).is_empty(),
        "Enter doesn't delete"
    );
    let fx = s.update(filter(FilterMsg::ConfirmDelete));
    assert!(matches!(
        &bodies(&fx)[..],
        [Body::DeleteQuickFilter(r)] if r.board_id == "jb" && r.filter == Some(urushi_client::jira_filter(4))
    ));
    assert!(!s.board.filter.active(), "it no longer filters");
}

#[test]
fn a_local_filter_is_renamed_or_given_the_current_filters() {
    let mut b = local();
    b.quick_filters = vec![v1::QuickFilter {
        id: "k".into(),
        name: "UI".into(),
        rule: Some(v1::QuickFilterRule {
            tags: vec!["UI".into()],
            ..Default::default()
        }),
    }];
    let mut s = showing(b);
    s.update(filter(FilterMsg::OpenEdit(QuickRef::Kept("k".into()))));
    assert!(editor(&s).rule.is_none(), "renaming keeps its rule");
    let fx = s.update(Msg::Submit);
    assert!(matches!(&bodies(&fx)[..], [Body::UpdateQuickFilter(r)] if r.rule.is_none()));
    s.update(Msg::CloseDialog);

    s.update(filter(FilterMsg::Toggle(Dim::Type, "Bug".into())));
    s.update(filter(FilterMsg::OpenEdit(QuickRef::Kept("k".into()))));
    s.update(filter(FilterMsg::UseCurrent));
    let fx = s.update(Msg::Submit);
    assert!(matches!(
        &bodies(&fx)[..],
        [Body::UpdateQuickFilter(r)] if r.rule.as_ref().is_some_and(|r| r.kinds == [i32::from(v1::TaskKind::Bug)])
    ));
}

#[test]
fn older_daemons_offer_no_saving_or_chip_menus() {
    let mut s = showing(local());
    s.daemon_minor = 20;
    s.update(filter(FilterMsg::Toggle(Dim::Type, "Bug".into())));
    let b = s.board.board(&s.workspace).unwrap().clone();
    assert!(!s.can_save_quick_filter(&b));
    s.update(filter(FilterMsg::OpenSave));
    assert!(s.dialog.is_none());
}
