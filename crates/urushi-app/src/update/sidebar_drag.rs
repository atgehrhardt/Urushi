//! Dragging in the sidebar: a thread onto a project (to file it there) or onto another
//! thread (to take its place and join its group), and a project onto another project
//! (to take its place). Also Move up / Move down from the menus. Every change is a
//! request to the daemon (`UpdateThread`, `MoveThread`, `MoveFolder`); the drop lines
//! are view state.

use urushi_client::v1::{self, request::Body};

use super::request;
use crate::msg::{Effect, Followup, Msg};
use crate::organize::{Dragged, DropTarget, join_group, project_drop_index, thread_drop_index};
use crate::state::AppState;

impl AppState {
    /// Dragging threads between projects, and reordering projects and threads.
    pub(super) fn drag(&mut self, msg: Msg, fx: &mut Vec<Effect>) {
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
            Msg::DropOnThread(target) => {
                self.thread_over = None;
                if let Some((Dragged::Thread(thread), _)) = self.dragging.take() {
                    self.drop_on_thread(&thread, &target, fx);
                }
            }
            Msg::DropThread(target) => {
                self.thread_over = None;
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
                self.thread_over = None;
                self.board.dragging = None;
                self.board.over = None;
                self.board.tab_over = None;
            }
            _ => {}
        }
    }

    /// Starts dragging a thread or project from where the pointer was pressed.
    fn start_drag(&mut self, what: Dragged) {
        if self.dialog.is_none() {
            self.menu = None;
            self.project_over = None;
            self.thread_over = None;
            self.dragging = Some((what, self.last_press.unwrap_or_default()));
        }
    }

    /// Sends a `MoveFolder` and closes the menu.
    pub(super) fn move_project(&mut self, folder_id: String, index: u32, fx: &mut Vec<Effect>) {
        self.menu = None;
        fx.push(request(
            Body::MoveFolder(v1::MoveFolderRequest { folder_id, index }),
            Followup::None,
        ));
    }

    /// Sends a `MoveThread` and closes the menu.
    pub(super) fn move_thread(&mut self, thread_id: String, index: u32, fx: &mut Vec<Effect>) {
        self.menu = None;
        fx.push(request(
            Body::MoveThread(v1::MoveThreadRequest { thread_id, index }),
            Followup::None,
        ));
    }

    /// Thread `thread` was dropped on thread `target`: it joins the target's group
    /// (`UpdateThread`) and takes its place in the list (`MoveThread`), each only if
    /// something changes. The two don't depend on each other, as the list's order is
    /// independent of the groups.
    fn drop_on_thread(&mut self, thread: &str, target: &str, fx: &mut Vec<Effect>) {
        let (Some(dragged), Some(onto)) =
            (self.workspace.thread(thread), self.workspace.thread(target))
        else {
            return;
        };
        if dragged.id == onto.id || onto.project_base {
            return;
        }
        let update = join_group(&self.workspace, dragged, onto);
        let index = thread_drop_index(&self.workspace.raw().threads, thread, target);
        if let Some(update) = update {
            fx.push(request(Body::UpdateThread(update), Followup::None));
        }
        if let Some(index) = index.filter(|_| self.daemon_supports_thread_order()) {
            self.move_thread(thread.to_owned(), index, fx);
        }
    }

    /// While a thread is dragged: remember which thread's place it would take, for the
    /// drop line. Only drops that change something count.
    pub(super) fn thread_drag_over(&mut self, target: Option<Msg>) {
        let Some((Dragged::Thread(thread), _)) = &self.dragging else {
            return;
        };
        self.thread_over = match target {
            Some(Msg::DropOnThread(t)) if self.thread_drop_changes(thread, &t) => Some(t),
            _ => None,
        };
    }

    /// Whether dropping `thread` on `target` would do anything: any other thread but a
    /// parent-branch one moves it.
    fn thread_drop_changes(&self, thread: &str, target: &str) -> bool {
        thread != target
            && self
                .workspace
                .thread(target)
                .is_some_and(|t| !t.project_base)
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
}
