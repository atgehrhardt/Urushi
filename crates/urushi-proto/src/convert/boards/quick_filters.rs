//! Quick filters (protocol 1.23, ADR 0037): core ⇄ wire for the filters a board keeps,
//! their rules, and references to a board's quick filters, Jira's own or kept.

use urushi_core::QuickFilter;
use urushi_core::board::{QuickFilterRef, QuickRule};

use super::jira::{kind_from_proto, kind_to_proto};
use super::{ConvertError, parse_id};
use crate::v1;

/// Core quick filter → wire.
pub fn quick_filter_to_proto(q: &QuickFilter) -> v1::QuickFilter {
    v1::QuickFilter {
        id: q.id.to_string(),
        name: q.name.clone(),
        rule: Some(rule_to_proto(&q.rule)),
    }
}

/// Wire quick filter → core. The id is validated; a missing rule reads as one that
/// filters by nothing.
pub fn quick_filter_from_proto(q: &v1::QuickFilter) -> Result<QuickFilter, ConvertError> {
    Ok(QuickFilter {
        id: parse_id(&q.id, "quick_filter.id")?,
        name: q.name.clone(),
        rule: rule_from_proto(&q.rule.clone().unwrap_or_default()),
    })
}

/// Core rule → wire.
pub fn rule_to_proto(rule: &QuickRule) -> v1::QuickFilterRule {
    match rule {
        QuickRule::Fields {
            search,
            kinds,
            tags,
        } => v1::QuickFilterRule {
            search: search.clone(),
            kinds: kinds.iter().map(|k| kind_to_proto(Some(*k))).collect(),
            tags: tags.clone(),
            jql: String::new(),
        },
        QuickRule::Jql { jql } => v1::QuickFilterRule {
            jql: jql.clone(),
            ..v1::QuickFilterRule::default()
        },
    }
}

/// Wire rule → core: JQL when it has any (the other criteria are then ignored), else
/// the search, kinds (unknown ones dropped) and tags. Not yet checked
/// ([`QuickRule::checked`]).
pub fn rule_from_proto(rule: &v1::QuickFilterRule) -> QuickRule {
    if rule.jql.trim().is_empty() {
        QuickRule::Fields {
            search: rule.search.clone(),
            kinds: rule
                .kinds
                .iter()
                .filter_map(|k| kind_from_proto(*k))
                .collect(),
            tags: rule.tags.clone(),
        }
    } else {
        QuickRule::Jql {
            jql: rule.jql.clone(),
        }
    }
}

/// Core reference → wire.
pub fn filter_ref_to_proto(r: QuickFilterRef) -> v1::QuickFilterRef {
    use v1::quick_filter_ref::Filter;
    v1::QuickFilterRef {
        filter: Some(match r {
            QuickFilterRef::Jira(id) => Filter::JiraId(id),
            QuickFilterRef::Kept(id) => Filter::Id(id.to_string()),
        }),
    }
}

/// Wire reference → core; a missing one is invalid.
pub fn filter_ref_from_proto(
    r: Option<&v1::QuickFilterRef>,
) -> Result<QuickFilterRef, ConvertError> {
    use v1::quick_filter_ref::Filter;
    match r.and_then(|r| r.filter.as_ref()) {
        Some(Filter::JiraId(id)) => Ok(QuickFilterRef::Jira(*id)),
        Some(Filter::Id(id)) => Ok(QuickFilterRef::Kept(parse_id(id, "filter.id")?)),
        None => Err(ConvertError::new("filter", "a quick filter is required")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test code")]

    use prost::Message;
    use urushi_core::{QuickFilterId, TaskKind};

    use super::*;

    #[test]
    fn quick_filters_and_rules_round_trip() {
        for rule in [
            QuickRule::Fields {
                search: "login".into(),
                kinds: vec![TaskKind::Bug, TaskKind::Story],
                tags: vec!["ui".into()],
            },
            QuickRule::Jql {
                jql: "type = Bug".into(),
            },
        ] {
            let q = QuickFilter {
                id: QuickFilterId::from_u128(7),
                name: "Bugs".into(),
                rule,
            };
            let wire = quick_filter_to_proto(&q);
            let decoded = v1::QuickFilter::decode(wire.encode_to_vec().as_slice()).unwrap();
            assert_eq!(quick_filter_from_proto(&decoded).unwrap(), q);
        }
    }

    #[test]
    fn jql_wins_and_unknown_kinds_are_dropped() {
        let rule = v1::QuickFilterRule {
            search: "x".into(),
            jql: "type = Bug".into(),
            ..Default::default()
        };
        assert_eq!(rule_from_proto(&rule).jql(), Some("type = Bug"));
        let rule = v1::QuickFilterRule {
            kinds: vec![99, kind_to_proto(Some(TaskKind::Bug))],
            ..Default::default()
        };
        assert_eq!(
            rule_from_proto(&rule),
            QuickRule::Fields {
                search: String::new(),
                kinds: vec![TaskKind::Bug],
                tags: Vec::new()
            }
        );
    }

    #[test]
    fn references_round_trip_and_must_be_given() {
        for r in [
            QuickFilterRef::Jira(12),
            QuickFilterRef::Kept(QuickFilterId::from_u128(3)),
        ] {
            assert_eq!(
                filter_ref_from_proto(Some(&filter_ref_to_proto(r))).unwrap(),
                r
            );
        }
        assert!(filter_ref_from_proto(None).is_err());
        assert!(filter_ref_from_proto(Some(&v1::QuickFilterRef::default())).is_err());
    }
}
