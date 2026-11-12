//! Jira boards in the service (ADR 0020): the sign-ins (several, ADR 0049) and
//! importing boards. Keeping them in sync is `jira_sync.rs`.
//!
//! Each board is read and written with its own sign-in: the one its
//! `JiraBoard.account` names, or (for a board imported before there could be several)
//! the first for its site.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use urushi_core::board::{JiraBoard, MAX_SYNCED_ISSUES, SyncState};
use urushi_core::{Board, BoardId, ColumnId};

use super::{Service, ServiceError, ServiceResult, State, blocking, lock, new_uuid, now};
use crate::jira::{
    Account, BoardConfig, BoardInfo, CredentialStore, IssueFields, IssueQuery, JiraApi, JiraError,
    site_address,
};

/// What a board says while the daemon has no sign-in for it.
pub const NOT_SIGNED_IN: &str = "Not signed in to Jira";

/// The service's Jira state.
pub(crate) struct JiraLink {
    pub(super) api: Arc<dyn JiraApi>,
    pub(super) store: CredentialStore,
    /// The sign-ins, in the order they were made.
    pub(super) accounts: Mutex<Vec<Account>>,
    pub(super) boards: Mutex<HashMap<BoardId, super::jira_sync::BoardSync>>,
    /// The kinds of the fields of issues opened lately (by board and issue key, as two
    /// sites can have the same key, then field id), to write edits to them as Jira
    /// takes them.
    pub(super) kinds: Mutex<IssueFieldKinds>,
    /// How many connections watch each board (`WatchBoard`).
    pub(super) watched: Arc<Mutex<HashMap<BoardId, usize>>>,
    /// Reading linked issues' pull requests (ADR 0036).
    pub(super) dev: Mutex<super::jira_dev::DevReads>,
}

/// Field kinds by board and issue key, then field id.
pub(super) type IssueFieldKinds =
    HashMap<(BoardId, String), HashMap<String, crate::jira::details::FieldKind>>;

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
    /// What names it ([`Account::key`]).
    pub key: String,
}

impl From<&Account> for JiraSignIn {
    fn from(a: &Account) -> Self {
        Self {
            site: a.site.clone(),
            email: a.email.clone(),
            display_name: a.display_name.clone(),
            account_id: a.account_id.clone(),
            key: a.key(),
        }
    }
}

impl JiraLink {
    pub(super) fn new(api: Arc<dyn JiraApi>, home: &std::path::Path) -> Self {
        let store = CredentialStore::in_home(home);
        let accounts = store.load();
        Self {
            api,
            store,
            accounts: Mutex::new(accounts),
            boards: Mutex::new(HashMap::new()),
            kinds: Mutex::new(HashMap::new()),
            watched: Arc::default(),
            dev: Mutex::default(),
        }
    }

    /// Every sign-in.
    pub(super) fn accounts(&self) -> Vec<Account> {
        lock(&self.accounts).clone()
    }

    /// The sign-in named `key`; an empty key names the first.
    pub(super) fn account_named(&self, key: &str) -> Option<Account> {
        let accounts = lock(&self.accounts);
        if key.is_empty() {
            return accounts.first().cloned();
        }
        accounts.iter().find(|a| a.key() == key).cloned()
    }

    /// The sign-in that reads and writes `board` ([`account_for`]).
    pub(super) fn account_for(&self, board: &JiraBoard) -> Option<Account> {
        account_for(&lock(&self.accounts), board).cloned()
    }

    /// Keeps the sign-ins, in memory and in their file.
    pub(super) fn save_accounts(&self, accounts: Vec<Account>) -> ServiceResult<()> {
        self.store
            .save(&accounts)
            .map_err(|e| ServiceError::Internal(format!("saving the Jira sign-ins: {e}")))?;
        *lock(&self.accounts) = accounts;
        Ok(())
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

/// The sign-in among `accounts` that reads and writes `board`: the one it names, else
/// (a board imported before several sign-ins were kept) the first for its site.
pub(super) fn account_for<'a>(accounts: &'a [Account], board: &JiraBoard) -> Option<&'a Account> {
    if board.account.is_empty() {
        accounts.iter().find(|a| a.site == board.site)
    } else {
        accounts.iter().find(|a| a.key() == board.account)
    }
}

