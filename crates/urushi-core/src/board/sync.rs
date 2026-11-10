//! Merging what was read from Jira into a board that follows it (ADR 0020).
//!
//! The merge is a pure function of the board and a snapshot of issues, so the daemon's
//! sync can be tested without Jira and the same snapshot always gives the same board:
//! - Tasks are matched to issues by the issue's Jira id. A matched task keeps its own
//!   id, thread link and color; Jira's fields replace the rest.
//! - Every task is placed again from its issue: a card in the column showing its
//!   status, ordered by the board's rank; a Jira subtask under its parent, checked off
//!   when its status is in the done category. Epics are cards too (ADR 0027), and are
//!   also the board's [`Epic`]s, which their stories name. Issues whose status no column
//!   shows leave the board, as they do in Jira.
//! - A *full* snapshot is everything on the board, so issues missing from it are
//!   removed. A partial one (recently updated issues) leaves the others as they are.
//! - The board stays within [`MAX_SYNCED_ISSUES`] tasks and [`super::MAX_SUBTASKS`]
//!   subtasks per task; what didn't fit is counted.

use std::collections::{HashMap, HashSet};

use super::jira::{Epic, Issue, JiraBoard, JiraColumn, StatusCategory, SyncedIssue};
use super::labels::tag_color;
use super::{
    Board, BoardError, BoardSource, Column, MAX_COLUMNS, MAX_NAME, MAX_NOTES, MAX_SUBTASKS,
    MAX_TAG, MAX_TAGS, MAX_TITLE, Task, board_name,
};
use crate::{BoardId, ColumnId, TaskId, Timestamp};

/// Most tasks (cards and subtasks) a Jira board keeps.
pub const MAX_SYNCED_ISSUES: usize = 500;

/// What a merge did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// Whether the board changed.
    pub changed: bool,
    /// Issues left out because the board or a task was full.
    pub dropped: u32,
}

impl Board {
    /// A board following `jira`, with its columns and no tasks yet.
    pub fn new_jira(
        id: BoardId,
        jira: JiraBoard,
        columns: &[JiraColumn],
        created_at: Timestamp,
        new_column_id: impl FnMut() -> ColumnId,
    ) -> Result<Self, BoardError> {
        let mut board = Self {
            id,
            name: board_name(&clip(&jira.name, MAX_NAME)).unwrap_or_else(|_| "Jira".into()),
            created_at,
            columns: Vec::new(),
            source: Some(BoardSource::Jira(jira)),
            epics: Vec::new(),
        };
        board.set_jira_columns(columns, new_column_id)?;
        Ok(board)
    }

    /// Replaces the columns with the Jira board's (the first [`MAX_COLUMNS`]). A column
    /// with the same name as before keeps its id and color. Tasks in columns that went
    /// away wait in the first column until the next merge places them.
    pub fn set_jira_columns(
        &mut self,
        columns: &[JiraColumn],
        mut new_id: impl FnMut() -> ColumnId,
    ) -> Result<(), BoardError> {
        if columns.is_empty() {
            return Err(BoardError::LastColumn);
        }
        let mut old = std::mem::take(&mut self.columns);
        for c in columns.iter().take(MAX_COLUMNS) {
            let name = board_name(&clip(&c.name, MAX_NAME)).unwrap_or_else(|_| "Column".into());
            let column = match old.iter().position(|o| o.name == name) {
                Some(at) => {
                    let mut column = old.remove(at);
                    column.statuses.clone_from(&c.statuses);
                    column
                }
                None => Column {
                    id: new_id(),
                    name,
                    color: None,
                    tasks: Vec::new(),
                    statuses: c.statuses.clone(),
                },
            };
            self.columns.push(column);
        }
        let stranded = old.into_iter().flat_map(|c| c.tasks);
        if let Some(first) = self.columns.first_mut() {
            first.tasks.extend(stranded);
        }
        Ok(())
    }

