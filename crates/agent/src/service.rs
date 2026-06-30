use std::time::{SystemTime, UNIX_EPOCH};

use helixflow_run::{EventBus, RunEventEnvelope};
use serde_json::{Value, json};

use crate::{
    AgentError, AgentLogEntry, AgentResult, AgentRuntime, AgentSession, AgentSessionRequest,
    AgentTurn, OutputContract, PromptStackMetadata, RuntimeEvent, ValidatedAgentProposal,
    ValidatedAgentReply, create_session_contract, read_validated_proposal, read_validated_reply,
};

#[derive(Debug, Clone)]
pub struct AgentService<R> {
    runtime: R,
    events: EventBus,
}

impl<R> AgentService<R>
where
    R: AgentRuntime,
{
    pub fn new(runtime: R, events: EventBus) -> Self {
        Self { runtime, events }
    }

    pub async fn propose_graph_change(
        &self,
        request: AgentSessionRequest,
    ) -> AgentResult<ValidatedAgentProposal> {
        ensure_output_contract(&request, OutputContract::ProposalJson)?;
        let base_graph = request.graph.clone();
        let current_version_id = request.base_version_id.clone();
        let (session, seq, agent_logs) = self.run_agent_turn(&request).await?;
        let mut proposal = read_validated_proposal(&session, &base_graph, &current_version_id)?;
        proposal.agent_logs = agent_logs;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            seq,
            "agent.status.end",
            json!({ "proposal_title": proposal.proposal.title }),
        );
        Ok(proposal)
    }

    pub async fn answer_chat(
        &self,
        request: AgentSessionRequest,
    ) -> AgentResult<ValidatedAgentReply> {
        ensure_output_contract(&request, OutputContract::ReplyJson)?;
        let (session, seq, agent_logs) = self.run_agent_turn(&request).await?;
        let mut reply = read_validated_reply(&session)?;
        reply.agent_logs = agent_logs;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            seq,
            "agent.status.end",
            json!({ "message": reply.message }),
        );
        Ok(reply)
    }

    async fn run_agent_turn(
        &self,
        request: &AgentSessionRequest,
    ) -> AgentResult<(AgentSession, i64, Vec<AgentLogEntry>)> {
        let turn = AgentTurn {
            message: request.user_message.clone(),
            mode: request.mode,
            output_contract: request.mode.output_contract(),
            skill: request.skill,
        };
        let session = create_session_contract(request)?;
        let mut agent_logs = Vec::new();
        self.emit_status(
            &session.workspace_id,
            &session.id,
            1,
            "ctx.created",
            json!({}),
        );
        agent_logs.push(agent_log_entry("ctx.created", &json!({})));
        agent_logs.push(prompt_metadata_log_entry(&session.prompt_metadata));
        if request.mode.uses_graph_context() {
            agent_logs.push(canvas_ops_log_entry(request));
        }

        let handle = self
            .runtime
            .start(session.clone())
            .await
            .map_err(|err| AgentError::Runtime(err.to_string()))?;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            2,
            "runtime.started",
            json!({}),
        );
        agent_logs.push(agent_log_entry("runtime.started", &json!({})));

        self.runtime
            .send(&handle, turn)
            .await
            .map_err(|err| AgentError::Runtime(err.to_string()))?;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            3,
            "turn.sent",
            json!({}),
        );
        agent_logs.push(agent_log_entry("turn.sent", &json!({})));

        let mut seq = 4;
        while let Some(event) = self.runtime.next_event(&handle).await {
            match event {
                RuntimeEvent::Status { message } => {
                    let detail = json!({ "message": message });
                    self.emit_status(
                        &session.workspace_id,
                        &session.id,
                        seq,
                        "runtime.status",
                        detail.clone(),
                    );
                    agent_logs.push(agent_log_entry("runtime.status", &detail));
                    seq += 1;
                }
                RuntimeEvent::Failed { message } => {
                    self.emit_status(
                        &session.workspace_id,
                        &session.id,
                        seq,
                        "runtime.failed",
                        json!({ "message": message }),
                    );
                    return Err(AgentError::Runtime(message));
                }
                RuntimeEvent::Finished => break,
            }
        }

        Ok((session, seq, agent_logs))
    }

    fn emit_status(
        &self,
        workspace_id: &str,
        session_id: &str,
        seq: i64,
        status: &str,
        detail: Value,
    ) {
        let message_kind = agent_message_kind(status, &detail);
        if let Err(err) = self.events.publish(RunEventEnvelope {
            workspace_id: workspace_id.to_owned(),
            run_id: session_id.to_owned(),
            seq,
            server_time: event_server_time(),
            ev: if status == "agent.status.end" {
                "agent.status.end".to_owned()
            } else {
                "agent.status".to_owned()
            },
            data: json!({
                "session_id": session_id,
                "message_kind": message_kind,
                "status": status,
                "detail": detail
            }),
        }) {
            eprintln!("failed to publish agent event: {err}");
        }
    }
}

fn ensure_output_contract(
    request: &AgentSessionRequest,
    expected: OutputContract,
) -> AgentResult<()> {
    let actual = request.mode.output_contract();
    if actual == expected {
        return Ok(());
    }
    Err(AgentError::InvalidMode {
        mode: request.mode,
        expected,
        actual,
    })
}

fn agent_message_kind(status: &str, detail: &Value) -> &'static str {
    if status.contains("failed") || status.contains("error") {
        return "agent_log:error";
    }
    if status == "agent.status.end" && detail.get("proposal_title").is_some() {
        "proposal_pending"
    } else if status == "agent.status.end" {
        "chat"
    } else {
        "agent_log:status"
    }
}

fn agent_log_entry(status: &str, detail: &Value) -> AgentLogEntry {
    AgentLogEntry {
        kind: agent_message_kind(status, detail).to_owned(),
        text: agent_log_text(status, detail),
    }
}

fn agent_log_text(status: &str, detail: &Value) -> String {
    if let Some(message) = detail.get("message").and_then(Value::as_str)
        && !message.is_empty()
    {
        return message.to_owned();
    }
    status.to_owned()
}

fn prompt_metadata_log_entry(metadata: &PromptStackMetadata) -> AgentLogEntry {
    let sections = metadata
        .sections
        .iter()
        .map(|section| section.key.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    AgentLogEntry {
        kind: "agent_log:status".to_owned(),
        text: format!(
            "Prompt telemetry: mode={}, output_contract={}, sections=[{}]",
            metadata.mode, metadata.output_contract, sections
        ),
    }
}

fn canvas_ops_log_entry(request: &AgentSessionRequest) -> AgentLogEntry {
    let selection_count = request
        .canvas_context
        .as_ref()
        .map(|context| context.selection.node_ids.len())
        .unwrap_or_default();
    AgentLogEntry {
        kind: "agent_log:canvas_ops".to_owned(),
        text: format!(
            "Canvas ops context ready: graph_nodes={}, selected_nodes={}, allowed_ops=[read_state, read_selection, propose_layout, propose_graph_ops, run_selected_workflow]",
            request.graph.nodes.len(),
            selection_count
        ),
    }
}

fn event_server_time() -> String {
    let epoch_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("unix:{epoch_seconds}")
}
