//! Jira in the app (protocol 1.12): the sign-in dialog, the import picker, syncing a
//! board now and opening Jira's pages. The daemon does the rest (ADR 0020); the API
//! token typed here goes to it in `ConnectJira` and is dropped from the app at once.
//!
//! A 1.29 daemon keeps several sign-ins (ADR 0051): the app lists them all, imports
//! with the one chosen, and treats each board by the sign-in that syncs it (its own
//! "me", whether it can sync). An older daemon's one sign-in is a list of one.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_ui::TextField;

use super::request;
use crate::msg::{Effect, Followup, JiraMsg};
use crate::state::{AppState, Dialog, Field};

impl AppState {
    pub(super) fn jira_msg(&mut self, msg: JiraMsg, fx: &mut Vec<Effect>) {
        self.menu = None;
        match msg {
            JiraMsg::OpenConnect => self.open_connect_jira("", ""),
            JiraMsg::Reconnect(board) => {
                let (site, email) = self
                    .workspace
                    .board(&board)
                    .and_then(|b| b.jira.as_deref())
                    .map(|j| match j.account.split_once('/') {
                        Some((host, email)) => (host.to_owned(), email.to_owned()),
                        None => (
                            j.site.trim_start_matches("https://").to_owned(),
                            String::new(),
                        ),
                    })
                    .unwrap_or_default();
                self.open_connect_jira(&site, &email);
            }
            JiraMsg::Disconnect(key) => {
                fx.push(request(
                    Body::DisconnectJira(v1::DisconnectJiraRequest {
                        account: key.clone(),
                    }),
                    Followup::JiraAccount,
                ));
                self.jira_accounts.retain(|a| a.key != key);
                // An older daemon has one, whatever its key.
                if !self.daemon_supports_jira_accounts() {
                    self.jira_accounts.clear();
                }
            }
            JiraMsg::OpenImport => {
                if self.jira_accounts.is_empty() {
                    self.open_connect_jira("", "");
                } else {
                    let first = self.jira_accounts[0].key.clone();
                    self.open_import_jira(first, fx);
                }
            }
            JiraMsg::ImportFrom(key) => {
                if let Some(Dialog::ImportJira {
                    account,
                    boards,
                    query,
                    error,
                }) = &mut self.dialog
                {
                    account.clone_from(&key);
                    *boards = None;
                    *error = None;
                    Self::list_jira_boards(&key, query.text(), fx);
                }
            }
            JiraMsg::Import(id) => {
                let mut from = String::new();
                if let Some(Dialog::ImportJira { error, account, .. }) = &mut self.dialog {
                    *error = None;
                    from.clone_from(account);
                }
                let body = Body::ImportJiraBoard(v1::ImportJiraBoardRequest {
                    jira_board_id: id,
                    account: from,
                });
                fx.push(request(body, Followup::BoardCreated));
            }
            JiraMsg::Sync(board) => {
                let body = Body::SyncBoard(v1::SyncBoardRequest { board_id: board });
                fx.push(request(body, Followup::None));
            }
            JiraMsg::Open(url) => fx.push(Effect::OpenUrl(url)),
        }
    }

    /// Asks for the daemon's Jira sign-ins (when it can say): all of them from a 1.29
    /// daemon, else its one.
    pub(super) fn request_jira_account(&self, fx: &mut Vec<Effect>) {
        if self.daemon_supports_jira_accounts() {
            fx.push(request(
                Body::ListJiraAccounts(v1::ListJiraAccountsRequest {}),
                Followup::JiraAccounts,
            ));
        } else if self.daemon_supports_jira() {
            fx.push(request(
                Body::GetJiraAccount(v1::GetJiraAccountRequest {}),
                Followup::JiraAccount,
            ));
        }
    }

    /// The sign-in that syncs `board`: the one it names, else (a board from before
    /// several, or an older daemon) the one for its site.
    pub(crate) fn jira_account_of(&self, board: &v1::Board) -> Option<&v1::JiraAccount> {
        let jira = board.jira.as_deref()?;
        if jira.account.is_empty() {
            self.jira_accounts.iter().find(|a| a.site == jira.site)
        } else {
            self.jira_accounts.iter().find(|a| a.key == jira.account)
        }
    }

    /// The sign-in of the board on screen: who "me" is there.
    pub(crate) fn shown_jira_account(&self) -> Option<&v1::JiraAccount> {
        self.board
            .board(&self.workspace)
            .and_then(|b| self.jira_account_of(b))
    }

    /// Keeps a sign-in the daemon told about: replaces the one with its key, or adds it.
    fn keep_jira_account(&mut self, account: v1::JiraAccount) {
        if !account.connected {
            return;
        }
        match self
            .jira_accounts
            .iter_mut()
            .find(|a| a.key == account.key && a.site == account.site)
        {
            Some(same) => *same = account,
            None => self.jira_accounts.push(account),
        }
    }

