//! Wire → service for a board's quick filters (protocol 1.21, ADR 0033): making,
//! changing and removing them. On a Jira board they are written to Jira.

use urushi_proto::convert;
use urushi_proto::v1::{self, request::Body, response::Outcome};

use super::handlers::{invalid, ok, parse};
use crate::service::{Service, ServiceError, ServiceResult};

/// Handles the quick-filter requests; anything else is refused as a routing bug.
pub(super) async fn route(service: &Service, body: Body) -> ServiceResult<Outcome> {
    Ok(match body {
        Body::CreateQuickFilter(r) => {
            let rule = convert::rule_from_proto(&r.rule.unwrap_or_default());
            let saved = service
                .create_quick_filter(parse(&r.board_id, "board_id")?, &r.name, rule)
                .await?;
            Outcome::QuickFilterSaved(v1::QuickFilterSaved {
                filter: Some(convert::filter_ref_to_proto(saved.filter)),
                kept_here: saved.kept_here.unwrap_or_default(),
            })
        }
        Body::UpdateQuickFilter(r) => {
            let name = (!r.name.trim().is_empty()).then_some(r.name.as_str());
            service
                .update_quick_filter(
                    parse(&r.board_id, "board_id")?,
                    filter(r.filter.as_ref())?,
                    name,
                    r.rule.as_ref().map(convert::rule_from_proto),
                )
                .await?;
            ok()
        }
        Body::DeleteQuickFilter(r) => {
            service
                .delete_quick_filter(parse(&r.board_id, "board_id")?, filter(r.filter.as_ref())?)
                .await?;
            ok()
        }
        _ => invalid("not a quick-filter request"),
    })
}

/// The filter a request names; a missing one is invalid.
fn filter(r: Option<&v1::QuickFilterRef>) -> ServiceResult<urushi_core::board::QuickFilterRef> {
    convert::filter_ref_from_proto(r).map_err(|e| ServiceError::Invalid(e.to_string()))
}
