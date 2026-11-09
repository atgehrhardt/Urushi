//! Organizing threads: the sidebar's tabs and sections, the context menu, pins,
//! statuses, projects, prompts, dragging threads between projects, and reordering
//! projects. Every change is a
//! request to the daemon (`UpdateThread`, `CreateFolder`, ...); the sidebar updates when
//! the daemon reports the new workspace.

use urushi_client::v1::{self, request::Body};
use urushi_ui::TextField;

use super::request;
use crate::msg::{Effect, Followup, Msg};
use crate::organize::{ContextMenu, Dragged, DropTarget, MenuPage, Prompt, project_drop_index};
use crate::state::{AppState, Dialog, Field, ThreadTarget};

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
            Msg::OpenMenu(target) => self.open_menu(target),
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
            msg @ (Msg::DragThread(_)
            | Msg::DragProject(_)
            | Msg::DropProject(_)
            | Msg::DragMove(_)
            | Msg::DropThread(_)
            | Msg::DragCancel) => self.drag(msg, fx),
            Msg::MoveProject { project, index } => self.move_project(project, index, fx),
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
            Msg::PickProject(pick) => {
                if let Some(Dialog::NewThread { project, open, .. }) = &mut self.dialog {
                    *project = pick;
                    *open = None;
                }
            }
            _ => {}
        }
    }

    /// Dragging threads between projects and reordering projects.
    fn drag(&mut self, msg: Msg, fx: &mut Vec<Effect>) {
        match msg {
            Msg::DragThread(thread) => self.start_drag(Dragged::Thread(thread)),
            Msg::DragProject(project) => {
                if self.daemon_supports_project_order() {
                    self.start_drag(Dragged::Project(project));
                }
            }
            Msg::DropProject(target) => {
                self.project_over = None;
                if let Some((Dragged::Project(project), _)) = self.dragging.take()
                    && let Some(index) =
                        project_drop_index(self.workspace.projects(), &project, &target)
                {
                    self.move_project(project, index, fx);
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
                if let Some((Dragged::Thread(thread), _)) = self.dragging.take() {
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
                self.project_over = None;
                self.board.dragging = None;
                self.board.over = None;
            }
            _ => {}
        }
    }

    /// Starts dragging a thread or project from where the pointer was pressed.
    fn start_drag(&mut self, what: Dragged) {
        if self.dialog.is_none() {
            self.menu = None;
            self.project_over = None;
            self.dragging = Some((what, self.last_press.unwrap_or_default()));
        }
    }

    /// Sends a `MoveFolder` and closes the menu.
    fn move_project(&mut self, folder_id: String, index: u32, fx: &mut Vec<Effect>) {
        self.menu = None;
        fx.push(request(
            Body::MoveFolder(v1::MoveFolderRequest { folder_id, index }),
            Followup::None,
        ));
    }

    /// While a project is dragged: remember which project's place it would take, for the
    /// drop line. Only drops that move it count.
    pub(super) fn project_drag_over(&mut self, target: Option<Msg>) {
        let Some((Dragged::Project(project), _)) = &self.dragging else {
            return;
        };
        self.project_over = match target {
            Some(Msg::DropProject(t))
                if project_drop_index(self.workspace.projects(), project, &t).is_some() =>
            {
                Some(t)
            }
            _ => None,
        };
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

    /// Opens a context menu where the pointer was pressed.
    pub(super) fn open_menu(&mut self, target: crate::organize::MenuTarget) {
        // Menus don't open over dialogs, except a subtask's over its task's
        // editor.
        let over_editor = matches!(
            (&self.dialog, &target),
            (
                Some(Dialog::Task { .. }),
                crate::organize::MenuTarget::Task(_)
            )
        );
        if self.dialog.is_none() || over_editor {
            self.menu = Some(ContextMenu {
                target,
                page: MenuPage::Main,
                at: self.last_press.unwrap_or_default(),
            });
        }
    }

    fn open_prompt(&mut self, prompt: Prompt) {
        self.menu = None;
        let initial = match &prompt {
            Prompt::RenameThread(id) => self.workspace.thread(id).map(|t| t.title.clone()),
            Prompt::RenameProject(id) => self.workspace.project(id).map(|p| p.name.clone()),
            Prompt::RenameBoard(id) => self.workspace.board(id).map(|b| b.name.clone()),
            Prompt::RenameColumn(id) => self.workspace.column(id).map(|(_, c)| c.name.clone()),
            Prompt::NewStatus(_)
            | Prompt::NewProject(_)
            | Prompt::NewBoard
            | Prompt::NewColumn(_) => None,
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
            board @ (Prompt::NewBoard
            | Prompt::RenameBoard(_)
            | Prompt::NewColumn(_)
            | Prompt::RenameColumn(_)) => {
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
            target: ThreadTarget::New,
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
        let dismissed = self.menu.take().is_some()
            || self.dragging.take().is_some()
            || self.board.dragging.take().is_some();
        if self.dragging.is_none() {
            self.project_over = None;
        }
        dismissed
    }
}
