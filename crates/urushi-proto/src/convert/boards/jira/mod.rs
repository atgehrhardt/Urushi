//! Jira boards and task kinds: core ⇄ wire (since 1.12). Unknown enum values from a
//! newer peer read as unspecified; an issue's web address is filled in from its board's
//! site, so it travels without being stored.

use urushi_core::board::{
    BoardSource, Epic, Issue, JiraBoard, JiraQuickFilter, Named, Person, StatusCategory, SyncState,
    TaskKind,
};

use super::{color_from_proto, color_to_proto, pull_request_to_proto, pull_requests_from_proto};
use crate::convert::{millis_from_proto, millis_to_proto};
use crate::v1;

/// Core kind → wire enum value.
pub fn kind_to_proto(kind: Option<TaskKind>) -> i32 {
    let wire = match kind {
        None => v1::TaskKind::Unspecified,
        Some(TaskKind::Task) => v1::TaskKind::Task,
        Some(TaskKind::Story) => v1::TaskKind::Story,
        Some(TaskKind::Bug) => v1::TaskKind::Bug,
        Some(TaskKind::Epic) => v1::TaskKind::Epic,
        Some(TaskKind::Subtask) => v1::TaskKind::Subtask,
    };
    wire.into()
}

/// Wire enum value → core kind; unknown values are none.
pub fn kind_from_proto(value: i32) -> Option<TaskKind> {
    match v1::TaskKind::try_from(value).ok()? {
        v1::TaskKind::Unspecified => None,
        v1::TaskKind::Task => Some(TaskKind::Task),
        v1::TaskKind::Story => Some(TaskKind::Story),
        v1::TaskKind::Bug => Some(TaskKind::Bug),
        v1::TaskKind::Epic => Some(TaskKind::Epic),
        v1::TaskKind::Subtask => Some(TaskKind::Subtask),
    }
}

/// Core status category → wire enum value.
pub fn category_to_proto(c: StatusCategory) -> i32 {
    match c {
        StatusCategory::ToDo => v1::JiraStatusCategory::ToDo,
        StatusCategory::InProgress => v1::JiraStatusCategory::InProgress,
        StatusCategory::Done => v1::JiraStatusCategory::Done,
    }
    .into()
}

fn category_from_proto(value: i32) -> StatusCategory {
    match v1::JiraStatusCategory::try_from(value) {
        Ok(v1::JiraStatusCategory::InProgress) => StatusCategory::InProgress,
        Ok(v1::JiraStatusCategory::Done) => StatusCategory::Done,
        _ => StatusCategory::ToDo,
    }
}

/// Core source → wire (`None` for a board of Urushi's own).
pub fn source_to_proto(source: Option<&BoardSource>) -> Option<v1::JiraBoardSource> {
    let BoardSource::Jira(j) = source?;
    Some(v1::JiraBoardSource {
        site: j.site.clone(),
        board_id: j.board_id,
        name: j.name.clone(),
        project_key: j.project_key.clone(),
        scrum: j.scrum,
        synced_at_ms: j.sync.synced_at.map_or(0, millis_to_proto),
        error: j.sync.error.clone().unwrap_or_default(),
        dropped: j.sync.dropped,
        url: j.board_url(),
        quick_filters: j
            .quick_filters
            .iter()
            .map(|q| v1::JiraQuickFilter {
                id: q.id,
                name: q.name.clone(),
                jql: q.jql.clone(),
            })
            .collect(),
    })
}

/// Wire source → core. The rank, estimate and sprint fields are the daemon's and aren't
/// sent.
pub fn source_from_proto(source: Option<&v1::JiraBoardSource>) -> Option<BoardSource> {
    let j = source?;
    Some(BoardSource::Jira(JiraBoard {
        site: j.site.clone(),
        board_id: j.board_id,
        name: j.name.clone(),
        project_key: j.project_key.clone(),
        scrum: j.scrum,
        rank_field: None,
        points_field: None,
        sprint_field: None,
        quick_filters: j
            .quick_filters
            .iter()
            .map(|q| JiraQuickFilter {
                id: q.id,
                name: q.name.clone(),
                jql: q.jql.clone(),
            })
            .collect(),
        sync: SyncState {
            synced_at: (j.synced_at_ms != 0).then(|| millis_from_proto(j.synced_at_ms)),
            error: (!j.error.is_empty()).then(|| j.error.clone()),
            dropped: j.dropped,
        },
    }))
}

