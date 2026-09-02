use async_trait::async_trait;
use helixflow_agent::{AgentError, AgentSessionRequest, ValidatedAgentReply};

use crate::app_state::WorkbenchAgent;

pub(crate) struct FailingWorkbenchAgent;

#[async_trait]
impl WorkbenchAgent for FailingWorkbenchAgent {
    async fn answer_chat(
        &self,
        _request: AgentSessionRequest,
    ) -> Result<ValidatedAgentReply, AgentError> {
        Err(AgentError::Runtime("noop agent".to_owned()))
    }
}
