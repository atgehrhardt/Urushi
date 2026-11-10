//! Client-side types for organizing the sidebar: tabs, collapsed sections, the context
//! menu, text prompts, dragging threads and projects, and the new-thread selectors. The organization
//! itself (projects, pins, statuses) lives in the daemon; these only describe what the
//! UI is doing with it.

use urushi_client::v1;
use urushi_ui::Point;

/// First protocol minor version that can reorder projects (`MoveFolder`).
pub const PROJECT_ORDER_SINCE_MINOR: u32 = 11;
/// First protocol minor version with project settings: a parent branch and its
/// parent-branch thread (`UpdateFolderSettings`, ADR 0029).
pub const PROJECT_SETTINGS_SINCE_MINOR: u32 = 18;

/// Which list the sidebar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarTab {
    /// Projects, then ungrouped threads.
    #[default]
    Projects,
    /// Repos (git repositories).
    Repos,
}

/// What a context menu is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuTarget {
    /// A thread (by id).
    Thread(String),
    /// A project (by id).
    Project(String),
    /// A text field in a dialog: cut, copy, paste, select all.
    Field(crate::state::Field),
    /// A task card on the board (by task id).
    Task(String),
    /// A column of a board (by column id).
    Column(String),
    /// A board's tab (by board id): move it left or right.
    Board(String),
    /// The board's filters (by board id).
    Filter(String),
    /// A quick filter's chip (board id, filter): change or delete it.
    QuickFilter(String, crate::filter::quick::QuickRef),
    /// The board panel's "+": a new board, or one imported from Jira.
    NewBoard,
    /// A terminal pane (by pane id): split, copy, paste, close.
    Pane(String),
}

/// Which page of the context menu is showing. Sub-pages replace the main page, which
/// keeps menus simple to draw and to use on touch screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuPage {
    /// The first page (for a thread: rename, pin, status ›, move ›, delete).
    Main,
    /// Choose a status.
    Status,
    /// Choose a project (for a thread) or a column (for a task).
    Move,
    /// Choose a color (for a task or a column).
    Color,
    /// Choose values to filter a board by.
    Filter(crate::filter::Dim),
}

/// An open context menu.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenu {
    /// What it acts on.
    pub target: MenuTarget,
    /// Which page shows.
    pub page: MenuPage,
    /// Where it opened (logical pixels).
    pub at: Point,
}

/// What is being dragged with a label following the pointer: a thread or project in the
/// sidebar, or a board's tab. (Task cards are dragged as `BoardPanel::dragging`.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dragged {
    /// A thread (by id), to file it under another project.
    Thread(String),
    /// A project (by id), to change the order of projects.
    Project(String),
    /// A board (by id), to change the order of the board tabs.
    Board(String),
}

/// Where item `dragged` goes when it is dropped on item `target`, in a list of `ids`: it
/// takes the target's place, so it lands after a target further on and before one
/// further back. The index counts positions without the dragged item, as `MoveFolder`
/// and `MoveBoard` do. `None` when nothing would move or either item is unknown.
fn drop_index<'a>(
    mut ids: impl Iterator<Item = &'a str> + Clone,
    dragged: &str,
    target: &str,
) -> Option<u32> {
    let from = ids.clone().position(|id| id == dragged)?;
    let to = ids.position(|id| id == target)?;
    if from == to {
        return None;
    }
    u32::try_from(to).ok()
}

/// Where project `dragged` goes when dropped on project `target` (see [`drop_index`]):
/// below a target further down, above one further up.
pub fn project_drop_index(projects: &[v1::Folder], dragged: &str, target: &str) -> Option<u32> {
    drop_index(projects.iter().map(|p| p.id.as_str()), dragged, target)
}

/// Where board `dragged` goes when dropped on board `target`'s tab (see
/// [`drop_index`]): right of a tab further right, left of one further left.
pub fn board_drop_index(boards: &[v1::Board], dragged: &str, target: &str) -> Option<u32> {
    drop_index(boards.iter().map(|b| b.id.as_str()), dragged, target)
}

/// Where a dragged thread can be dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropTarget {
    /// File it under this project.
    Project(String),
    /// Take it out of any project.
    Ungrouped,
}

/// What a one-line text prompt is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// Rename a thread.
    RenameThread(String),
    /// Give a thread a new status.
    NewStatus(String),
    /// Create a project, then move this thread into it if given.
    NewProject(Option<String>),
    /// Rename a project.
    RenameProject(String),
    /// Create a task board.
    NewBoard,
    /// Rename a task board.
    RenameBoard(String),
    /// Add a column at the end of a board.
    NewColumn(String),
    /// Rename a column.
    RenameColumn(String),
}

impl Prompt {
    /// Dialog heading, placeholder and button label.
    pub fn texts(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::RenameThread(_) => ("Rename thread", "Thread name", "Rename"),
            Self::NewStatus(_) => ("New status", "e.g. In review", "Set status"),
            Self::NewProject(_) => ("New project", "e.g. Billing launch", "Create"),
            Self::RenameProject(_) => ("Rename project", "Project name", "Rename"),
            Self::NewBoard => ("New board", "e.g. Sprint 12", "Create"),
            Self::RenameBoard(_) => ("Rename board", "Board name", "Rename"),
            Self::NewColumn(_) => ("New column", "e.g. In review", "Add column"),
            Self::RenameColumn(_) => ("Rename column", "Column name", "Rename"),
        }
    }
}

/// A selector in the new-thread or project-settings dialog that can be open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Select {
    /// The repo to create the thread in.
    Repo,
    /// The branch to start from (or, for a project, its parent branch).
    Base,
    /// The project to file the thread under.
    Project,
}

/// Key of a collapsible sidebar section. Collapsed sections are remembered per client.
pub fn section_key(project: Option<&str>, status: Option<&str>) -> String {
    format!(
        "{}/{}",
        project.unwrap_or("-"),
        status.map_or_else(String::new, |s| format!("s:{s}"))
    )
}