/// Core epic → wire.
pub fn epic_to_proto(e: &Epic, site: &str) -> v1::Epic {
    v1::Epic {
        key: e.key.clone(),
        summary: e.summary.clone(),
        color: color_to_proto(Some(e.color)),
        url: format!("{site}/browse/{}", e.key),
    }
}

/// Wire epic → core; an unknown color reads as gray.
pub fn epic_from_proto(e: &v1::Epic) -> Epic {
    Epic {
        key: e.key.clone(),
        summary: e.summary.clone(),
        color: color_from_proto(e.color).unwrap_or(urushi_core::LabelColor::Gray),
    }
}

/// Core issue → wire, with its web address on `site`.
pub fn issue_to_proto(i: &Issue, site: Option<&str>) -> v1::JiraIssue {
    v1::JiraIssue {
        id: i.id.clone(),
        key: i.key.clone(),
        url: site.map_or_else(String::new, |s| format!("{s}/browse/{}", i.key)),
        type_name: i.type_name.clone(),
        status_id: i.status_id.clone(),
        status: i.status.clone(),
        category: category_to_proto(i.category),
        assignee: i.assignee.clone().unwrap_or_default(),
        priority: i.priority.clone().unwrap_or_default(),
        epic_key: i.epic.clone().unwrap_or_default(),
        notes_editable: i.notes_editable,
        parent_id: i.parent.clone().unwrap_or_default(),
        rank: i.rank.clone(),
        updated_ms: millis_to_proto(i.updated),
        type_id: i.type_id.clone(),
        assignee_id: i.assignee_id.clone().unwrap_or_default(),
        reporter: i.reporter.as_ref().map(|p| v1::JiraUser {
            account_id: p.account_id.clone(),
            name: p.name.clone(),
        }),
        priority_id: i.priority_id.clone().unwrap_or_default(),
        due: i.due.clone().unwrap_or_default(),
        points: i.points.clone().unwrap_or_default(),
        sprint: i.sprint.as_ref().map(named_to_proto),
        components: i.components.iter().map(named_to_proto).collect(),
        fix_versions: i.fix_versions.iter().map(named_to_proto).collect(),
        links: i.links,
        pull_requests: i.pull_requests.iter().map(pull_request_to_proto).collect(),
    }
}

fn named_to_proto(n: &Named) -> v1::JiraNamed {
    v1::JiraNamed {
        id: n.id.clone(),
        name: n.name.clone(),
    }
}

fn named_from_proto(n: &v1::JiraNamed) -> Named {
    Named {
        id: n.id.clone(),
        name: n.name.clone(),
    }
}

/// Wire issue → core.
pub fn issue_from_proto(i: &v1::JiraIssue) -> Issue {
    let some = |s: &String| (!s.is_empty()).then(|| s.clone());
    Issue {
        id: i.id.clone(),
        key: i.key.clone(),
        type_name: i.type_name.clone(),
        status_id: i.status_id.clone(),
        status: i.status.clone(),
        category: category_from_proto(i.category),
        assignee: some(&i.assignee),
        priority: some(&i.priority),
        epic: some(&i.epic_key),
        parent: some(&i.parent_id),
        rank: i.rank.clone(),
        updated: millis_from_proto(i.updated_ms),
        notes_editable: i.notes_editable,
        type_id: i.type_id.clone(),
        assignee_id: some(&i.assignee_id),
        reporter: i.reporter.as_ref().map(|p| Person {
            account_id: p.account_id.clone(),
            name: p.name.clone(),
        }),
        priority_id: some(&i.priority_id),
        due: some(&i.due),
        points: some(&i.points),
        sprint: i.sprint.as_ref().map(named_from_proto),
        components: i.components.iter().map(named_from_proto).collect(),
        fix_versions: i.fix_versions.iter().map(named_from_proto).collect(),
        links: i.links,
        pull_requests: pull_requests_from_proto(&i.pull_requests),
    }
}

#[cfg(test)]
mod tests;
