//! Jira in the app (protocol 1.12): the sign-in dialog, the import picker, syncing a
//! board now and opening Jira's pages. The daemon does the rest (ADR 0020); the API
//! token typed here goes to it in `ConnectJira` and is dropped from the app at once.

use urushi_client::v1::{self, request::Body, response::Outcome};
use urushi_ui::TextField;

use super::request;
use crate::msg::{Effect, Followup, JiraMsg};
use crate::state::{AppState, Dialog, Field};

impl AppState {
    pub(super) fn jira_msg(&mut self, msg: JiraMsg, fx: &mut Vec<Effect>) {
        self.menu = None;
        match msg {
            JiraMsg::OpenConnect => self.open_connect_jira(),
            JiraMsg::Disconnect => {
                fx.push(request(
                    Body::DisconnectJira(v1::DisconnectJiraRequest {}),
                    Followup::JiraAccount,
                ));
                self.jira = Some(v1::JiraAccount::default());
            }
            JiraMsg::OpenImport => {
                if self.jira.as_ref().is_some_and(|a| a.connected) {
                    self.open_import_jira(fx);
                } else {
                    self.open_connect_jira();
                }
            }
            JiraMsg::Import(id) => {
                if let Some(Dialog::ImportJira { error, .. }) = &mut self.dialog {
                    *error = None;
                }
                let body = Body::ImportJiraBoard(v1::ImportJiraBoardRequest { jira_board_id: id });
                fx.push(request(body, Followup::BoardCreated));
            }
            JiraMsg::Sync(board) => {
                let body = Body::SyncBoard(v1::SyncBoardRequest { board_id: board });
                fx.push(request(body, Followup::None));
            }
            JiraMsg::Open(url) => fx.push(Effect::OpenUrl(url)),
        }
    }

    /// Asks who the daemon is signed in to Jira as (when it can say).
    pub(super) fn request_jira_account(&self, fx: &mut Vec<Effect>) {
        if self.daemon_supports_jira() {
            fx.push(request(
                Body::GetJiraAccount(v1::GetJiraAccountRequest {}),
                Followup::JiraAccount,
            ));
        }
    }

    fn open_connect_jira(&mut self) {
        let account = self.jira.clone().unwrap_or_default();
        let site = account
            .site
            .strip_prefix("https://")
            .unwrap_or(&account.site)
            .to_owned();
        let focus = if site.is_empty() {
            Field::JiraSite
        } else {
            Field::JiraToken
        };
        self.open(
            Dialog::ConnectJira {
                site: TextField::new(site),
                email: TextField::new(account.email),
                token: TextField::secret(""),
                busy: false,
                error: None,
            },
            Some(focus),
        );
    }

    fn open_import_jira(&mut self, fx: &mut Vec<Effect>) {
        self.open(
            Dialog::ImportJira {
                query: TextField::default(),
                boards: None,
                error: None,
            },
            Some(Field::JiraQuery),
        );
        Self::list_jira_boards("", fx);
    }

    fn list_jira_boards(query: &str, fx: &mut Vec<Effect>) {
        let body = Body::ListJiraBoards(v1::ListJiraBoardsRequest {
            query: query.trim().to_owned(),
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
            Some(Dialog::ImportJira { query, boards, .. }) => {
                *boards = None;
                Self::list_jira_boards(query.text(), fx);
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
            (Followup::QuickFilter(_), _) => {
                self.board.quick_asking = false;
                self.board.filter.quick_matches = None;
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
            (Followup::JiraAccount, Outcome::JiraAccount(account)) => self.jira = Some(account),
            (Followup::JiraAccount, Outcome::Ok(_)) => {}
            (Followup::JiraConnected, Outcome::JiraAccount(account)) => {
                self.jira = Some(account);
                // Signed in: on to choosing a board.
                if matches!(self.dialog, Some(Dialog::ConnectJira { .. })) {
                    self.open_import_jira(fx);
                }
            }
            (Followup::JiraBoards, Outcome::JiraBoards(list)) => {
                if let Some(Dialog::ImportJira { boards, error, .. }) = &mut self.dialog {
                    *boards = Some(list.boards);
                    *error = None;
                }
            }
            (Followup::QuickFilter(board), Outcome::JiraIssueIds(ids)) => {
                self.quick_filter_answered(&board, Some(ids.ids), fx);
            }
            other => return Some(other),
        }
        None
    }
}
