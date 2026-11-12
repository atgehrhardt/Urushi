//! Wire → service for Jira (protocol 1.12): the sign-ins (several since 1.29, ADR
//! 0051), listing and importing boards,
//! syncing one now, and (1.13) evaluating its quick filters (since 1.23 the board's own
//! too). The API token comes in with `ConnectJira` and never goes out.

use urushi_proto::convert;
use urushi_proto::v1::{self, request::Body, response::Outcome};

use super::handlers::{invalid, ok, parse};
use crate::service::{JiraSignIn, Service, ServiceResult};

/// A sign-in on the wire (never its token); `None`: not connected.
fn wire_account(sign_in: Option<JiraSignIn>) -> v1::JiraAccount {
    match sign_in {
        Some(s) => v1::JiraAccount {
            connected: true,
            site: s.site,
            email: s.email,
            display_name: s.display_name,
            account_id: s.account_id,
            key: s.key,
        },
        None => v1::JiraAccount::default(),
    }
}

fn account(sign_in: Option<JiraSignIn>) -> Outcome {
    Outcome::JiraAccount(wire_account(sign_in))
}

/// Handles the Jira requests; anything else is refused as a routing bug.
pub(super) async fn route(service: &Service, body: Body) -> ServiceResult<Outcome> {
    Ok(match body {
        Body::ConnectJira(r) => account(Some(
            service
                .connect_jira(&r.site, &r.email, &r.api_token)
                .await?,
        )),
        Body::GetJiraAccount(_) => account(service.jira_sign_in()),
        Body::ListJiraAccounts(_) => Outcome::JiraAccounts(v1::JiraAccountList {
            accounts: service
                .jira_sign_ins()
                .into_iter()
                .map(|s| wire_account(Some(s)))
                .collect(),
        }),
        Body::DisconnectJira(r) => {
            service.disconnect_jira(&r.account)?;
            ok()
        }
        Body::ListJiraBoards(r) => {
            let boards = service.list_jira_boards(&r.account, &r.query).await?;
            Outcome::JiraBoards(v1::JiraBoardList {
                boards: boards
                    .into_iter()
                    .map(|(b, here)| v1::JiraBoardInfo {
                        id: b.id,
                        name: b.name,
                        scrum: b.scrum,
                        project_key: b.project_key,
                        project_name: b.project_name,
                        imported_board_id: here.map(|id| id.to_string()).unwrap_or_default(),
                    })
                    .collect(),
            })
        }
        Body::ImportJiraBoard(r) => Outcome::Board(convert::board_to_proto(
            &service
                .import_jira_board(&r.account, r.jira_board_id)
                .await?,
        )),
        Body::SyncBoard(r) => {
            service
                .sync_board(parse(&r.board_id, "board_id")?, true)
                .await?;
            ok()
        }
        Body::QueryJiraFilter(r) => {
            let kept = r
                .kept_quick_filter_ids
                .iter()
                .map(|id| parse(id, "kept_quick_filter_ids"))
                .collect::<ServiceResult<Vec<_>>>()?;
            Outcome::JiraIssueIds(v1::JiraIssueIds {
                ids: service
                    .query_jira_filter(parse(&r.board_id, "board_id")?, &r.quick_filter_ids, &kept)
                    .await?,
            })
        }
        _ => invalid("not a Jira request"),
    })
}
