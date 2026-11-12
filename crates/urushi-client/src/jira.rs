//! Typed wrappers for Jira requests (protocol 1.12): the daemon's sign-ins (several
//! since 1.29, ADR 0051), and finding, importing and syncing Jira boards. An `account`
//! is a sign-in's `JiraAccount.key`; empty means the first, as daemons before 1.29
//! have only one.

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

    /// Who the daemon is signed in to Jira as: its first sign-in.
    pub async fn jira_account(&self) -> Result<v1::JiraAccount, ClientError> {
        match self
            .request(Body::GetJiraAccount(v1::GetJiraAccountRequest {}))
            .await?
        {
            Outcome::JiraAccount(account) => Ok(account),
            _ => Err(unexpected("GetJiraAccount")),
        }
    }

    /// Every Jira sign-in the daemon has (1.29).
    pub async fn jira_accounts(&self) -> Result<Vec<v1::JiraAccount>, ClientError> {
        match self
            .request(Body::ListJiraAccounts(v1::ListJiraAccountsRequest {}))
            .await?
        {
            Outcome::JiraAccounts(list) => Ok(list.accounts),
            _ => Err(unexpected("ListJiraAccounts")),
        }
    }

    /// Forgets one of the daemon's Jira sign-ins.
    pub async fn disconnect_jira(&self, account: &str) -> Result<(), ClientError> {
        self.ok(
            Body::DisconnectJira(v1::DisconnectJiraRequest {
                account: account.into(),
            }),
            "DisconnectJira",
        )
        .await
    }

    /// The boards of `account`'s site whose name contains `query`.
    pub async fn list_jira_boards(
        &self,
        account: &str,
        query: &str,
    ) -> Result<Vec<v1::JiraBoardInfo>, ClientError> {
        let body = Body::ListJiraBoards(v1::ListJiraBoardsRequest {
            query: query.into(),
            account: account.into(),
        });
        match self.request(body).await? {
            Outcome::JiraBoards(list) => Ok(list.boards),
            _ => Err(unexpected("ListJiraBoards")),
        }
    }

    /// Makes a board that follows a Jira board of `account`'s site, synced with that
    /// sign-in (or answers the one that already does).
    pub async fn import_jira_board(
        &self,
        account: &str,
        jira_board_id: u64,
    ) -> Result<v1::Board, ClientError> {
        let body = Body::ImportJiraBoard(v1::ImportJiraBoardRequest {
            jira_board_id,
            account: account.into(),
        });
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
        self.query_quick_filters(board_id, quick_filter_ids, &[])
            .await
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
