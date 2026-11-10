//! Client-side state of the task-board panel: whether it's open (and how far it has
//! slid out), which board is shown, and the task being pressed or dragged (and where it
//! would land). The boards
//! themselves live in the daemon and arrive with the workspace (`Workspace.boards`).

use urushi_client::{TaskRef, WorkspaceMirror, v1};
use urushi_ui::Point;

/// First protocol minor version with task boards and agent usage.
pub const BOARDS_SINCE_MINOR: u32 = 5;
/// First protocol minor version with subtasks and threads started from tasks.
pub const SUBTASKS_SINCE_MINOR: u32 = 6;
/// First protocol minor version with colors, tags and changeable columns.
pub const BOARD_EXTRAS_SINCE_MINOR: u32 = 8;
/// First protocol minor version with Jira boards and task kinds.
pub const JIRA_SINCE_MINOR: u32 = 12;

/// How long the panel takes to slide fully in or out, in seconds.
pub const SLIDE_SECONDS: f32 = 0.18;

/// The board panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardPanel {
    /// Open (or opening).
    pub open: bool,
    /// How far it has slid out: 0 = hidden, 1 = fully shown. The runner animates this
    /// towards `open`.
    pub reveal: f32,
    /// The board shown; `None` (or a deleted one) shows the first.
    pub selected: Option<String>,
    /// A task pressed but not yet released or dragged: releasing opens it.
    pub pressed: Option<String>,
    /// The task being dragged, and where the pointer is.
    pub dragging: Option<(String, Point)>,
    /// While dragging: where the task would land if dropped now, as `(column, before)`
    /// (see [`drop_position`]); `None` when it wouldn't move.
    pub over: Option<(String, Option<String>)>,
    /// Show only the tasks with this tag (compared ignoring case); `None` shows all.
    pub tag_filter: Option<String>,
    /// On a Jira board, show only the tasks in this epic (by key); `None` shows all.
    pub epic_filter: Option<String>,
    /// Image attachments' thumbnails, fetched from the daemon as they come into view.
    pub thumbnails: Thumbnails,
}

/// Most thumbnails kept at once; ones out of view go first.
pub const MAX_THUMBNAILS: usize = 64;

/// Thumbnails of image attachments, by attachment id. The daemon decodes images and
/// sends raw RGBA (ADR 0016); each is asked for once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Thumbnails {
    /// The ones received.
    pub images: std::collections::HashMap<String, std::sync::Arc<urushi_ui::ImageData>>,
    /// Every one asked for (received, failed or on its way), so none is asked twice.
    pub requested: std::collections::HashSet<String>,
}

impl Thumbnails {
    /// The thumbnail of an attachment, if it has arrived.
    pub fn get(&self, id: &str) -> Option<&std::sync::Arc<urushi_ui::ImageData>> {
        self.images.get(id)
    }
}

/// The image attachments whose thumbnails are on screen: the first image of each card
/// on the shown board, and every image of the task open in the editor.
pub fn wanted_thumbnails(
    workspace: &WorkspaceMirror,
    board: Option<&v1::Board>,
    editing: Option<&str>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for card in board.iter().flat_map(|b| &b.columns).flat_map(|c| &c.tasks) {
        if let Some(a) = card.attachments.iter().find(|a| a.width > 0) {
            out.push(a.id.clone());
        }
    }
    if let Some(found) = editing.and_then(|t| workspace.find_task(t)) {
        for a in found.task().attachments().iter().filter(|a| a.width > 0) {
            if !out.contains(&a.id) {
                out.push(a.id.clone());
            }
        }
    }
    out
}

/// The color, tags and kind part of the task editor, kept as drafts and sent with Save.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskLabels {
    /// The chosen color, as a wire `LabelColor` value (0: none).
    pub color: i32,
    /// The chosen kind, as a wire `TaskKind` value (0: none).
    pub kind: i32,
    /// The tags, in order.
    pub tags: Vec<String>,
    /// The "add a tag" field.
    pub input: urushi_ui::TextField,
}

impl TaskLabels {
    /// Adds `tag` unless it is blank or already there (ignoring case).
    pub fn add(&mut self, tag: &str) {
        let tag = tag.split_whitespace().collect::<Vec<_>>().join(" ");
        if !tag.is_empty() && !self.tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            self.tags.push(tag);
        }
    }

    /// The tags, plus whatever is typed in the field but not yet added.
    pub fn with_typed(&self) -> Vec<String> {
        let mut all = self.clone();
        all.add(self.input.text());
        all.tags
    }
}

