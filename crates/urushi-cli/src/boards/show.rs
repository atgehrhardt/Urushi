//! `urushi board show <task|KEY>`: a task in full, as plain text an agent can read
//! (ADR 0024). Where it is (its board, column, and the tasks it is part of), its notes,
//! subtasks and attachments; for a Jira issue its address, type, status and people,
//! and, from a 1.13 daemon, its other fields, comments, links and files.

use anyhow::{Context, Result};
use urushi_client::{Client, FoundTask, TaskRef, WorkspaceMirror, v1};

use crate::commands::{out, resolve};

/// First protocol minor version that reads a Jira issue in full.
const ISSUES_SINCE_MINOR: u32 = 13;

/// The task `which` names: a Jira issue key (any case), else an id or its start.
async fn find(client: &Client, mirror: &WorkspaceMirror, which: &str) -> Result<String> {
    let by_key = mirror
        .all_tasks()
        .find(|t| t.issue().is_some_and(|i| i.key.eq_ignore_ascii_case(which)))
        .map(|t| t.id().to_owned());
    match by_key {
        Some(id) => Ok(id),
        None => resolve(client, which).await,
    }
}

fn name(t: TaskRef<'_>) -> String {
    match t.issue() {
        Some(i) => format!("{} {}", i.key, t.title()),
        None => t.title().to_owned(),
    }
}

pub(crate) async fn show(client: &Client, which: &str) -> Result<()> {
    let mirror = WorkspaceMirror::new(client.list_workspace(false).await?);
    let id = find(client, &mirror, which).await?;
    let found = mirror
        .find_task(&id)
        .with_context(|| format!("{which:?} isn't a task"))?;
    let task = found.task();
    out(&name(task))?;
    summary(&found)?;
    let notes = task.notes().trim();
    if !notes.is_empty() {
        out(&format!("\nNotes:\n{notes}"))?;
    }
    if !task.subtasks().is_empty() {
        out("\nSubtasks:")?;
        for s in task.subtasks() {
            let mark = if s.done { "x" } else { " " };
            out(&format!("- [{mark}] {}", name(TaskRef::Sub(s))))?;
        }
    }
    if !task.attachments().is_empty() {
        out("\nAttachments:")?;
        for a in task.attachments() {
            out(&format!("- {} ({} bytes)", a.name, a.size))?;
        }
    }
    if task.issue().is_some() && client.welcome().protocol_minor >= ISSUES_SINCE_MINOR {
        details(&client.jira_issue(&id).await?)?;
    }
    Ok(())
}

/// Where the task is, and a Jira issue's own facts.
fn summary(found: &FoundTask<'_>) -> Result<()> {
    let task = found.task();
    let mut place = format!("Board: {} › {}", found.board.name, found.column.name);
    if task.done() {
        place.push_str(" (done)");
    }
    out(&place)?;
    let above: Vec<String> = found.path[..found.path.len().saturating_sub(1)]
        .iter()
        .map(|t| name(*t))
        .collect();
    if !above.is_empty() {
        out(&format!("Part of: {}", above.join(" › ")))?;
    }
    if !task.tags().is_empty() {
        out(&format!("Tags: {}", task.tags().join(", ")))?;
    }
    if !task.pr_url().is_empty() {
        out(&format!("Pull request: {}", task.pr_url()))?;
    }
    let Some(i) = task.issue() else {
        return Ok(());
    };
    out(&format!("Jira: {}", i.url))?;
    for pr in &i.pull_requests {
        out(&format!("Pull request: {}{}", pr.url, pr_facts(pr)))?;
    }
    let mut facts = vec![
        format!("Type: {}", i.type_name),
        format!("Status: {}", i.status),
    ];
    let unassigned = "Unassigned".to_owned();
    facts.push(format!(
        "Assignee: {}",
        if i.assignee.is_empty() {
            &unassigned
        } else {
            &i.assignee
        }
    ));
    if !i.priority.is_empty() {
        facts.push(format!("Priority: {}", i.priority));
    }
    if let Some(epic) = found.board.epics.iter().find(|e| e.key == i.epic_key) {
        facts.push(format!("Epic: {} {}", epic.key, epic.summary));
    }
    out(&facts.join(" · "))
}

/// A Jira pull request's status and title, after its URL.
fn pr_facts(pr: &v1::PullRequest) -> String {
    let status = match v1::PullRequestStatus::try_from(pr.status) {
        Ok(v1::PullRequestStatus::Open) => " (open)",
        Ok(v1::PullRequestStatus::Merged) => " (merged)",
        Ok(v1::PullRequestStatus::Declined) => " (declined)",
        _ => "",
    };
    if pr.title.is_empty() {
        status.to_owned()
    } else {
        format!("{status} {}", pr.title)
    }
}

/// A Jira issue's other fields, comments, links and files.
fn details(d: &v1::JiraIssueDetails) -> Result<()> {
    let fields: Vec<String> = d
        .fields
        .iter()
        .filter_map(|f| {
            let v = f.value.as_ref()?;
            let shown = if v.options.is_empty() {
                v.text.clone()
            } else {
                v.options
                    .iter()
                    .map(|o| o.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            (!shown.trim().is_empty()).then(|| format!("- {}: {shown}", f.name))
        })
        .collect();
    if !fields.is_empty() {
        out(&format!("\nFields:\n{}", fields.join("\n")))?;
    }
    if !d.comments.is_empty() {
        out("\nComments:")?;
        for c in &d.comments {
            let who = c.author.as_ref().map_or("Someone", |a| a.name.as_str());
            out(&format!("- {who}:\n  {}", c.body.replace('\n', "\n  ")))?;
        }
    }
    if !d.links.is_empty() {
        out("\nLinks:")?;
        for l in &d.links {
            if let Some(i) = &l.issue {
                out(&format!(
                    "- {} {} {} [{}]",
                    l.relation, i.key, i.summary, i.status
                ))?;
            }
        }
    }
    if !d.attachments.is_empty() {
        out("\nFiles in Jira:")?;
        for a in &d.attachments {
            out(&format!("- {} ({} bytes)", a.name, a.size))?;
        }
    }
    Ok(())
}
