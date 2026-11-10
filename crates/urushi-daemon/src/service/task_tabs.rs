//! Handing a board task to a thread that already exists (ADR 0018): a new tab in it,
//! running the task agent with the task as its prompt, and the task linked to it.
//!
//! Like starting a thread for a task, everything that can fail is checked before the
//! tab's terminal starts, and the link is recorded in the same mutation as the tab.

use urushi_core::{PaneId, Tab, TabId, TaskId, ThreadId};
use urushi_mux::TermSize;

use super::task_agent::TaskAgent;
use super::threads::DEFAULT_TAB_TITLE;
use super::{Service, ServiceError, ServiceResult, new_uuid};

impl Service {
    /// Opens a new tab in `thread` for the task (and maybe subtask of it) `link`, and
    /// links the task to the thread, moving it from any thread it was linked to before.
    /// With `start_agent`, the tab runs the user's task agent with the task as its
    /// prompt; otherwise it is a plain shell.
    pub async fn create_task_tab(
        &self,
        thread: ThreadId,
        link: Option<(TaskId, Option<TaskId>)>,
        start_agent: bool,
    ) -> ServiceResult<Tab> {
        let (record, shell, target) = self.read(|s| {
            let record = s.thread(thread)?.thread.clone();
            if !record.is_ready() {
                return Err(ServiceError::Precondition("thread is not ready".into()));
            }
            let target = link.map(|l| s.task_link_target(l)).transpose()?;
            let shell = s.effective_settings(record.repo).default_shell;
            Ok((record, shell, target))
        })?;
        // Resolve the agent and the prompt first, so a refusal leaves nothing behind.
        let agent = self
            .task_agent_for(start_agent, link, shell.clone())
            .await?;

        let tab_id = TabId::from_uuid(new_uuid());
        let pane = PaneId::from_uuid(new_uuid());
        let env = agent.as_ref().map(TaskAgent::env).unwrap_or_default();
        self.spawn_session_with_env(pane, &record, shell, TermSize::DEFAULT, env)?;
        let title = agent
            .as_ref()
            .map_or(DEFAULT_TAB_TITLE, |a| a.agent.display_name());
        let tab = Tab::new(tab_id, thread, title, pane);
        let result = self.mutate(|state| {
            if let Some(target) = target {
                // Checked above, but the task may have gone while the shell started.
                state
                    .boards
                    .iter_mut()
                    .find(|b| b.task(target).is_some())
                    .ok_or_else(|| ServiceError::NotFound(format!("task {target}")))?
                    .link_thread(target, thread)?;
            }
            state.thread_mut(thread)?.thread.tabs.push(tab_id);
            state.tabs.push(tab.clone());
            Ok(tab)
        });
        if result.is_err() {
            self.kill_sessions([pane]);
        }
        let tab = result?;
        if let Some(agent) = agent {
            self.start_task_agent(thread, pane, agent).await;
        }
        Ok(tab)
    }
}
