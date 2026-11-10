//! What is chosen on a Jira board, as JQL: what "Save as quick filter…" offers to save
//! (ADR 0037), for the person to read and change before Jira gets it.
//!
//! Each dimension becomes one clause (its values widening it, `x in (…)`), and clauses
//! narrow each other (`AND`), as the filter does. Quick filters that are on join in
//! parentheses. Text goes through [`urushi_core::board::jql`] so it can't break out of
//! its quotes.

use urushi_client::v1;
use urushi_core::board::jql::{any_of, string};

use super::{BoardFilter, Dim, UNASSIGNED, board_values};

/// The JQL for everything chosen on `board`; empty when nothing is.
pub fn selection_jql(f: &BoardFilter, board: &v1::Board) -> String {
    let mut clauses = Vec::new();
    let search = f.search.text().trim();
    if !search.is_empty() {
        clauses.push(format!("text ~ {}", string(search)));
    }
    if f.mine {
        clauses.push("assignee = currentUser()".to_owned());
    }
    for dim in Dim::ALL {
        let Some(values) = f.chosen.get(&dim).filter(|v| !v.is_empty()) else {
            continue;
        };
        let values: Vec<&str> = values.iter().map(String::as_str).collect();
        clauses.extend(clause(dim, &values, board));
    }
    if f.recent {
        clauses.push("updated >= -1d".to_owned());
    }
    if let Some(jira) = board.jira.as_deref() {
        clauses.extend(
            jira.quick_filters
                .iter()
                .filter(|q| f.quick.contains(&q.id) && !q.jql.trim().is_empty())
                .map(|q| format!("({})", q.jql.trim())),
        );
    }
    clauses.extend(
        board
            .quick_filters
            .iter()
            .filter(|q| f.saved.contains(&q.id))
            .filter_map(|q| q.rule.as_ref())
            .filter(|r| !r.jql.trim().is_empty())
            .map(|r| format!("({})", r.jql.trim())),
    );
    clauses.join(" AND ")
}

/// The clause for the values chosen in `dim`.
fn clause(dim: Dim, values: &[&str], board: &v1::Board) -> Option<String> {
    match dim {
        Dim::Assignee => {
            let people: Vec<&str> = values
                .iter()
                .copied()
                .filter(|v| *v != UNASSIGNED)
                .collect();
            let unassigned = values.contains(&UNASSIGNED);
            match (any_of("assignee", &people), unassigned) {
                (Some(c), true) => Some(format!("({c} OR assignee is EMPTY)")),
                (Some(c), false) => Some(c),
                (None, true) => Some("assignee is EMPTY".to_owned()),
                (None, false) => None,
            }
        }
        Dim::Reporter => any_of("reporter", values),
        Dim::Type => any_of("issuetype", values),
        Dim::Priority => any_of("priority", values),
        Dim::Status => any_of("status", values),
        // Labels are chosen ignoring case; Jira gets them as the issues spell them.
        Dim::Label => {
            let labels = board_values(board, Dim::Label);
            let spelled: Vec<&str> = values
                .iter()
                .map(|v| {
                    labels
                        .iter()
                        .find(|(value, _)| value == v)
                        .map_or(*v, |(_, label)| label.as_str())
                })
                .collect();
            any_of("labels", &spelled)
        }
        // An epic shows with its stories.
        Dim::Epic => {
            let list = any_of("parent", values)?;
            let keys = any_of("key", values)?;
            Some(format!("({list} OR {keys})"))
        }
        Dim::Sprint => ids("sprint", values),
        Dim::Component => ids("component", values),
        Dim::Version => ids("fixVersion", values),
    }
}

/// `field in (1, 2)` for values Jira knows by id: numbers stay numbers (a quoted number
/// would be read as a name).
fn ids(field: &str, values: &[&str]) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    let list = values
        .iter()
        .map(|v| {
            if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
                (*v).to_owned()
            } else {
                string(v)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("{field} in ({list})"))
}

#[cfg(test)]
mod tests {
    use urushi_client::v1;
    use urushi_ui::TextField;

    use super::*;

    fn board() -> v1::Board {
        v1::Board {
            columns: vec![v1::BoardColumn {
                tasks: vec![v1::Task {
                    tags: vec!["Payments".into()],
                    ..Default::default()
                }],
                ..Default::default()
            }],
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
                rule: Some(v1::QuickFilterRule {
                    jql: "status = Blocked".into(),
                    ..Default::default()
                }),
            }],
            ..Default::default()
        }
    }

    fn chose(f: &mut BoardFilter, dim: Dim, values: &[&str]) {
        for v in values {
            f.toggle(dim, v);
        }
    }

    #[test]
    fn nothing_chosen_is_no_jql() {
        assert_eq!(selection_jql(&BoardFilter::default(), &board()), "");
    }

    #[test]
    fn every_dimension_becomes_a_clause_and_they_narrow_each_other() {
        let mut f = BoardFilter::default();
        chose(&mut f, Dim::Assignee, &["acc-ada", UNASSIGNED]);
        chose(&mut f, Dim::Reporter, &["acc-grace"]);
        chose(&mut f, Dim::Type, &["Bug", "Story"]);
        chose(&mut f, Dim::Priority, &["High"]);
        chose(&mut f, Dim::Status, &["In Progress"]);
        chose(&mut f, Dim::Label, &["payments"]);
        chose(&mut f, Dim::Epic, &["ACME-9"]);
        chose(&mut f, Dim::Sprint, &["6"]);
        chose(&mut f, Dim::Component, &["10000"]);
        chose(&mut f, Dim::Version, &["10001"]);
        let jql = selection_jql(&f, &board());
        assert_eq!(
            jql,
            [
                r#"(assignee in ("acc-ada") OR assignee is EMPTY)"#,
                r#"issuetype in ("Bug", "Story")"#,
                r#"priority in ("High")"#,
                r#"status in ("In Progress")"#,
                r#"labels in ("Payments")"#,
                r#"(parent in ("ACME-9") OR key in ("ACME-9"))"#,
                "sprint in (6)",
                "component in (10000)",
                "fixVersion in (10001)",
                r#"reporter in ("acc-grace")"#,
            ]
            .join(" AND ")
        );
    }

    #[test]
    fn switches_search_and_quick_filters_join_in() {
        let mut f = BoardFilter {
            search: TextField::new(r#"say "hi""#),
            mine: true,
            recent: true,
            quick: [4].into(),
            saved: ["k".to_owned()].into(),
            ..BoardFilter::default()
        };
        chose(&mut f, Dim::Assignee, &[UNASSIGNED]);
        assert_eq!(
            selection_jql(&f, &board()),
            [
                r#"text ~ "say \"hi\"""#,
                "assignee = currentUser()",
                "assignee is EMPTY",
                "updated >= -1d",
                "(type = Bug)",
                "(status = Blocked)",
            ]
            .join(" AND ")
        );
    }
}
