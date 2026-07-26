use helixflow_compiler::IntentPlan;
use helixflow_graph::{
    GraphService, PreparedProposal, ProposalDraft, ProposalKind, ProposalOp, WorkflowGraph,
};
use helixflow_registry::NodeRegistry;
use serde::{Deserialize, Serialize};

use crate::{AgentError, AgentResult, AgentSession, read_output_file};

#[derive(Debug, Clone)]
pub struct ValidatedAgentProposal {
    pub session_id: String,
    pub agent_logs: Vec<AgentLogEntry>,
    pub proposal: PreparedProposal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedAgentReply {
    pub session_id: String,
    pub agent_logs: Vec<AgentLogEntry>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedRunRequest {
    pub session_id: String,
    pub request: RunRequestOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentLogEntry {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunRequestOutput {
    pub action: RunRequestAction,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunRequestAction {
    RequestConfirmation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalOutput {
    base_version_id: String,
    kind: ProposalKind,
    title: String,
    summary: String,
    ops: Vec<ProposalOp>,
    #[serde(default)]
    message_id: Option<String>,
}

impl From<ProposalOutput> for ProposalDraft {
    fn from(value: ProposalOutput) -> Self {
        Self {
            base_version_id: value.base_version_id,
            kind: value.kind,
            title: value.title,
            summary: value.summary,
            ops: value.ops,
            message_id: value.message_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyOutput {
    message: String,
}

pub fn read_validated_proposal(
    session: &AgentSession,
    base_graph: &WorkflowGraph,
    current_version_id: &str,
) -> AgentResult<ValidatedAgentProposal> {
    let output_path = session.out_dir.join("proposal.json");
    let output: ProposalOutput =
        serde_json::from_slice(&read_output_file(&session.out_dir, &output_path)?)?;
    let proposal = GraphService::new(NodeRegistry::builtin()).preview_proposal(
        base_graph,
        current_version_id,
        output.into(),
    )?;

    Ok(ValidatedAgentProposal {
        session_id: session.id.clone(),
        agent_logs: Vec::new(),
        proposal,
    })
}

/// Reads and validates `out/intent.json` (GH130 T3). The IntentPlan schema
/// rejects unknown fields and the structural validator rejects topology and
/// reference errors before anything reaches the compiler. The runtime keeps
/// emitting `proposal.json` until the T6 switchover; this contract entry is
/// flag-gated groundwork.
pub fn read_validated_intent(session: &AgentSession) -> AgentResult<IntentPlan> {
    let output_path = session.out_dir.join("intent.json");
    let intent: IntentPlan =
        serde_json::from_slice(&read_output_file(&session.out_dir, &output_path)?)?;
    intent
        .validate()
        .map_err(|err| AgentError::InvalidOutputFile {
            path: output_path,
            reason: err.to_string(),
        })?;
    Ok(intent)
}

pub fn read_validated_reply(session: &AgentSession) -> AgentResult<ValidatedAgentReply> {
    let output_path = session.out_dir.join("reply.json");
    let output: ReplyOutput =
        serde_json::from_slice(&read_output_file(&session.out_dir, &output_path)?)?;

    Ok(ValidatedAgentReply {
        session_id: session.id.clone(),
        agent_logs: Vec::new(),
        message: output.message,
    })
}

pub fn read_validated_run_request(session: &AgentSession) -> AgentResult<ValidatedRunRequest> {
    let output_path = session.out_dir.join("run_request.json");
    let request: RunRequestOutput =
        serde_json::from_slice(&read_output_file(&session.out_dir, &output_path)?)?;

    Ok(ValidatedRunRequest {
        session_id: session.id.clone(),
        request,
    })
}