/// Where "Make subtask of…" can put `task`: every other task on its board, at any
/// depth, except the task itself, its own subtasks and its current parent, whose title
/// (or path) contains `query` (ignoring case). Each comes as `(id, path)`, the path
/// reading "Card › Subtask".
pub fn nest_candidates(
    workspace: &WorkspaceMirror,
    task: &str,
    query: &str,
) -> Vec<(String, String)> {
    fn walk(t: TaskRef<'_>, above: &str, skip: &[&str], out: &mut Vec<(String, String)>) {
        if skip.contains(&t.id()) {
            return;
        }
        let path = if above.is_empty() {
            t.title().to_owned()
        } else {
            format!("{above} › {}", t.title())
        };
        for s in t.subtasks() {
            walk(TaskRef::Sub(s), &path, skip, out);
        }
        out.push((t.id().to_owned(), path));
    }
    let Some(found) = workspace.find_task(task) else {
        return Vec::new();
    };
    let skip = found.task().subtree_ids();
    let parent = found.parent().map(TaskRef::id);
    let mut out = Vec::new();
    for card in found.board.columns.iter().flat_map(|c| &c.tasks) {
        walk(TaskRef::Card(card), "", &skip, &mut out);
    }
    let query = query.trim().to_lowercase();
    out.retain(|(id, path)| Some(id.as_str()) != parent && path.to_lowercase().contains(&query));
    // Cards before their subtasks, in board order.
    out.sort_by_key(|(_, path)| path.matches(" › ").count());
    out
}

/// Whether `task` passes the tag filter.
pub fn shows(filter: Option<&str>, task: &v1::Task) -> bool {
    filter.is_none_or(|f| task.tags.iter().any(|t| t.eq_ignore_ascii_case(f)))
}

/// Whether `task` passes the panel's filters: its tag and its epic.
pub fn passes(panel: &BoardPanel, task: &v1::Task) -> bool {
    shows(panel.tag_filter.as_deref(), task)
        && panel
            .epic_filter
            .as_deref()
            .is_none_or(|e| task.issue.as_ref().is_some_and(|i| i.epic_key == e))
}

/// The epics a Jira board's cards belong to, in the board's order.
pub fn board_epics(board: &v1::Board) -> Vec<&v1::Epic> {
    board
        .epics
        .iter()
        .filter(|e| {
            board
                .columns
                .iter()
                .flat_map(|c| &c.tasks)
                .any(|t| t.issue.as_ref().is_some_and(|i| i.epic_key == e.key))
        })
        .collect()
}

/// Every tag used on a board, in first-seen order (ignoring case), for suggestions and
/// the filter.
pub fn board_tags(board: &v1::Board) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in board
        .columns
        .iter()
        .flat_map(|c| &c.tasks)
        .flat_map(|t| &t.tags)
    {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(tag)) {
            out.push(tag.clone());
        }
    }
    out
}

impl BoardPanel {
    /// Whether any of the panel is on screen (it covers the terminals).
    pub fn visible(&self) -> bool {
        self.open || self.reveal > 0.0
    }

    /// The board to show.
    pub fn board<'a>(&self, workspace: &'a WorkspaceMirror) -> Option<&'a v1::Board> {
        self.selected
            .as_deref()
            .and_then(|id| workspace.board(id))
            .or_else(|| workspace.boards().first())
    }

    /// `reveal` with an ease-out curve, for drawing.
    pub fn eased(&self) -> f32 {
        let t = self.reveal.clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
    }
}

/// Where a dropped task goes: `(column, index)` for `MoveTask`, or `None` if it would
/// land where it already is. `before` is the task it was dropped on (it goes above
/// it); `None` means the bottom of the column.
pub fn drop_position(
    board: &v1::Board,
    task: &str,
    column: &str,
    before: Option<&str>,
) -> Option<(String, u32)> {
    let target = board.columns.iter().find(|c| c.id == column)?;
    let (from_column, from_index) = board.columns.iter().find_map(|c| {
        c.tasks
            .iter()
            .position(|t| t.id == task)
            .map(|i| (c.id.as_str(), i))
    })?;
    // Positions are counted without the dragged task (that's how `MoveTask` reads them).
    let others: Vec<&str> = target
        .tasks
        .iter()
        .map(|t| t.id.as_str())
        .filter(|id| *id != task)
        .collect();
    let index = match before {
        Some(b) if b == task => return None,
        Some(b) => others.iter().position(|id| *id == b)?,
        None => others.len(),
    };
    if from_column == column && from_index == index {
        return None;
    }
    Some((column.to_owned(), u32::try_from(index).unwrap_or(u32::MAX)))
}

