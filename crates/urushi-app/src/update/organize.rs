//! Organizing threads: the sidebar's tabs and sections, the context menu, pins,
//! statuses, projects, prompts, and dragging threads between projects. Every change is a
//! request to the daemon (`UpdateThread`, `CreateFolder`, ...); the sidebar updates when
//! the daemon reports the new workspace.

use urushi_client::v1::{self, request::Body};
use urushi_ui::TextField;

use super::request;
use crate::msg::{Effect, Followup, Msg};
use crate::organize::{ContextMenu, DropTarget, MenuPage, Prompt};
use crate::state::{AppState, Dialog, Field};

impl AppState {
    /// Handles the organizing messages (see `route_organize`).
    pub(super) fn organize(&mut self, msg: Msg, fx: &mut Vec<Effect>) {
        match msg {
            Msg::SidebarTab(tab) => self.sidebar_tab = tab,
            Msg::ToggleSection(key) => {
                if !self.collapsed.remove(&key) {
                    self.collapsed.insert(key);
                }
            }
            Msg::Pointer(p) => self.last_press = Some(p),
            Msg::OpenMenu(target) => {
                if self.dialog.is_none() {
                    self.menu = Some(ContextMenu {
                        target,
                        page: MenuPage::Main,
                        at: self.last_press.unwrap_or_default(),
                    });
                }
            }
            Msg::MenuPage(page) => {
                if let Some(menu) = &mut self.menu {
                    menu.page = page;
                }
            }
            Msg::CloseMenu => self.menu = None,
            Msg::TogglePin(thread) => {
                let pinned = self.workspace.thread(&thread).is_some_and(|t| t.pinned);
                self.update_thread(thread, |r| r.pinned = Some(!pinned), fx);
            }
            Msg::SetStatus { thread, status } => {
                self.update_thread(thread, |r| r.status = Some(status.unwrap_or_default()), fx);
            }
            Msg::MoveThread { thread, project } => {
                self.update_thread(
                    thread,
                    |r| r.folder_id = Some(project.unwrap_or_default()),
                    fx,
                );
            }
            Msg::OpenPrompt(prompt) => self.open_prompt(prompt),
            Msg::DeleteProject(project) => {
                self.menu = None;
                fx.push(request(
                    Body::DeleteFolder(v1::DeleteFolderRequest { folder_id: project }),
                    Followup::None,
                ));
            }
            Msg::DragThread(thread) => {
                if self.dialog.is_none() {
                    self.menu = None;
                    self.dragging = Some((thread, self.last_press.unwrap_or_default()));
                }
            }
            Msg::DragMove(p) => {
                if let Some((_, at)) = &mut self.dragging {
                    *at = p;
                }
                if let Some((_, at)) = &mut self.board.dragging {
                    *at = p;
                }
            }
            Msg::DropThread(target) => {
                if let Some((thread, _)) = self.dragging.take() {
                    let project = match target {
                        DropTarget::Project(id) => Some(id),
                        DropTarget::Ungrouped => None,
                    };
                    let current = self.workspace.thread(&thread).map(|t| t.folder_id.clone());
                    if current != Some(project.clone().unwrap_or_default()) {
                        self.update_thread(
                            thread,
                            |r| r.folder_id = Some(project.unwrap_or_default()),
                            fx,
                        );
                    }
                }
            }
            Msg::DragCancel => {
                self.dragging = None;
                self.board.dragging = None;
                self.board.over = None;
            }
            Msg::ToggleSelect(which) => {
                if let Some(Dialog::NewThread { open, .. }) = &mut self.dialog {
                    *open = if *open == Some(which) {
                        None
                    } else {
                        Some(which)
                    };
                }
            }
            Msg::PickRepo(id) => self.pick_repo(id, fx),
            Msg::PickBase(base) => {
                if let Some(Dialog::NewThread { base: b, open, .. }) = &mut self.dialog {
                    *b = base;
                    *open = None;
                }
            }
            _ => {}
        }
    }

    /// Sends an `UpdateThread` for `thread` and closes the menu.
    fn update_thread(
        &mut self,
        thread: String,
        change: impl FnOnce(&mut v1::UpdateThreadRequest),
        fx: &mut Vec<Effect>,
    ) {
        self.menu = None;
        let mut r = v1::UpdateThreadRequest {
            thread_id: thread,
            ..Default::default()
        };
        change(&mut r);
        fx.push(request(Body::UpdateThread(r), Followup::None));
    }