/// What a board with no sign-in says: which site to sign in to.
pub(super) fn not_signed_in(board: &JiraBoard) -> String {
    format!(
        "{NOT_SIGNED_IN} at {}",
        board.site.trim_start_matches("https://")
    )
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
    /// The daemon's Jira sign-ins, in the order they were made.
    pub fn jira_sign_ins(&self) -> Vec<JiraSignIn> {
        self.jira.accounts().iter().map(JiraSignIn::from).collect()
    }

    /// The first sign-in: what `GetJiraAccount` answers, for clients before 1.27.
    pub fn jira_sign_in(&self) -> Option<JiraSignIn> {
        self.jira.account_named("").as_ref().map(JiraSignIn::from)
    }

    /// The sign-in that reads and writes `board`, or why a Jira change can't be made
    /// without one.
    pub(super) fn jira_account(&self, board: BoardId) -> ServiceResult<Account> {
        let jira = self.read(|state| state.jira_board(board))?;
        self.jira.account_for(&jira).ok_or_else(|| {
            ServiceError::Precondition(format!("{}: connect it first", not_signed_in(&jira)))
        })
    }

    /// The sign-in named `key` (empty: the first), or why there is none.
    fn jira_account_named(&self, key: &str) -> ServiceResult<Account> {
        self.jira.account_named(key).ok_or_else(|| {
            if key.is_empty() || self.jira.accounts().is_empty() {
                ServiceError::Precondition(format!("{NOT_SIGNED_IN}: connect it first"))
            } else {
                ServiceError::NotFound(format!("Jira sign-in {key}"))
            }
        })
    }

    /// The site of Jira board `board` (where its issues' links go).
    pub fn jira_site(&self, board: BoardId) -> ServiceResult<String> {
        Ok(self.read(|state| state.jira_board(board))?.site)
    }

    /// The Atlassian account id the sign-in of the board holding task `id` has, and
    /// its site: who "mine" is in an issue's comments, and where its links go.
    pub(crate) fn jira_me_for_task(&self, id: urushi_core::TaskId) -> (String, String) {
        let Ok(at) = self.issue_at(id) else {
            return Default::default();
        };
        let account_id = self
            .jira
            .account_for(&at.jira)
            .map(|a| a.account_id)
            .unwrap_or_default();
        (account_id, at.jira.site)
    }

    /// Signs in to a Jira Cloud site, after Jira accepts the email and token, and keeps
    /// the sign-in for next time beside the others. Signing in again with the same site
    /// and email replaces that sign-in (a new token).
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
        let key = account.key();
        let mut accounts = self.jira.accounts();
        match accounts.iter_mut().find(|a| a.key() == key) {
            Some(same) => *same = account.clone(),
            None => accounts.push(account.clone()),
        }
        self.jira.save_accounts(accounts)?;
        tracing::info!(site = %account.site, "signed in to Jira");
        // Its boards are due again at once.
        let boards = self.boards_of(&key)?;
        for (id, sync) in lock(&self.jira.boards).iter_mut() {
            if boards.contains(id) {
                sync.reset();
            }
        }
        Ok(JiraSignIn::from(&account))
    }

    /// The boards the sign-in named `key` reads and writes.
    fn boards_of(&self, key: &str) -> ServiceResult<Vec<BoardId>> {
        let accounts = self.jira.accounts();
        self.read(|state| {
            Ok(state
                .boards
                .iter()
                .filter(|b| {
                    b.jira()
                        .and_then(|j| account_for(&accounts, j))
                        .is_some_and(|a| a.key() == key)
                })
                .map(|b| b.id)
                .collect())
        })
    }

    /// Forgets the sign-in named `key` (empty: the first). The boards it read stay,
    /// saying they aren't syncing; the other sign-ins' boards carry on.
    pub fn disconnect_jira(&self, key: &str) -> ServiceResult<()> {
        let gone = self.jira_account_named(key)?.key();
        let accounts = self
            .jira
            .accounts()
            .into_iter()
            .filter(|a| a.key() != gone)
            .collect();
        self.jira.save_accounts(accounts)?;
        tracing::info!("signed out of a Jira site");
        self.note_not_signed_in()
    }

    /// Marks every Jira board that no sign-in reads as not syncing for want of one.
    pub(super) fn note_not_signed_in(&self) -> ServiceResult<()> {
        let accounts = self.jira.accounts();
        self.mutate_if_changed(|state| {
            let mut changed = false;
            for jira in state.boards.iter_mut().filter_map(Board::jira_mut) {
                if account_for(&accounts, jira).is_some() {
                    continue;
                }
                let error = not_signed_in(jira);
                if jira.sync.error.as_deref() != Some(error.as_str()) {
                    jira.sync.error = Some(error);
                    changed = true;
                }
            }
            Ok(changed)
        })
        .map(drop)
    }

    /// The boards of the site of the sign-in named `account` (empty: the first) whose
    /// name contains `query`, each with the board here that already follows it.
    pub async fn list_jira_boards(
        &self,
        account: &str,
        query: &str,
    ) -> ServiceResult<Vec<(BoardInfo, Option<BoardId>)>> {
        let account = self.jira_account_named(account)?;
        let imported = self.imported_boards(&account.site)?;
        let api = self.jira.api.clone();
        let query = query.to_owned();
        let boards = blocking(move || Ok(api.boards(&account, &query)?)).await?;
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

    /// Makes a board that follows Jira board `jira_board`, read and written with the
    /// sign-in named `account` (empty: the first), with its columns, and starts reading
    /// its issues. A board already imported is answered as it is.
    pub async fn import_jira_board(&self, account: &str, jira_board: u64) -> ServiceResult<Board> {
        let account = self.jira_account_named(account)?;
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
                account: account.key(),
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
