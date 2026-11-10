//! Saving, changing and deleting quick filters (protocol 1.23, ADR 0037).
//!
//! "Save as quick filter…" opens a dialog holding what is chosen on the board: on a Jira
//! board as JQL (`filter/jql.rs`) to read and change, elsewhere as a search, kinds and
//! tags. The daemon writes a Jira board's to Jira, or keeps one Jira won't take and
//! says why (shown in the banner). Once saved, the board shows just that filter, on.
//! A chip's menu changes or deletes it; deleting asks first. Failures show in the
//! dialog.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::filter::editor::QuickFilterEditor;
use crate::filter::jql::selection_jql;
use crate::filter::quick::{QuickRef, rule_from_selection};
use crate::msg::{Effect, FilterMsg, Followup};
use crate::state::{AppState, Dialog, Field};

impl AppState {
    /// A message about saving, changing or deleting quick filters.
    pub(super) fn quick_filter_msg(&mut self, msg: FilterMsg, fx: &mut Vec<Effect>) {
        match msg {
            FilterMsg::OpenSave => self.open_save_quick_filter(),
            FilterMsg::OpenEdit(filter) => self.open_edit_quick_filter(filter),
            FilterMsg::UseCurrent => {
                let rule = self
                    .board
                    .board(&self.workspace)
                    .and_then(|b| rule_from_selection(&self.board.filter, b));
                if let (Some(Dialog::QuickFilter(e)), Some(rule)) = (&mut self.dialog, rule) {
                    e.rule = Some(rule);
                }
            }
            FilterMsg::OpenDelete(filter) => self.open_delete_quick_filter(filter),
            FilterMsg::ConfirmDelete => self.confirm_delete_quick_filter(fx),
            _ => {}
        }
    }

    /// Whether what is chosen on `board` can be saved as a quick filter.
    pub(crate) fn can_save_quick_filter(&self, board: &v1::Board) -> bool {
        self.daemon_supports_quick_filters()
            && if board.jira.is_some() {
                !selection_jql(&self.board.filter, board).is_empty()
            } else {
                rule_from_selection(&self.board.filter, board).is_some()
            }
    }

    fn open_save_quick_filter(&mut self) {
        self.menu = None;
        let Some(board) = self.board.board(&self.workspace) else {
            return;
        };
        if !self.can_save_quick_filter(board) {
            return;
        }
        let editor = QuickFilterEditor {
            board: board.id.clone(),
            jql: board
                .jira
                .is_some()
                .then(|| urushi_ui::TextField::multiline(selection_jql(&self.board.filter, board))),
            rule: rule_from_selection(&self.board.filter, board).filter(|_| board.jira.is_none()),
            ..QuickFilterEditor::default()
        };
        self.open(
            Dialog::QuickFilter(Box::new(editor)),
            Some(Field::QuickFilterName),
        );
    }

    fn open_edit_quick_filter(&mut self, filter: QuickRef) {
        self.menu = None;
        let Some(board) = self.board.board(&self.workspace) else {
            return;
        };
        let Some(name) = filter.name(board) else {
            return;
        };
        let (jql, current) = match &filter {
            QuickRef::Jira(id) => (
                board
                    .jira
                    .as_deref()
                    .and_then(|j| j.quick_filters.iter().find(|q| q.id == *id))
                    .map(|q| q.jql.clone()),
                None,
            ),
            QuickRef::Kept(id) => {
                let rule = board
                    .quick_filters
                    .iter()
                    .find(|q| q.id == *id)
                    .and_then(|q| q.rule.clone())
                    .unwrap_or_default();
                if board.jira.is_some() {
                    (Some(rule.jql), None)
                } else {
                    (None, Some(rule))
                }
            }
        };
        let editor = QuickFilterEditor {
            board: board.id.clone(),
            name: urushi_ui::TextField::new(name),
            jql: jql.map(urushi_ui::TextField::multiline),
            current,
            editing: Some(filter),
            ..QuickFilterEditor::default()
        };
        self.open(
            Dialog::QuickFilter(Box::new(editor)),
            Some(Field::QuickFilterName),
        );
    }

