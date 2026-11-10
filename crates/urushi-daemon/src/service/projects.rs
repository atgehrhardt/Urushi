//! Projects (user-made groups of threads) and the organization of threads: titles,
//! pins, statuses and which project a thread is filed under. None of this touches
//! worktrees or branches.

use urushi_core::{Project, ProjectId, ProjectSettings, Status, ThreadId, order, project};

use super::{Service, ServiceError, ServiceResult, new_uuid, now};

/// Sets or clears an optional value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Assign<T> {
    /// Clear it.
    Clear,
    /// Set it.
    Set(T),
}

/// Changes to a thread's organization. `None` fields are left as they are.
#[derive(Clone, Debug, Default)]
pub struct ThreadUpdate {
    /// New title.
    pub title: Option<String>,
    /// Pin or unpin.
    pub pinned: Option<bool>,
    /// Set or clear the status.
    pub status: Option<Assign<Status>>,
    /// File under a project, or ungroup.
    pub project: Option<Assign<ProjectId>>,
}

/// Longest project name, in characters.
const MAX_PROJECT_NAME: usize = 80;

fn project_name(name: &str) -> ServiceResult<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(ServiceError::Invalid(
            "a project name must not be empty".into(),
        ));
    }
    if name.chars().count() > MAX_PROJECT_NAME {
        return Err(ServiceError::Invalid(format!(
            "a project name must be at most {MAX_PROJECT_NAME} characters"
        )));
    }
    Ok(name)
}

impl Service {
    /// Creates an empty project at the end of the list.
    pub fn create_project(&self, name: &str) -> ServiceResult<Project> {
        let project = Project {
            id: ProjectId::from_uuid(new_uuid()),
            name: project_name(name)?,
            created_at: now(),
            settings: ProjectSettings::default(),
        };
        self.mutate(|state| {
            state.projects.push(project.clone());
            Ok(project)
        })
    }

    /// Renames a project.
    pub fn rename_project(&self, id: ProjectId, name: &str) -> ServiceResult<()> {
        let name = project_name(name)?;
        self.mutate(|state| {
            state.project_mut(id)?.name = name;
            Ok(())
        })
    }

    /// Deletes a project. Its threads become ungrouped, its parent-branch thread too
    /// (nothing on disk changes; it can be closed like any thread).
    pub fn delete_project(&self, id: ProjectId) -> ServiceResult<()> {
        self.mutate(|state| {
            state.project(id)?;
            state.projects.retain(|p| p.id != id);
            for record in &mut state.threads {
                if record.thread.project == Some(id) {
                    record.thread.project = None;
                }
            }
            Ok(())
        })
    }

    /// Moves a project to `index` among the others (counted without it; clamped to the
    /// end). Projects are listed in this order.
    pub fn move_project(&self, id: ProjectId, index: usize) -> ServiceResult<()> {
        self.mutate(|state| {
            state.project(id)?;
            project::move_project(&mut state.projects, id, index);
            Ok(())
        })
    }

    /// Moves a thread to `index` in the list of threads (counted without it; clamped to
    /// the end). The sidebar shows threads in this order within their project, pin and
    /// status groups; which group a thread is in doesn't change.
    pub fn move_thread(&self, id: ThreadId, index: usize) -> ServiceResult<()> {
        self.mutate(|state| {
            state.thread(id)?;
            order::move_where(&mut state.threads, |r| r.thread.id == id, index);
            Ok(())
        })
    }

    /// Applies `update` to a thread, all or nothing. A project's parent-branch thread
    /// stays in its project.
    pub fn update_thread(&self, id: ThreadId, update: ThreadUpdate) -> ServiceResult<()> {
        let title = match update.title.as_deref().map(str::trim) {
            Some("") => return Err(ServiceError::Invalid("title must not be empty".into())),
            other => other.map(str::to_owned),
        };
        self.mutate(|state| {
            if let Some(Assign::Set(project)) = update.project {
                state.project(project)?;
            }
            // Checked before anything changes: a failed mutation isn't rolled back.
            let current = &state.thread(id)?.thread;
            let target = match update.project {
                Some(Assign::Set(p)) => Some(Some(p)),
                Some(Assign::Clear) => Some(None),
                None => None,
            };
            if current.is_project_base() && target.is_some_and(|t| t != current.project) {
                return Err(ServiceError::Precondition(
                    "a parent-branch thread stays in its project".into(),
                ));
            }
            let thread = &mut state.thread_mut(id)?.thread;
            if let Some(title) = title {
                thread.title = title;
            }
            if let Some(pinned) = update.pinned {
                thread.pinned = pinned;
            }
            match update.status {
                Some(Assign::Set(status)) => thread.status = Some(status),
                Some(Assign::Clear) => thread.status = None,
                None => {}
            }
            match update.project {
                Some(Assign::Set(project)) => thread.project = Some(project),
                Some(Assign::Clear) => thread.project = None,
                None => {}
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_names_are_trimmed_and_bounded() {
        assert_eq!(
            project_name("  Billing   launch ").unwrap(),
            "Billing launch"
        );
        assert!(project_name("   ").is_err());
        assert!(project_name(&"x".repeat(81)).is_err());
    }
}
