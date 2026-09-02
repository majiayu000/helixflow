use helixflow_compiler::IntentPlan;
use serde::{Deserialize, Serialize};

use crate::{AgentError, AgentResult, AgentSession, TurnMode, read_output_file};

#[derive(Debug, Clone)]
pub struct ValidatedAgentIntent {
    pub session_id: String,
    pub runtime_identity: Option<AgentRuntimeIdentity>,
    pub agent_logs: Vec<AgentLogEntry>,
    pub intent: IntentPlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedAgentReply {
    pub session_id: String,
    pub runtime_identity: Option<AgentRuntimeIdentity>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeIdentity {
    pub thread_id: String,
    pub turn_id: String,
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
struct ReplyOutput {
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidatedRoute {
    pub mode: TurnMode,
    pub requested_action: String,
}

/// Reads and validates `out/intent.json`. The IntentPlan schema rejects
/// unknown fields and the structural validator rejects topology and reference
/// errors before anything reaches the compiler.
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
    validate_text_field(&output_path, "message", &output.message, 64 * 1024)?;

    Ok(ValidatedAgentReply {
        session_id: session.id.clone(),
        runtime_identity: None,
        agent_logs: Vec::new(),
        message: output.message,
    })
}

pub fn read_validated_route(session: &AgentSession) -> AgentResult<ValidatedRoute> {
    let output_path = session.out_dir.join("route.json");
    let output: ValidatedRoute =
        serde_json::from_slice(&read_output_file(&session.out_dir, &output_path)?)?;
    validate_text_field(
        &output_path,
        "requestedAction",
        &output.requested_action,
        1024,
    )?;
    Ok(output)
}

pub fn read_validated_run_request(session: &AgentSession) -> AgentResult<ValidatedRunRequest> {
    let output_path = session.out_dir.join("run_request.json");
    let request: RunRequestOutput =
        serde_json::from_slice(&read_output_file(&session.out_dir, &output_path)?)?;
    validate_text_field(&output_path, "summary", &request.summary, 4 * 1024)?;

    Ok(ValidatedRunRequest {
        session_id: session.id.clone(),
        request,
    })
}

fn validate_text_field(
    output_path: &std::path::Path,
    field: &str,
    value: &str,
    max_chars: usize,
) -> AgentResult<()> {
    let chars = value.chars().count();
    if value.trim().is_empty() || chars > max_chars {
        return Err(AgentError::InvalidOutputFile {
            path: output_path.to_path_buf(),
            reason: format!("`{field}` must contain 1 to {max_chars} characters"),
        });
    }
    Ok(())
}
