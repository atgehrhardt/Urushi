//! Jira boards in the service (ADR 0020): the sign-in and importing boards. Keeping
//! them in sync is `jira_sync.rs`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use urushi_core::board::{JiraBoard, MAX_SYNCED_ISSUES, SyncState};
use urushi_core::{Board, BoardId, ColumnId};

use super::{Service, ServiceError, ServiceResult, State, blocking, lock, new_uuid, now};
use crate::jira::{
    Account, BoardConfig, BoardInfo, CredentialStore, IssueFields, IssueQuery, JiraApi, JiraError,
    site_address,
};

/// What a board says while the daemon has no sign-in.
pub const NOT_SIGNED_IN: &str = "Not signed in to Jira";

/// The service's Jira state.
pub(crate) struct JiraLink {
    pub(super) api: Arc<dyn JiraApi>,
    pub(super) store: CredentialStore,
    pub(super) account: Mutex<Option<Account>>,
    pub(super) boards: Mutex<HashMap<BoardId, super::jira_sync::BoardSync>>,
    /// The kinds of the fields of issues opened lately (by issue key, then field id),
    /// to write edits to them as Jira takes them.
    pub(super) kinds: Mutex<HashMap<String, HashMap<String, crate::jira::details::FieldKind>>>,
    /// How many connections watch each board (`WatchBoard`).
    pub(super) watched: Arc<Mutex<HashMap<BoardId, usize>>>,
    /// Reading linked issues' pull requests (ADR 0036).
    pub(super) dev: Mutex<super::jira_dev::DevReads>,
}

/// Who the daemon is signed in to Jira as (never the token).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JiraSignIn {
    /// The site's address.
    pub site: String,
    /// The account's email.
    pub email: String,
    /// The account's name in Jira.
    pub display_name: String,
    /// Its Atlassian account id (empty until read, for a sign-in from before 1.13).
    pub account_id: String,
}

impl JiraLink {
    pub(super) fn new(api: Arc<dyn JiraApi>, home: &std::path::Path) -> Self {
        let store = CredentialStore::in_home(home);
        let account = store.load();
        Self {
            api,
            store,
            account: Mutex::new(account),
            boards: Mutex::new(HashMap::new()),
            kinds: Mutex::new(HashMap::new()),
            watched: Arc::default(),
            dev: Mutex::default(),
        }
    }

    pub(super) fn account(&self) -> Option<Account> {
        lock(&self.account).clone()
    }

    /// The board's configuration from its last full read, if any.
    pub(super) fn cached_config(&self, board: BoardId) -> Option<BoardConfig> {
        lock(&self.boards)
            .get(&board)
            .and_then(|s| s.config.clone())
    }

    /// The lock that keeps syncs and writes of a board one at a time.
    pub(super) fn lock(&self, board: BoardId) -> Arc<tokio::sync::Mutex<()>> {
        lock(&self.boards).entry(board).or_default().lock.clone()
    }
}

/// The query for all of a board's issues.
pub(super) fn board_query(config: &BoardConfig) -> IssueQuery {
    IssueQuery {
        filter_id: config.filter_id,
        sub_query: config.sub_query.clone(),
        scrum: config.info.scrum,
        fields: IssueFields::of_config(config),
        updated_within_minutes: None,
        extra_jql: Vec::new(),
        limit: MAX_SYNCED_ISSUES,
    }
}

/// How a Jira failure reaches clients.
impl From<JiraError> for ServiceError {
    fn from(e: JiraError) -> Self {
        match e {
            JiraError::Unauthorized | JiraError::Forbidden(_) => Self::Forbidden(e.to_string()),
            JiraError::NotFound(_) => Self::NotFound(e.to_string().replace("Jira can't find ", "")),
            JiraError::Rejected(_) => Self::Invalid(e.to_string()),
            JiraError::RateLimited { .. } | JiraError::Unavailable(_) => {
                Self::Unavailable(e.to_string())
            }
        }
    }
}

impl State {
    /// The Jira board that `board` follows.
    pub(super) fn jira_board(&self, board: BoardId) -> ServiceResult<JiraBoard> {
        self.boards
            .iter()
            .find(|b| b.id == board)
            .ok_or_else(|| ServiceError::NotFound(format!("board {board}")))?
            .jira()
            .cloned()
            .ok_or_else(|| ServiceError::Invalid("that board doesn't follow Jira".into()))
    }
}

impl Service {
    /// Who the daemon is signed in to Jira as.
    pub fn jira_sign_in(&self) -> Option<JiraSignIn> {
        self.jira.account().map(|a| JiraSignIn {
            site: a.site,
            email: a.email,
            display_name: a.display_name,
            account_id: a.account_id,
        })
    }

    /// The sign-in, or why a Jira change can't be made without one.
    pub(super) fn jira_account(&self) -> ServiceResult<Account> {
        self.jira
            .account()
            .ok_or_else(|| ServiceError::Precondition(format!("{NOT_SIGNED_IN}: connect it first")))
    }

