use async_trait::async_trait;
use helixflow_agent::{
    AgentError, AgentSessionRequest, ValidatedAgentProposal, ValidatedAgentReply,
};

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

    async fn propose_graph_change(
        &self,
        _request: AgentSessionRequest,
    ) -> Result<ValidatedAgentProposal, AgentError> {
        Err(AgentError::Runtime("noop agent".to_owned()))
    }
}
