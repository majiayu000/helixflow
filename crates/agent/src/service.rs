use std::fs;
use std::io::ErrorKind;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use helixflow_run::{EventBus, RunEventEnvelope};
use serde_json::{Value, json};

use crate::{
    AgentError, AgentLogEntry, AgentResult, AgentRuntime, AgentSession, AgentSessionRequest,
    AgentTurn, OutputContract, PromptStackMetadata, RuntimeEvent, RuntimeHandle,
    ValidatedAgentIntent, ValidatedAgentProposal, ValidatedAgentReply, create_session_contract,
    read_validated_intent, read_validated_proposal, read_validated_reply,
};

const DEFAULT_MAX_PROPOSAL_ROUNDS: usize = 3;
const DEFAULT_TURN_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const MAX_PROPOSAL_ROUNDS: usize = 10;

#[derive(Debug, Clone)]
pub struct AgentService<R> {
    runtime: R,
    events: EventBus,
    max_proposal_rounds: usize,
    turn_timeout: Duration,
}

impl<R> AgentService<R>
where
    R: AgentRuntime,
{
    pub fn new(runtime: R, events: EventBus) -> Self {
        Self {
            runtime,
            events,
            max_proposal_rounds: DEFAULT_MAX_PROPOSAL_ROUNDS,
            turn_timeout: DEFAULT_TURN_TIMEOUT,
        }
    }

    pub fn with_max_proposal_rounds(mut self, max_proposal_rounds: usize) -> Self {
        self.max_proposal_rounds = max_proposal_rounds.clamp(1, MAX_PROPOSAL_ROUNDS);
        self
    }

    pub fn with_turn_timeout(mut self, turn_timeout: Duration) -> Self {
        self.turn_timeout = turn_timeout.max(Duration::from_millis(1));
        self
    }

    /// GH130 T6: the IntentPlan contract. Same bounded retry loop as the
    /// proposal path, but validation errors come from the intent schema and
    /// structural validator instead of graph preview.
    pub async fn propose_intent(
        &self,
        request: AgentSessionRequest,
    ) -> AgentResult<ValidatedAgentIntent> {
        ensure_output_contract(&request, OutputContract::IntentJson)?;
        let mut run = self.start_agent_session(&request).await?;
        let max_rounds = self.max_proposal_rounds.max(1);
        let mut next_message = request.user_message.clone();
        let mut last_error;

        for round in 1..=max_rounds {
            self.record_status(
                &mut run,
                "intent.round.started",
                json!({
                    "round": round,
                    "max_rounds": max_rounds,
                    "message": format!("intent round {round}/{max_rounds} started")
                }),
            );

            let turn = AgentTurn {
                message: next_message.clone(),
                mode: request.mode,
                output_contract: OutputContract::IntentJson,
                skill: request.skill,
            };
            let outcome = self.send_agent_turn(&mut run, turn).await?;

            if let TurnRunOutcome::RuntimeFailed(message) = outcome {
                last_error = sanitize_retry_feedback(&message);
            } else {
                match read_validated_intent(&run.session) {
                    Ok(intent) => {
                        self.record_status(
                            &mut run,
                            "intent.ready",
                            json!({
                                "round": round,
                                "stages": intent.stages.len(),
                                "message": format!("intent ready after round {round}")
                            }),
                        );
                        self.emit_status(
                            &run.session.workspace_id,
                            &run.session.id,
                            run.seq,
                            "agent.status.end",
                            json!({ "intent_stages": intent.stages.len() }),
                        );
                        return Ok(ValidatedAgentIntent {
                            session_id: run.session.id.clone(),
                            agent_logs: run.agent_logs,
                            intent,
                        });
                    }
                    Err(err) => {
                        last_error = sanitize_retry_feedback(&err.to_string());
                        self.record_status(
                            &mut run,
                            "intent.validation_failed",
                            json!({
                                "round": round,
                                "max_rounds": max_rounds,
                                "message": format!("intent validation failed: {last_error}")
                            }),
                        );
                    }
                }
            }

            if round == max_rounds {
                return Err(AgentError::ProposalRetryExhausted {
                    rounds: max_rounds,
                    last_error,
                });
            }
            next_message = format!(
                "Your previous out/intent.json was invalid: {last_error}\nRewrite out/intent.json following the Intent output contract exactly. Original request: {}",
                request.user_message
            );
        }
        unreachable!("intent rounds always return or error")
    }

    pub async fn propose_graph_change(
        &self,
        request: AgentSessionRequest,
    ) -> AgentResult<ValidatedAgentProposal> {
        ensure_output_contract(&request, OutputContract::ProposalJson)?;
        let base_graph = request.graph.clone();
        let current_version_id = request.base_version_id.clone();
        let mut run = self.start_agent_session(&request).await?;
        let max_rounds = self.max_proposal_rounds.max(1);
        let mut next_message = request.user_message.clone();
        let mut last_error = String::new();
        let mut last_summary: String;

        for round in 1..=max_rounds {
            self.record_status(
                &mut run,
                "proposal.round.started",
                json!({
                    "round": round,
                    "max_rounds": max_rounds,
                    "message": format!("proposal round {round}/{max_rounds} started")
                }),
            );

            let turn = AgentTurn {
                message: next_message.clone(),
                mode: request.mode,
                output_contract: request
                    .mode
                    .output_contract_with(request.use_intent_contract),
                skill: request.skill,
            };
            let outcome = self.send_agent_turn(&mut run, turn).await?;

            if let TurnRunOutcome::RuntimeFailed(message) = outcome {
                last_error = sanitize_retry_feedback(&message);
                last_summary = "runtime failed before producing a valid proposal".to_owned();
            } else {
                match read_validated_proposal(&run.session, &base_graph, &current_version_id) {
                    Ok(mut proposal) => {
                        self.record_status(
                            &mut run,
                            "proposal.ready",
                            json!({
                                "round": round,
                                "proposal_title": proposal.proposal.title,
                                "message": format!("proposal ready after round {round}")
                            }),
                        );
                        proposal.agent_logs = run.agent_logs;
                        self.emit_status(
                            &run.session.workspace_id,
                            &run.session.id,
                            run.seq,
                            "agent.status.end",
                            json!({ "proposal_title": proposal.proposal.title }),
                        );
                        return Ok(proposal);
                    }
                    Err(err) => {
                        last_error = sanitize_retry_feedback(&err.to_string());
                        last_summary = summarize_last_proposal(&run.session);
                        self.record_status(
                            &mut run,
                            "proposal.validation_failed",
                            json!({
                                "round": round,
                                "max_rounds": max_rounds,
                                "message": format!("proposal validation failed: {last_error}")
                            }),
                        );
                    }
                }
            }

            if round == max_rounds {
                self.record_status(
                    &mut run,
                    "proposal.retry_exhausted",
                    json!({
                        "rounds": max_rounds,
                        "last_error": last_error.clone(),
                        "last_summary": last_summary.clone(),
                        "message": format!(
                            "proposal retry exhausted after {max_rounds} rounds: {last_error}"
                        )
                    }),
                );
                return Err(AgentError::ProposalRetryExhausted {
                    rounds: max_rounds,
                    last_error,
                });
            }

            self.record_status(
                &mut run,
                "proposal.retrying",
                json!({
                    "round": round,
                    "next_round": round + 1,
                    "max_rounds": max_rounds,
                    "last_error": last_error.clone(),
                    "last_summary": last_summary.clone(),
                    "message": format!(
                        "retrying proposal generation (round {}/{max_rounds})",
                        round + 1
                    )
                }),
            );
            next_message =
                build_retry_message(&request, round + 1, max_rounds, &last_error, &last_summary);
        }

        Err(AgentError::ProposalRetryExhausted {
            rounds: max_rounds,
            last_error,
        })
    }

    pub async fn answer_chat(
        &self,
        request: AgentSessionRequest,
    ) -> AgentResult<ValidatedAgentReply> {
        ensure_output_contract(&request, OutputContract::ReplyJson)?;
        let mut run = self.start_agent_session(&request).await?;
        let turn = AgentTurn {
            message: request.user_message.clone(),
            mode: request.mode,
            output_contract: request.mode.output_contract(),
            skill: request.skill,
        };
        if let TurnRunOutcome::RuntimeFailed(message) = self.send_agent_turn(&mut run, turn).await?
        {
            return Err(AgentError::Runtime(message));
        }
        let mut reply = read_validated_reply(&run.session)?;
        reply.agent_logs = run.agent_logs;
        self.emit_status(
            &run.session.workspace_id,
            &run.session.id,
            run.seq,
            "agent.status.end",
            json!({ "message": reply.message }),
        );
        Ok(reply)
    }

    async fn start_agent_session(
        &self,
        request: &AgentSessionRequest,
    ) -> AgentResult<AgentRunState> {
        let session = create_session_contract(request)?;
        let mut run = AgentRunState {
            handle: self
                .runtime
                .start(session.clone())
                .await
                .map_err(|err| AgentError::Runtime(err.to_string()))?,
            session,
            seq: 1,
            agent_logs: Vec::new(),
        };

        self.record_status(&mut run, "ctx.created", json!({}));
        run.agent_logs
            .push(prompt_metadata_log_entry(&run.session.prompt_metadata));
        if request.mode.uses_graph_context() {
            run.agent_logs.push(canvas_ops_log_entry(request));
        }
        self.record_status(&mut run, "runtime.started", json!({}));

        Ok(run)
    }

    async fn send_agent_turn(
        &self,
        run: &mut AgentRunState,
        turn: AgentTurn,
    ) -> AgentResult<TurnRunOutcome> {
        clear_turn_output(&run.session, turn.output_contract)?;
        self.runtime
            .send(&run.handle, turn)
            .await
            .map_err(|err| AgentError::Runtime(err.to_string()))?;
        self.record_status(run, "turn.sent", json!({}));

        let outcome = tokio::time::timeout(self.turn_timeout, async {
            while let Some(event) = self.runtime.next_event(&run.handle).await {
                match event {
                    RuntimeEvent::Status { message } => {
                        self.record_status(run, "runtime.status", json!({ "message": message }));
                    }
                    RuntimeEvent::Failed { message } => {
                        self.record_status(
                            run,
                            "runtime.failed",
                            json!({ "message": message.clone() }),
                        );
                        return TurnRunOutcome::RuntimeFailed(message);
                    }
                    RuntimeEvent::Finished => return TurnRunOutcome::Finished,
                }
            }
            TurnRunOutcome::Finished
        })
        .await;

        match outcome {
            Ok(outcome) => Ok(outcome),
            Err(_) => {
                self.runtime
                    .cancel(&run.handle)
                    .await
                    .map_err(|error| AgentError::Runtime(error.to_string()))?;
                let message = format!(
                    "agent turn timed out after {} seconds",
                    self.turn_timeout.as_secs_f64()
                );
                self.record_status(
                    run,
                    "runtime.timeout",
                    json!({ "message": message.clone() }),
                );
                Ok(TurnRunOutcome::RuntimeFailed(message))
            }
        }
    }

    fn record_status(&self, run: &mut AgentRunState, status: &str, detail: Value) {
        self.emit_status(
            &run.session.workspace_id,
            &run.session.id,
            run.seq,
            status,
            detail.clone(),
        );
        run.agent_logs.push(agent_log_entry(status, &detail));
        run.seq += 1;
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

struct AgentRunState {
    session: AgentSession,
    handle: RuntimeHandle,
    seq: i64,
    agent_logs: Vec<AgentLogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TurnRunOutcome {
    Finished,
    RuntimeFailed(String),
}

fn clear_turn_output(session: &AgentSession, output_contract: OutputContract) -> AgentResult<()> {
    let path = session.out_dir.join(output_contract.file_name());
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(AgentError::Io(err)),
    }
}

fn build_retry_message(
    request: &AgentSessionRequest,
    round: usize,
    max_rounds: usize,
    last_error: &str,
    last_summary: &str,
) -> String {
    let graph_summary = format!(
        "workspace_id={}, base_version_id={}, node_count={}, edge_count={}",
        request.workspace_id,
        request.base_version_id,
        request.graph.nodes.len(),
        request.graph.edges.len()
    );

    format!(
        "\
{original_message}

Previous proposal validation failed.
Retry round: {round}/{max_rounds}
Current graph context: {graph_summary}. Re-read ctx/graph.json, ctx/canvas_state.json, ctx/node_defs/catalog.json, and ctx/canvas_ops.json.
Validator error: {last_error}
Last failed proposal summary: {last_summary}
Write a fresh out/proposal.json that satisfies the proposal_json contract. Keep the same user intent, correct only the invalid proposal shape or ops, and do not include secrets, local paths, or extra fields.",
        original_message = request.user_message.as_str(),
    )
}

fn summarize_last_proposal(session: &AgentSession) -> String {
    let path = session
        .out_dir
        .join(OutputContract::ProposalJson.file_name());
    let Ok(bytes) = fs::read(path) else {
        return "proposal output was not readable".to_owned();
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return "proposal output was not valid JSON".to_owned();
    };

    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("untitled");
    let summary = value
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("missing summary");
    let op_count = value
        .get("ops")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    sanitize_retry_feedback(&format!("title={title}; summary={summary}; ops={op_count}"))
}

fn sanitize_retry_feedback(message: &str) -> String {
    let mut sanitized = message
        .split_whitespace()
        .map(sanitize_feedback_token)
        .collect::<Vec<_>>()
        .join(" ");

    if sanitized.len() > 1200 {
        sanitized.truncate(1200);
        sanitized.push_str("...");
    }

    sanitized
}

fn sanitize_feedback_token(token: &str) -> String {
    let path_trimmed = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '`' | ',' | ':' | ';' | '(' | ')' | '[' | ']'
        )
    });
    if looks_like_local_path(path_trimmed) {
        return token.replace(path_trimmed, "[redacted-path]");
    }

    let lower = token.to_ascii_lowercase();
    if lower.starts_with("bearer ")
        || lower.starts_with("sk-")
        || lower.contains("secret=")
        || lower.contains("token=")
        || lower.contains("authorization:")
    {
        return "[redacted-secret]".to_owned();
    }

    token.to_owned()
}

fn looks_like_local_path(value: &str) -> bool {
    value.starts_with("/Users/")
        || value.starts_with("/private/")
        || value.starts_with("/tmp/")
        || value.starts_with("/var/")
        || value.starts_with("file://")
}

fn ensure_output_contract(
    request: &AgentSessionRequest,
    expected: OutputContract,
) -> AgentResult<()> {
    let actual = request
        .mode
        .output_contract_with(request.use_intent_contract);
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