    /// Signs in to a Jira Cloud site, after Jira accepts the email and token, and keeps
    /// the sign-in for next time.
    pub async fn connect_jira(
        &self,
        site: &str,
        email: &str,
        token: &str,
    ) -> ServiceResult<JiraSignIn> {
        let (email, token) = (email.trim(), token.trim());
        if email.is_empty() || token.is_empty() {
            return Err(ServiceError::Invalid(
                "an email and an API token are needed".into(),
            ));
        }
        let mut account = Account {
            site: site_address(site).map_err(ServiceError::Invalid)?,
            email: email.to_owned(),
            token: token.to_owned(),
            display_name: String::new(),
            account_id: String::new(),
        };
        let api = self.jira.api.clone();
        let checking = account.clone();
        let me = blocking(move || Ok(api.myself(&checking)?)).await?;
        account.display_name = me.display_name;
        account.account_id = me.account_id;
        self.jira
            .store
            .save(&account)
            .map_err(|e| ServiceError::Internal(format!("saving the Jira sign-in: {e}")))?;
        tracing::info!(site = %account.site, "signed in to Jira");
        *lock(&self.jira.account) = Some(account);
        // Every board is due again at once.
        for sync in lock(&self.jira.boards).values_mut() {
            sync.reset();
        }
        self.jira_sign_in()
            .ok_or_else(|| ServiceError::Internal("the sign-in went away".into()))
    }

    /// Forgets the sign-in. Imported boards stay, saying they aren't syncing.
    pub fn disconnect_jira(&self) -> ServiceResult<()> {
        self.jira
            .store
            .delete()
            .map_err(|e| ServiceError::Internal(format!("forgetting the Jira sign-in: {e}")))?;
        *lock(&self.jira.account) = None;
        self.note_not_signed_in()
    }

    /// Marks every Jira board as not syncing for want of a sign-in.
    pub(super) fn note_not_signed_in(&self) -> ServiceResult<()> {
        self.mutate_if_changed(|state| {
            let mut changed = false;
            for jira in state.boards.iter_mut().filter_map(Board::jira_mut) {
                if jira.sync.error.as_deref() != Some(NOT_SIGNED_IN) {
                    jira.sync.error = Some(NOT_SIGNED_IN.into());
                    changed = true;
                }
            }
            Ok(changed)
        })
        .map(drop)
    }

    /// The site's boards whose name contains `query`, each with the board here that
    /// already follows it.
    pub async fn list_jira_boards(
        &self,
        query: &str,
    ) -> ServiceResult<Vec<(BoardInfo, Option<BoardId>)>> {
        let account = self.jira_account()?;
        let api = self.jira.api.clone();
        let query = query.to_owned();
        let boards = blocking(move || Ok(api.boards(&account, &query)?)).await?;
        let imported = self.imported_boards(&self.jira_account()?.site)?;
        Ok(boards
            .into_iter()
            .map(|b| {
                let here = imported.get(&b.id).copied();
                (b, here)
            })
            .collect())
    }

    /// Jira board id → the board here following it, on `site`.
    fn imported_boards(&self, site: &str) -> ServiceResult<HashMap<u64, BoardId>> {
        self.read(|state| {
            Ok(state
                .boards
                .iter()
                .filter_map(|b| {
                    let j = b.jira()?;
                    (j.site == site).then_some((j.board_id, b.id))
                })
                .collect())
        })
    }

    /// Makes a board that follows Jira board `jira_board`, with its columns, and starts
    /// reading its issues. A board already imported is answered as it is.
    pub async fn import_jira_board(&self, jira_board: u64) -> ServiceResult<Board> {
        let account = self.jira_account()?;
        if let Some(id) = self.imported_boards(&account.site)?.get(&jira_board) {
            let id = *id;
            return self.read(|state| {
                state
                    .boards
                    .iter()
                    .find(|b| b.id == id)
                    .cloned()
                    .ok_or_else(|| ServiceError::NotFound(format!("board {id}")))
            });
        }
        let api = self.jira.api.clone();
        let reading = account.clone();
        let config = blocking(move || Ok(api.board(&reading, jira_board)?)).await?;
        let board = Board::new_jira(
            BoardId::from_uuid(new_uuid()),
            JiraBoard {
                site: account.site.clone(),
                board_id: config.info.id,
                name: config.info.name.clone(),
                project_key: config.info.project_key.clone(),
                scrum: config.info.scrum,
                rank_field: config.rank_field.clone(),
                points_field: config.points_field.clone(),
                sprint_field: config.sprint_field.clone(),
                quick_filters: config.quick_filters.clone(),
                sync: SyncState::default(),
            },
            &config.columns,
            now(),
            || ColumnId::from_uuid(new_uuid()),
        )?;
        lock(&self.jira.boards).entry(board.id).or_default().config = Some(config);
        let board = self.mutate(|state| {
            state.boards.push(board.clone());
            Ok(board)
        })?;
        tracing::info!(board = %board.id, jira = jira_board, "imported a Jira board");
        let me = self.me.clone();
        let id = board.id;
        tokio::spawn(async move {
            if let Some(service) = me.upgrade()
                && let Err(e) = service.sync_board(id, true).await
            {
                tracing::warn!(board = %id, error = %e, "first sync failed");
            }
        });
        Ok(board)
    }
}