    /// Save in the quick-filter dialog: says what's missing, or sends it.
    pub(super) fn submit_quick_filter(&mut self, fx: &mut Vec<Effect>) {
        let Some(Dialog::QuickFilter(e)) = &mut self.dialog else {
            return;
        };
        if e.busy {
            return;
        }
        let name = e.name.text().trim().to_owned();
        let rule = e.rule_to_send();
        let missing = if name.is_empty() {
            Some("Give the quick filter a name.")
        } else if rule.as_ref().is_some_and(|r| r.jql.is_empty()) && e.jql.is_some() {
            Some("Write the JQL it filters by.")
        } else if rule.is_none() && e.editing.is_none() {
            Some("Choose what it filters by on the board first.")
        } else {
            None
        };
        if let Some(missing) = missing {
            e.error = Some(missing.to_owned());
            return;
        }
        e.busy = true;
        e.error = None;
        let board = e.board.clone();
        let (body, followup) = match &e.editing {
            None => (
                Body::CreateQuickFilter(v1::CreateQuickFilterRequest {
                    board_id: board.clone(),
                    name,
                    rule,
                }),
                Followup::QuickFilterSaved(board),
            ),
            Some(filter) => (
                Body::UpdateQuickFilter(v1::UpdateQuickFilterRequest {
                    board_id: board,
                    filter: Some(filter.to_proto()),
                    name,
                    rule,
                }),
                Followup::QuickFilterChanged,
            ),
        };
        fx.push(request(body, followup));
    }

    /// A quick-filter request failed: the dialog says why. Returns whether it was one.
    pub(super) fn quick_filter_failed(&mut self, followup: &Followup, message: &str) -> bool {
        if !matches!(
            followup,
            Followup::QuickFilterSaved(_) | Followup::QuickFilterChanged
        ) {
            return false;
        }
        match &mut self.dialog {
            Some(Dialog::QuickFilter(e)) => {
                e.busy = false;
                e.error = Some(message.to_owned());
                true
            }
            _ => false,
        }
    }

    /// A new quick filter was saved: the board shows just it, on.
    pub(super) fn quick_filter_saved(
        &mut self,
        board: &str,
        saved: v1::QuickFilterSaved,
        fx: &mut Vec<Effect>,
    ) {
        if matches!(self.dialog, Some(Dialog::QuickFilter(_))) {
            self.dialog = None;
            self.focus = None;
        }
        if !saved.kept_here.is_empty() {
            self.error = Some(format!(
                "Jira didn't take the quick filter ({}), so Urushi keeps it on this board. \
                 Jira still evaluates its JQL.",
                saved.kept_here
            ));
        }
        let shown = self.board.board(&self.workspace).map(|b| b.id.as_str());
        if shown != Some(board) {
            return;
        }
        let filter = match saved.filter.and_then(|f| f.filter) {
            Some(v1::quick_filter_ref::Filter::JiraId(id)) => QuickRef::Jira(id),
            Some(v1::quick_filter_ref::Filter::Id(id)) => QuickRef::Kept(id),
            None => return,
        };
        let f = &mut self.board.filter;
        f.clear();
        match filter {
            QuickRef::Jira(id) => {
                f.quick.insert(id);
            }
            QuickRef::Kept(id) => {
                f.saved.insert(id);
            }
        }
        self.board.quick_asking = false;
        self.refresh_quick_filter(fx);
    }

    /// A quick filter was changed: close the dialog, and ask Jira again if its JQL may
    /// have changed while it is on.
    pub(super) fn quick_filter_changed(&mut self, fx: &mut Vec<Effect>) {
        let edited = match &self.dialog {
            Some(Dialog::QuickFilter(e)) => e.editing.clone(),
            _ => None,
        };
        if edited.is_some() {
            self.dialog = None;
            self.focus = None;
        }
        if edited.is_some_and(|f| f.is_on(&self.board.filter)) {
            self.board.filter.quick_matches = None;
            self.board.quick_asking = false;
            self.refresh_quick_filter(fx);
        }
    }

    fn open_delete_quick_filter(&mut self, filter: QuickRef) {
        self.menu = None;
        let Some(board) = self.board.board(&self.workspace) else {
            return;
        };
        let Some(name) = filter.name(board).map(str::to_owned) else {
            return;
        };
        let board = board.id.clone();
        self.open(
            Dialog::DeleteQuickFilter {
                board,
                filter,
                name,
            },
            None,
        );
    }

    fn confirm_delete_quick_filter(&mut self, fx: &mut Vec<Effect>) {
        let Some(Dialog::DeleteQuickFilter { board, filter, .. }) = &self.dialog else {
            return;
        };
        let body = Body::DeleteQuickFilter(v1::DeleteQuickFilterRequest {
            board_id: board.clone(),
            filter: Some(filter.to_proto()),
        });
        let filter = filter.clone();
        fx.push(request(body, Followup::CloseDialog));
        // It goes: stop filtering by it.
        if filter.is_on(&self.board.filter) {
            match &filter {
                QuickRef::Jira(id) => {
                    self.board.filter.quick.remove(id);
                }
                QuickRef::Kept(id) => {
                    self.board.filter.saved.remove(id);
                }
            }
            self.board.filter.quick_matches = None;
            self.board.quick_asking = false;
            self.refresh_quick_filter(fx);
        }
    }
}