    /// Merges issues read from Jira into the board (see the module docs). `full` says
    /// the snapshot is everything on the Jira board. New tasks get ids from `new_id`.
    pub fn merge_issues(
        &mut self,
        snapshot: Vec<SyncedIssue>,
        full: bool,
        mut new_id: impl FnMut() -> TaskId,
    ) -> MergeReport {
        let before = self.clone();

        // Every task, flattened, by issue id; tasks without an issue stay where they are.
        let mut records: HashMap<String, Task> = HashMap::new();
        for column in &mut self.columns {
            let tasks = std::mem::take(&mut column.tasks);
            for task in tasks {
                if task.issue.is_some() {
                    flatten(task, &mut records);
                } else {
                    column.tasks.push(task);
                }
            }
        }

        if full {
            let in_snapshot: HashSet<&str> = snapshot.iter().map(|s| s.issue.id.as_str()).collect();
            records.retain(|id, _| in_snapshot.contains(id.as_str()));
        }
        for synced in snapshot {
            if synced.kind == super::TaskKind::Epic {
                upsert_epic(&mut self.epics, &synced.issue.key, &synced.title);
            }
            if let (Some(key), Some(summary)) = (&synced.issue.epic, &synced.epic_summary) {
                upsert_epic(&mut self.epics, key, summary);
            }
            let id = synced.issue.id.clone();
            let task = match records.remove(&id) {
                Some(task) => refresh(task, synced),
                None => refresh(blank(new_id(), synced.created), synced),
            };
            records.insert(id, task);
        }
        let parents: HashMap<String, String> = records
            .iter()
            .filter_map(|(id, t)| Some((id.clone(), t.issue.as_ref()?.parent.clone()?)))
            .collect();

        let dropped = self.place(records, &parents);
        // An epic stays while it is a card or a task names it.
        let used: HashSet<String> = self
            .all_tasks()
            .filter_map(|t| {
                let issue = t.issue.as_ref()?;
                if t.kind == Some(super::TaskKind::Epic) {
                    Some(issue.key.clone())
                } else {
                    issue.epic.clone()
                }
            })
            .collect();
        let epics = std::mem::take(&mut self.epics);
        self.epics = epics
            .into_iter()
            .filter(|e| used.contains(&e.key))
            .collect();
        self.epics.sort_by(|a, b| a.key.cmp(&b.key));

        MergeReport {
            changed: *self != before,
            dropped,
        }
    }

    /// Puts every record in its place: subtasks under their parent, cards in their
    /// status's column, in rank order. Returns how many didn't fit.
    fn place(&mut self, records: HashMap<String, Task>, parents: &HashMap<String, String>) -> u32 {
        let mut dropped = 0u32;
        let mut children: HashMap<String, Vec<Task>> = HashMap::new();
        let mut cards = Vec::new();
        for (id, task) in records {
            match parents.get(&id) {
                // Jira subtasks have no subtasks of their own, so one level suffices.
                Some(parent) if !parents.contains_key(parent) => {
                    children.entry(parent.clone()).or_default().push(task);
                }
                _ => cards.push(task),
            }
        }
        let card_ids: HashSet<String> = cards.iter().filter_map(issue_id).collect();
        for (parent, orphans) in &mut children {
            if !card_ids.contains(parent) {
                // The parent isn't on the board (another status, or not loaded): show
                // the subtasks as cards until it is.
                cards.append(orphans);
            }
        }
        let mut columns: Vec<Vec<Task>> = vec![Vec::new(); self.columns.len()];
        for mut card in cards {
            let status = card.issue.as_ref().map_or("", |i| i.status_id.as_str());
            let Some(at) = self
                .columns
                .iter()
                .position(|c| c.statuses.iter().any(|s| s == status))
            else {
                continue;
            };
            card.done = false;
            if let Some(mut subtasks) = issue_id(&card).and_then(|id| children.remove(&id)) {
                subtasks.sort_by(by_rank);
                if subtasks.len() > MAX_SUBTASKS {
                    dropped += count(subtasks.len() - MAX_SUBTASKS);
                    subtasks.truncate(MAX_SUBTASKS);
                }
                for s in &mut subtasks {
                    s.done = s
                        .issue
                        .as_ref()
                        .is_some_and(|i| i.category == StatusCategory::Done);
                }
                card.subtasks = subtasks;
            }
            columns[at].push(card);
        }
        let mut room = MAX_SYNCED_ISSUES.saturating_sub(self.all_tasks().count());
        for (column, mut cards) in self.columns.iter_mut().zip(columns) {
            cards.sort_by(by_rank);
            for card in cards {
                let size = 1 + card.subtasks.len();
                if size <= room {
                    room -= size;
                    column.tasks.push(card);
                } else {
                    dropped += count(size);
                }
            }
        }
        dropped
    }
}

