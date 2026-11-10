//! Wire → service for Jira (protocol 1.12): the sign-in, listing and importing boards,
//! and syncing one now. The API token comes in with `ConnectJira` and never goes out.

use urushi_proto::convert;
use urushi_proto::v1::{self, request::Body, response::Outcome};

use super::handlers::{invalid, ok, parse};
use crate::service::{JiraSignIn, Service, ServiceResult};

fn account(sign_in: Option<JiraSignIn>) -> Outcome {
    Outcome::JiraAccount(match sign_in {
        Some(s) => v1::JiraAccount {
            connected: true,
            site: s.site,
            email: s.email,
            display_name: s.display_name,
        },
        None => v1::JiraAccount::default(),
    })
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
        Body::DisconnectJira(_) => {
            service.disconnect_jira()?;
            ok()
        }
        Body::ListJiraBoards(r) => {
            let boards = service.list_jira_boards(&r.query).await?;
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
            &service.import_jira_board(r.jira_board_id).await?,
        )),
        Body::SyncBoard(r) => {
            service
                .sync_board(parse(&r.board_id, "board_id")?, true)
                .await?;
            ok()
        }
        _ => invalid("not a Jira request"),
    })
}
