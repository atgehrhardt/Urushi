//! Typed wrappers for Jira requests (protocol 1.12): the daemon's sign-in, and finding,
//! importing and syncing Jira boards.

use urushi_proto::v1::{self, request::Body, response::Outcome};

use crate::requests::unexpected;
use crate::{Client, ClientError};

impl Client {
    /// Signs the daemon in to a Jira Cloud site. The token goes to the daemon only.
    pub async fn connect_jira(
        &self,
        site: &str,
        email: &str,
        api_token: &str,
    ) -> Result<v1::JiraAccount, ClientError> {
        let body = Body::ConnectJira(v1::ConnectJiraRequest {
            site: site.into(),
            email: email.into(),
            api_token: api_token.into(),
        });
        match self.request(body).await? {
            Outcome::JiraAccount(account) => Ok(account),
            _ => Err(unexpected("ConnectJira")),
        }
    }

    /// Who the daemon is signed in to Jira as.
    pub async fn jira_account(&self) -> Result<v1::JiraAccount, ClientError> {
        match self
            .request(Body::GetJiraAccount(v1::GetJiraAccountRequest {}))
            .await?
        {
            Outcome::JiraAccount(account) => Ok(account),
            _ => Err(unexpected("GetJiraAccount")),
        }
    }

    /// Forgets the daemon's Jira sign-in.
    pub async fn disconnect_jira(&self) -> Result<(), ClientError> {
        self.ok(
            Body::DisconnectJira(v1::DisconnectJiraRequest {}),
            "DisconnectJira",
        )
        .await
    }

    /// The site's boards whose name contains `query`.
    pub async fn list_jira_boards(
        &self,
        query: &str,
    ) -> Result<Vec<v1::JiraBoardInfo>, ClientError> {
        let body = Body::ListJiraBoards(v1::ListJiraBoardsRequest {
            query: query.into(),
        });
        match self.request(body).await? {
            Outcome::JiraBoards(list) => Ok(list.boards),
            _ => Err(unexpected("ListJiraBoards")),
        }
    }

    /// Makes a board that follows a Jira board (or answers the one that already does).
    pub async fn import_jira_board(&self, jira_board_id: u64) -> Result<v1::Board, ClientError> {
        let body = Body::ImportJiraBoard(v1::ImportJiraBoardRequest { jira_board_id });
        match self.request(body).await? {
            Outcome::Board(board) => Ok(board),
            _ => Err(unexpected("ImportJiraBoard")),
        }
    }

    /// Syncs a Jira board completely, now.
    pub async fn sync_board(&self, board_id: &str) -> Result<(), ClientError> {
        let body = Body::SyncBoard(v1::SyncBoardRequest {
            board_id: board_id.into(),
        });
        self.ok(body, "SyncBoard").await
    }

    /// The ids (`JiraIssue.id`) of a Jira board's issues that all of these quick
    /// filters let through, as Jira evaluates them (1.13).
    pub async fn query_jira_filter(
        &self,
        board_id: &str,
        quick_filter_ids: &[u64],
    ) -> Result<Vec<String>, ClientError> {
        let body = Body::QueryJiraFilter(v1::QueryJiraFilterRequest {
            board_id: board_id.into(),
            quick_filter_ids: quick_filter_ids.to_vec(),
        });
        match self.request(body).await? {
            Outcome::JiraIssueIds(ids) => Ok(ids.ids),
            _ => Err(unexpected("QueryJiraFilter")),
        }
    }

    /// Says which Jira board this connection shows (`None`: none), so the daemon reads
    /// it every few seconds (1.13).
    pub async fn watch_board(&self, board_id: Option<&str>) -> Result<(), ClientError> {
        let body = Body::WatchBoard(v1::WatchBoardRequest {
            board_id: board_id.unwrap_or_default().into(),
        });
        self.ok(body, "WatchBoard").await
    }
}
