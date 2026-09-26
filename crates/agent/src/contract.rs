use serde::{Deserialize, Serialize};

use crate::{AgentError, AgentResult, AgentSession, CanvasEditPlan, TurnMode, read_output_file};

#[derive(Debug, Clone)]
pub struct ValidatedCanvasEdit {
    pub session_id: String,
    pub runtime_identity: Option<AgentRuntimeIdentity>,
    pub agent_logs: Vec<AgentLogEntry>,
    pub edit: CanvasEditPlan,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_ids: Vec<String>,
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

pub fn read_validated_canvas_edit(session: &AgentSession) -> AgentResult<CanvasEditPlan> {
    let output_path = session.out_dir.join("canvas_edit.json");
    let edit: CanvasEditPlan =
        serde_json::from_slice(&read_output_file(&session.out_dir, &output_path)?)?;
    if edit.operations.is_empty() {
        return Err(AgentError::InvalidOutputFile {
            path: output_path,
            reason: "canvas edit has no operations".to_owned(),
        });
    }
    Ok(edit)
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
    path: &std::path::Path,
    field: &str,
    value: &str,
    max_bytes: usize,
) -> AgentResult<()> {
    if value.trim().is_empty() {
        return Err(AgentError::InvalidOutputFile {
            path: path.to_path_buf(),
            reason: format!("{field} must not be empty"),
        });
    }
    if value.len() > max_bytes {
        return Err(AgentError::InvalidOutputFile {
            path: path.to_path_buf(),
            reason: format!("{field} exceeds {max_bytes} bytes"),
        });
    }
    Ok(())
}