/// Tasks not in a board's last column ("Done"): what's still open, across all boards.
pub fn open_tasks(workspace: &WorkspaceMirror) -> usize {
    workspace
        .boards()
        .iter()
        .map(|b| {
            let n = b.columns.len().saturating_sub(1);
            b.columns
                .iter()
                .take(n)
                .map(|c| c.tasks.len())
                .sum::<usize>()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> v1::Board {
        let task = |id: &str| v1::Task {
            id: id.into(),
            title: id.into(),
            ..Default::default()
        };
        let column = |id: &str, tasks: &[&str]| v1::BoardColumn {
            id: id.into(),
            name: id.into(),
            tasks: tasks.iter().map(|t| task(t)).collect(),
            ..Default::default()
        };
        v1::Board {
            id: "b".into(),
            name: "B".into(),
            created_at_ms: 0,
            columns: vec![column("todo", &["a", "b", "c"]), column("done", &["d"])],
            ..Default::default()
        }
    }

    #[test]
    fn drops_land_above_the_target_or_at_the_bottom() {
        let b = board();
        // Onto "a": to the top of its own column.
        assert_eq!(
            drop_position(&b, "c", "todo", Some("a")),
            Some(("todo".into(), 0))
        );
        // Moving down past "c": counted without "a", "c" is at 1.
        assert_eq!(
            drop_position(&b, "a", "todo", Some("c")),
            Some(("todo".into(), 1))
        );
        // Into another column, at the bottom.
        assert_eq!(
            drop_position(&b, "a", "done", None),
            Some(("done".into(), 1))
        );
        // Nowhere new.
        assert_eq!(
            drop_position(&b, "a", "todo", Some("b")),
            None,
            "already above b"
        );
        assert_eq!(drop_position(&b, "c", "todo", None), None, "already last");
        assert_eq!(
            drop_position(&b, "a", "todo", Some("a")),
            None,
            "onto itself"
        );
        assert_eq!(drop_position(&b, "zz", "todo", None), None, "unknown task");
    }

    #[test]
    fn open_tasks_leave_out_the_last_column() {
        let workspace = WorkspaceMirror::new(v1::Workspace {
            boards: vec![board()],
            ..Default::default()
        });
        assert_eq!(open_tasks(&workspace), 3);
    }

    #[test]
    fn the_first_board_shows_unless_another_is_chosen() {
        let workspace = WorkspaceMirror::new(v1::Workspace {
            boards: vec![board()],
            ..Default::default()
        });
        let mut panel = BoardPanel::default();
        assert_eq!(panel.board(&workspace).map(|b| b.id.as_str()), Some("b"));
        panel.selected = Some("deleted".into());
        assert_eq!(panel.board(&workspace).map(|b| b.id.as_str()), Some("b"));
        assert!(!panel.visible());
        panel.reveal = 0.5;
        assert!(panel.visible());
        assert!(panel.eased() > 0.5);
    }
}

#[cfg(test)]
mod nest_tests {
    use super::*;

    #[test]
    fn a_task_can_go_under_any_other_task_but_its_own_subtree() {
        let sub = |id: &str, subtasks: Vec<v1::Subtask>| v1::Subtask {
            id: id.into(),
            title: id.to_uppercase(),
            subtasks,
            ..Default::default()
        };
        let card = |id: &str, subtasks: Vec<v1::Subtask>| v1::Task {
            id: id.into(),
            title: id.to_uppercase(),
            subtasks,
            ..Default::default()
        };
        let w = WorkspaceMirror::new(v1::Workspace {
            boards: vec![v1::Board {
                id: "b".into(),
                columns: vec![v1::BoardColumn {
                    id: "c".into(),
                    tasks: vec![
                        card("a", vec![sub("a1", vec![sub("a2", vec![])])]),
                        card("x", vec![sub("x1", vec![])]),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        });
        let ids = |task: &str, q: &str| -> Vec<String> {
            nest_candidates(&w, task, q)
                .into_iter()
                .map(|(id, _)| id)
                .collect()
        };
        // Not itself, not under itself.
        assert_eq!(ids("a", ""), ["x", "x1"]);
        // Not its current parent; the path names where each one is.
        assert_eq!(ids("x1", ""), ["a", "a1", "a2"]);
        assert_eq!(nest_candidates(&w, "x1", "a2")[0].1, "A › A1 › A2");
        assert_eq!(ids("a1", "x"), ["x", "x1"]);
    }
}