/// Splits a task and its subtasks into records by issue id.
fn flatten(mut task: Task, records: &mut HashMap<String, Task>) {
    let Some(id) = issue_id(&task) else {
        return;
    };
    for sub in std::mem::take(&mut task.subtasks) {
        flatten(sub, records);
    }
    records.insert(id, task);
}

fn issue_id(task: &Task) -> Option<String> {
    task.issue.as_ref().map(|i| i.id.clone())
}

/// An empty task to fill from an issue.
fn blank(id: TaskId, created_at: Timestamp) -> Task {
    Task {
        id,
        title: String::new(),
        notes: String::new(),
        created_at,
        done: false,
        color: None,
        tags: Vec::new(),
        attachments: Vec::new(),
        subtasks: Vec::new(),
        thread: None,
        kind: None,
        issue: None,
    }
}

/// `task` with Jira's fields from `synced`; its id, link and color are kept.
fn refresh(mut task: Task, synced: SyncedIssue) -> Task {
    let title = clip(
        &synced
            .title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        MAX_TITLE,
    );
    task.title = if title.is_empty() {
        format!("({})", synced.issue.key)
    } else {
        title
    };
    let notes = synced.notes.trim_end();
    let mut issue = synced.issue;
    issue.notes_editable = notes.chars().count() <= MAX_NOTES;
    task.notes = clip(notes, MAX_NOTES);
    task.tags = labels(&synced.labels);
    task.kind = Some(synced.kind);
    task.issue = Some(issue);
    task
}

/// Jira labels as tags: those a tag can hold, at most [`MAX_TAGS`]. Labels are left
/// out rather than changed, so writing the tags back never renames one.
fn labels(labels: &[String]) -> Vec<String> {
    labels
        .iter()
        .filter(|l| {
            !l.is_empty() && l.chars().count() <= MAX_TAG && !l.contains(char::is_whitespace)
        })
        .take(MAX_TAGS)
        .cloned()
        .collect()
}

fn upsert_epic(epics: &mut Vec<Epic>, key: &str, summary: &str) {
    let summary = clip(summary.trim(), MAX_TITLE);
    match epics.iter_mut().find(|e| e.key == key) {
        Some(epic) => epic.summary = summary,
        None => epics.push(Epic {
            key: key.to_owned(),
            summary,
            color: tag_color(key),
        }),
    }
}

/// Rank order; unranked issues last, by key.
fn by_rank(a: &Task, b: &Task) -> std::cmp::Ordering {
    let key = |t: &Task| {
        let issue = t.issue.as_ref();
        let rank = issue.map_or("", |i| i.rank.as_str());
        (
            rank.is_empty(),
            rank.to_owned(),
            issue.map_or_else(String::new, sort_key),
        )
    };
    key(a).cmp(&key(b))
}

/// `PROJ-12` sorts before `PROJ-100`.
fn sort_key(issue: &Issue) -> String {
    match issue.key.rsplit_once('-') {
        Some((project, n)) => format!("{project}-{n:0>12}"),
        None => issue.key.clone(),
    }
}

/// The first `max` characters of `text`.
fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
