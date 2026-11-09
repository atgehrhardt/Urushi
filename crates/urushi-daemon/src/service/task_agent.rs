//! Starting the user's task agent in a thread made for a task (ADR 0014).
//!
//! Everything that can fail (no task agent chosen, the agent not installed, the task
//! gone) is checked by [`Service::prepare_task_agent`] before the thread's worktree is
//! made, so a refused request leaves nothing behind. The prompt travels in the first
//! pane's environment, and the typed command line refers to it.

use urushi_core::{Agent, PaneId, SubtaskId, TASK_PROMPT_ENV, TaskId, ThreadId};

use super::{Service, ServiceError, ServiceResult};

/// The agent a new thread's first tab starts, ready to go.
#[derive(Clone, Debug)]
pub(super) struct TaskAgent {
    /// Which agent; the tab is named after it.
    pub(super) agent: Agent,
    /// The line typed into the pane's shell.
    pub(super) command_line: String,
    /// The task's prompt, put in the pane's environment as [`TASK_PROMPT_ENV`].
    pub(super) prompt: String,
}

impl TaskAgent {
    /// The environment the pane's shell needs for [`Self::command_line`].
    pub(super) fn env(&self) -> Vec<(String, String)> {
        vec![(TASK_PROMPT_ENV.to_owned(), self.prompt.clone())]
    }
}

impl Service {
    /// The task agent a new thread should start: `None` unless `start` is set, which
    /// needs a task `link`.
    pub(super) async fn task_agent_for(
        &self,
        start: bool,
        link: Option<(TaskId, Option<SubtaskId>)>,
        shell: Option<String>,
    ) -> ServiceResult<Option<TaskAgent>> {
        match (start, link) {
            (false, _) => Ok(None),
            (true, None) => Err(ServiceError::Invalid(
                "start_task_agent needs a task_id".into(),
            )),
            (true, Some(link)) => self.prepare_task_agent(link, shell).await.map(Some),
        }
    }

    /// Types the task agent's command into the new thread's first pane. The thread stands
    /// on its own once recorded: if this fails it keeps its shell, and the user can start
    /// the agent by hand.
    pub(super) async fn start_task_agent(&self, thread: ThreadId, pane: PaneId, line: &str) {
        if let Err(e) = self.type_command(pane, line).await {
            tracing::warn!(%thread, error = %e, "couldn't start the task agent");
        }
    }

    /// Resolves the user's task agent, its mode and the prompt for `link`. Fails if no
    /// task agent is chosen, it isn't installed, or the task or subtask doesn't exist.
    pub(super) async fn prepare_task_agent(
        &self,
        link: (TaskId, Option<SubtaskId>),
        shell: Option<String>,
    ) -> ServiceResult<TaskAgent> {
        let (choice, prompt) = self.read(|s| {
            let agents = &s.settings.agents;
            Ok((
                agents.task_agent.map(|a| (a, agents.task_mode)),
                s.task_prompt(link)?,
            ))
        })?;
        let (agent, mode) = choice.ok_or_else(|| {
            ServiceError::Precondition(
                "no agent is chosen for tasks yet; pick one in Settings".into(),
            )
        })?;
        let command_line = self.agent_task_line(agent, shell, mode).await?;
        Ok(TaskAgent {
            agent,
            command_line,
            prompt,
        })
    }
}