    fn open_connect_jira(&mut self, site: &str, email: &str) {
        let focus = if site.is_empty() {
            Field::JiraSite
        } else if email.is_empty() {
            Field::JiraEmail
        } else {
            Field::JiraToken
        };
        self.open(
            Dialog::ConnectJira {
                site: TextField::new(site),
                email: TextField::new(email),
                token: TextField::secret(""),
                busy: false,
                error: None,
            },
            Some(focus),
        );
    }

    fn open_import_jira(&mut self, account: String, fx: &mut Vec<Effect>) {
        Self::list_jira_boards(&account, "", fx);
        self.open(
            Dialog::ImportJira {
                account,
                query: TextField::default(),
                boards: None,
                error: None,
            },
            Some(Field::JiraQuery),
        );
    }

    fn list_jira_boards(account: &str, query: &str, fx: &mut Vec<Effect>) {
        let body = Body::ListJiraBoards(v1::ListJiraBoardsRequest {
            query: query.trim().to_owned(),
            account: account.to_owned(),
        });
        fx.push(request(body, Followup::JiraBoards));
    }

    /// Enter or the main button in a Jira dialog.
    pub(super) fn submit_jira(&mut self, fx: &mut Vec<Effect>) {
        match &mut self.dialog {
            Some(Dialog::ConnectJira {
                site,
                email,
                token,
                busy,
                error,
            }) if !*busy => {
                if [&*site, &*email, &*token]
                    .iter()
                    .any(|f| f.text().trim().is_empty())
                {
                    *error = Some("Fill in the site, your email and an API token.".into());
                    return;
                }
                *busy = true;
                *error = None;
                let body = Body::ConnectJira(v1::ConnectJiraRequest {
                    site: site.text().trim().to_owned(),
                    email: email.text().trim().to_owned(),
                    api_token: token.text().trim().to_owned(),
                });
                fx.push(request(body, Followup::JiraConnected));
            }
            Some(Dialog::ImportJira {
                query,
                boards,
                account,
                ..
            }) => {
                *boards = None;
                Self::list_jira_boards(account, query.text(), fx);
            }
            _ => {}
        }
    }

    /// A Jira request failed: the dialog that asked says why. Returns whether it was
    /// one of those.
    pub(super) fn jira_failed(&mut self, followup: &Followup, message: &str) -> bool {
        match (followup, &mut self.dialog) {
            (Followup::JiraAccount, _) => true,
            // Nothing hidden by the quick filters; the banner says why.
            (Followup::JiraIssue(_), _) => {
                self.issue_failed(followup);
                false
            }
            (Followup::QuickFilter(_, query), _) => {
                if *query == self.board.quick_query {
                    self.board.quick_asking = false;
                    self.board.filter.quick_matches = None;
                }
                false
            }
            (Followup::JiraConnected, Some(Dialog::ConnectJira { busy, error, .. })) => {
                *busy = false;
                *error = Some(message.to_owned());
                true
            }
            (
                Followup::JiraBoards | Followup::BoardCreated,
                Some(Dialog::ImportJira { boards, error, .. }),
            ) => {
                boards.get_or_insert_with(Vec::new);
                *error = Some(message.to_owned());
                true
            }
            _ => false,
        }
    }

    /// A Jira request's answer. Returns it back if it isn't one.
    pub(super) fn jira_answered(
        &mut self,
        followup: Followup,
        outcome: Outcome,
        fx: &mut Vec<Effect>,
    ) -> Option<(Followup, Outcome)> {
        match (followup, outcome) {
            (Followup::JiraAccount, Outcome::JiraAccount(account)) => {
                self.jira_accounts = vec![account].into_iter().filter(|a| a.connected).collect();
            }
            // Signed out of one: ask again what is left.
            (Followup::JiraAccount, Outcome::Ok(_)) => self.request_jira_account(fx),
            (Followup::JiraAccounts, Outcome::JiraAccounts(list)) => {
                self.jira_accounts = list.accounts.into_iter().filter(|a| a.connected).collect();
            }
            (Followup::JiraConnected, Outcome::JiraAccount(account)) => {
                // An older daemon replaced its one sign-in.
                if !self.daemon_supports_jira_accounts() {
                    self.jira_accounts.clear();
                }
                let key = account.key.clone();
                self.keep_jira_account(account);
                // Signed in: on to choosing a board of that site.
                if matches!(self.dialog, Some(Dialog::ConnectJira { .. })) {
                    self.open_import_jira(key, fx);
                }
            }
            (Followup::JiraBoards, Outcome::JiraBoards(list)) => {
                if let Some(Dialog::ImportJira { boards, error, .. }) = &mut self.dialog {
                    *boards = Some(list.boards);
                    *error = None;
                }
            }
            (Followup::QuickFilter(board, query), Outcome::JiraIssueIds(ids)) => {
                self.quick_filter_answered((&board, query), Some(ids.ids), fx);
            }
            (Followup::QuickFilterSaved(board), Outcome::QuickFilterSaved(saved)) => {
                self.quick_filter_saved(&board, saved, fx);
            }
            (Followup::QuickFilterChanged, Outcome::Ok(_)) => self.quick_filter_changed(fx),
            other => return self.issue_answered(other.0, other.1),
        }
        None
    }
}