    fn open_prompt(&mut self, prompt: Prompt) {
        self.menu = None;
        let initial = match &prompt {
            Prompt::RenameThread(id) => self.workspace.thread(id).map(|t| t.title.clone()),
            Prompt::RenameProject(id) => self.workspace.project(id).map(|p| p.name.clone()),
            Prompt::RenameBoard(id) => self.workspace.board(id).map(|b| b.name.clone()),
            Prompt::NewStatus(_) | Prompt::NewProject(_) | Prompt::NewBoard => None,
        };
        self.dialog = Some(Dialog::Prompt {
            prompt,
            field: TextField::new(initial.unwrap_or_default()),
        });
        self.focus = Some(Field::Prompt);
    }

    /// Enter in a prompt: send the request it stands for. Blank input does nothing.
    pub(super) fn submit_prompt(&mut self, fx: &mut Vec<Effect>) {
        let Some(Dialog::Prompt { prompt, field }) = &self.dialog else {
            return;
        };
        let text = field.text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let (body, followup) = match prompt.clone() {
            Prompt::RenameThread(thread_id) => (
                Body::UpdateThread(v1::UpdateThreadRequest {
                    thread_id,
                    title: Some(text),
                    ..Default::default()
                }),
                Followup::CloseDialog,
            ),
            Prompt::NewStatus(thread_id) => (
                Body::UpdateThread(v1::UpdateThreadRequest {
                    thread_id,
                    status: Some(text),
                    ..Default::default()
                }),
                Followup::CloseDialog,
            ),
            Prompt::NewProject(thread) => (
                Body::CreateFolder(v1::CreateFolderRequest { name: text }),
                Followup::ProjectCreated(thread),
            ),
            Prompt::RenameProject(folder_id) => (
                Body::RenameFolder(v1::RenameFolderRequest {
                    folder_id,
                    name: text,
                }),
                Followup::CloseDialog,
            ),
            board @ (Prompt::NewBoard | Prompt::RenameBoard(_)) => {
                let Some(request) = Self::board_prompt(&board, text) else {
                    return;
                };
                request
            }
        };
        fx.push(request(body, followup));
    }

    /// A project was created from a prompt: close it, and file the thread under it.
    pub(super) fn project_created(
        &mut self,
        thread: Option<String>,
        project: &v1::Folder,
        fx: &mut Vec<Effect>,
    ) {
        self.dialog = None;
        self.focus = None;
        self.collapsed
            .remove(&crate::organize::section_key(Some(&project.id), None));
        if let Some(thread) = thread {
            self.update_thread(thread, |r| r.folder_id = Some(project.id.clone()), fx);
        }
    }

    /// Opens the new-thread dialog in `repo` (default: the first repo), filed under
    /// `project`. Without any repo, asks to add one instead.
    pub(super) fn open_new_thread(
        &mut self,
        repo: Option<String>,
        project: Option<String>,
        fx: &mut Vec<Effect>,
    ) {
        self.menu = None;
        let Some(repo) = repo.or_else(|| self.workspace.repos().first().map(|r| r.id.clone()))
        else {
            self.sidebar_tab = crate::organize::SidebarTab::Repos;
            fx.extend(self.update(Msg::OpenAddProject));
            return;
        };
        self.dialog = Some(Dialog::NewThread {
            repo: repo.clone(),
            project,
            title: TextField::default(),
            branch: TextField::default(),
            branch_edited: false,
            base: None,
            open: None,
            task: None,
        });
        self.focus = Some(Field::ThreadTitle);
        Self::request_branches(repo, fx);
    }

    fn pick_repo(&mut self, id: String, fx: &mut Vec<Effect>) {
        if let Some(Dialog::NewThread {
            repo, base, open, ..
        }) = &mut self.dialog
        {
            if *repo != id {
                *base = None;
            }
            repo.clone_from(&id);
            *open = None;
            Self::request_branches(id, fx);
        }
    }

    fn request_branches(repo: String, fx: &mut Vec<Effect>) {
        fx.push(request(
            Body::ListBranches(v1::ListBranchesRequest {
                project_id: repo.clone(),
            }),
            Followup::Branches(repo),
        ));
    }

    /// Escape: close the menu or end a drag. Returns whether it did something.
    pub(super) fn dismiss_overlay(&mut self) -> bool {
        self.menu.take().is_some()
            || self.dragging.take().is_some()
            || self.board.dragging.take().is_some()
    }
}
