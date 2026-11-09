//! Agent CLIs: which are installed, starting one in a new tab (ADR 0009), and resuming
//! one in a restored pane.

use urushi_core::{Agent, AgentSessionId};
use urushi_mux::Input;

use super::{Service, ServiceError, ServiceResult, blocking};
use crate::agents::FoundAgent;

impl Service {
    /// The agent CLIs installed on this machine, as the user's shell would find them.
    pub async fn installed_agents(&self) -> ServiceResult<Vec<FoundAgent>> {
        let shell = self.settings().default_shell;
        let locator = self.agents.clone();
        blocking(move || Ok(locator.installed(shell.as_deref()))).await
    }

    /// The command line that starts `agent`, honouring the user's skip-permissions choice.
    /// Fails if the agent isn't installed.
    pub(super) async fn agent_command_line(
        &self,
        agent: Agent,
        shell: Option<String>,
    ) -> ServiceResult<String> {
        let found = self.find_agent(agent, shell).await?;
        let skip = self.settings().agents.skips_permissions(agent);
        Ok(agent.command_line(&found.program, skip))
    }

    /// The command line that resumes `agent`'s conversation `session` (or its latest one
    /// in the current directory), honouring the skip-permissions choice. Fails if the
    /// agent isn't installed.
    pub(super) async fn agent_resume_line(
        &self,
        agent: Agent,
        shell: Option<String>,
        session: Option<&AgentSessionId>,
    ) -> ServiceResult<String> {
        let found = self.find_agent(agent, shell).await?;
        let skip = self.settings().agents.skips_permissions(agent);
        Ok(agent.resume_command_line(&found.program, skip, session))
    }

    async fn find_agent(&self, agent: Agent, shell: Option<String>) -> ServiceResult<FoundAgent> {
        let locator = self.agents.clone();
        let found = blocking(move || Ok(locator.find(agent, shell.as_deref()))).await?;
        found.ok_or_else(|| {
            ServiceError::Precondition(format!(
                "{agent} is not installed: no `{}` was found on your PATH",
                agent.program()
            ))
        })
    }

    /// Types `command_line` into `pane`'s shell and presses Enter. The terminal buffers
    /// it until the shell is ready to read, and the shell stays when the command exits.
    pub(super) async fn type_command(
        &self,
        pane: urushi_core::PaneId,
        command_line: &str,
    ) -> ServiceResult<()> {
        self.session(pane)?
            .input(Input::Text(format!("{command_line}\r")))
            .await?;
        Ok(())
    }
}
