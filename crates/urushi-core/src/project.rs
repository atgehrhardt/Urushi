//! A project: a named group of threads, made by the user to organize work.
//!
//! Projects are only organization. A project can hold threads from any repo, and a
//! thread is in at most one project (`Thread::project`). Deleting a project ungroups its
//! threads; it never touches a worktree or branch.

use serde::{Deserialize, Serialize};

use crate::{ProjectId, Timestamp};

/// A user-made group of threads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Identity.
    pub id: ProjectId,
    /// Display name.
    pub name: String,
    /// When it was created.
    pub created_at: Timestamp,
}

/// Moves project `id` to `index` in the list (counted without it; clamped to the end).
/// The list's order is the order projects are shown in. Returns `false`, changing
/// nothing, if there is no such project.
pub fn move_project(projects: &mut Vec<Project>, id: ProjectId, index: usize) -> bool {
    let Some(at) = projects.iter().position(|p| p.id == id) else {
        return false;
    };
    let project = projects.remove(at);
    projects.insert(index.min(projects.len()), project);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects(n: u128) -> Vec<Project> {
        (0..n)
            .map(|i| Project {
                id: ProjectId::from_u128(i),
                name: format!("p{i}"),
                created_at: Timestamp::unknown(),
            })
            .collect()
    }

    fn order(projects: &[Project]) -> Vec<&str> {
        projects.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn projects_move_up_and_down() {
        let mut list = projects(3);
        assert!(move_project(&mut list, ProjectId::from_u128(2), 0));
        assert_eq!(order(&list), ["p2", "p0", "p1"]);
        assert!(move_project(&mut list, ProjectId::from_u128(2), 2));
        assert_eq!(order(&list), ["p0", "p1", "p2"]);
        assert!(move_project(&mut list, ProjectId::from_u128(0), 1));
        assert_eq!(order(&list), ["p1", "p0", "p2"]);
    }

    #[test]
    fn moving_clamps_to_the_end_and_keeps_its_place() {
        let mut list = projects(3);
        assert!(move_project(&mut list, ProjectId::from_u128(0), 99));
        assert_eq!(order(&list), ["p1", "p2", "p0"]);
        assert!(move_project(&mut list, ProjectId::from_u128(2), 1));
        assert_eq!(order(&list), ["p1", "p2", "p0"]);
    }

    #[test]
    fn an_unknown_project_changes_nothing() {
        let mut list = projects(2);
        assert!(!move_project(&mut list, ProjectId::from_u128(7), 0));
        assert_eq!(order(&list), ["p0", "p1"]);
    }
}
