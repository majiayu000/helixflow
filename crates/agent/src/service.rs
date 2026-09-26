use std::fs;
use std::io::ErrorKind;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use helixflow_run::{EventBus, RunEventEnvelope};
use serde_json::{Value, json};

use crate::{
    AgentError, AgentLogEntry, AgentResult, AgentRuntime, AgentSession, AgentSessionRequest,
    AgentTurn, OutputContract, PromptStackMetadata, RuntimeEvent, RuntimeHandle,
    TurnClassification, TurnMode, TurnModeSource, ValidatedAgentReply, ValidatedCanvasEdit,
    create_session_contract, read_validated_canvas_edit, read_validated_reply,
    read_validated_route,
};

const DEFAULT_MAX_INTENT_ROUNDS: usize = 3;
const DEFAULT_TURN_TIMEOUT: Duration = Duration::from_secs(300);
pub(crate) const MAX_INTENT_ROUNDS: usize = 10;

#[derive(Debug, Clone)]
pub struct AgentService<R> {
    runtime: R,
    events: EventBus,
    max_intent_rounds: usize,
    turn_timeout: Duration,
}

impl<R> AgentService<R>
where
    R: AgentRuntime,
{
    pub async fn route_turn(&self, request: AgentSessionRequest) -> AgentResult<TurnClassification>
    where
        R: Clone + 'static,
    {
        ensure_output_contract(&request, OutputContract::RouteJson)?;
        let mut run = self.start_agent_session(&request).await?;
        let mut cancellation = RuntimeCancellationGuard::new(self.runtime.clone(), &run.handle);
        let turn = AgentTurn {
            message: request.user_message.clone(),
            mode: request.mode,
            output_contract: OutputContract::RouteJson,
            skill: request.skill,
        };
        let outcome = self.send_agent_turn(&mut run, turn).await?;
        cancellation.disarm();
        if let TurnRunOutcome::RuntimeFailed(message) = outcome {
            return Err(AgentError::Runtime(message));
        }
        let route = read_validated_route(&run.session)?;
        Ok(TurnClassification {
            mode: route.mode,
            source: TurnModeSource::Model,
        })
    }

    pub fn new(runtime: R, events: EventBus) -> Self {
        Self {
            runtime,
            events,
            max_intent_rounds: DEFAULT_MAX_INTENT_ROUNDS,
            turn_timeout: DEFAULT_TURN_TIMEOUT,
        }
    }

    pub fn with_max_intent_rounds(mut self, max_intent_rounds: usize) -> Self {
        self.max_intent_rounds = max_intent_rounds.clamp(1, MAX_INTENT_ROUNDS);
        self
    }

    pub fn with_turn_timeout(mut self, turn_timeout: Duration) -> Self {
        self.turn_timeout = turn_timeout.max(Duration::from_millis(1));
        self
    }

    /// Runs the canvas-edit contract with a bounded retry loop.
    pub async fn propose_canvas_edit(
        &self,
        request: AgentSessionRequest,
    ) -> AgentResult<ValidatedCanvasEdit> {
        ensure_output_contract(&request, OutputContract::CanvasEditJson)?;
        let mut run = self.start_agent_session(&request).await?;
        let max_rounds = self.max_intent_rounds.max(1);
        let mut next_message = request.user_message.clone();
        let mut last_error;

        for round in 1..=max_rounds {
            self.record_status(
                &mut run,
                "canvas_edit.round.started",
                json!({
                    "round": round,
                    "max_rounds": max_rounds,
                    "message": format!("canvas edit round {round}/{max_rounds} started")
                }),
            );

            let turn = AgentTurn {
                message: next_message.clone(),
                mode: request.mode,
                output_contract: OutputContract::CanvasEditJson,
                skill: request.skill,
            };
            let outcome = self.send_agent_turn(&mut run, turn).await?;

            if let TurnRunOutcome::RuntimeFailed(message) = outcome {
                last_error = sanitize_retry_feedback(&message);
            } else {
                match read_validated_canvas_edit(&run.session) {
                    Ok(edit) => {
                        self.record_status(
                            &mut run,
                            "canvas_edit.ready",
                            json!({
                                "round": round,
                                "operations": edit.operations.len(),
                                "message": format!("canvas edit ready after round {round}")
                            }),
                        );
                        self.emit_status(
                            &run.session,
                            run.seq,
                            "agent.status.end",
                            json!({ "canvas_edit_ops": edit.operations.len() }),
                        );
                        return Ok(ValidatedCanvasEdit {
                            session_id: run.session.id.clone(),
                            runtime_identity: run.handle.identity().await,
                            agent_logs: run.agent_logs,
                            edit,
                        });
                    }
                    Err(err) => {
                        last_error = sanitize_retry_feedback(&err.to_string());
                        self.record_status(
                            &mut run,
                            "canvas_edit.validation_failed",
                            json!({
                                "round": round,
                                "max_rounds": max_rounds,
                                "message": format!("canvas edit validation failed: {last_error}")
                            }),
                        );
                    }
                }
            }

            if round == max_rounds {
                return Err(AgentError::IntentRetryExhausted {
                    rounds: max_rounds,
                    last_error,
                });
            }
            next_message = format!(
                "Your previous out/canvas_edit.json was invalid: {last_error}\nRewrite out/canvas_edit.json using canvas.edit operations only. Original request: {}",
                request.user_message
            );
        }
        unreachable!("canvas edit rounds always return or error")
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
        reply.runtime_identity = run.handle.identity().await;
        self.emit_status(
            &run.session,
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
        if request.mode.uses_canvas_context() {
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
        self.emit_status(&run.session, run.seq, status, detail.clone());
        run.agent_logs.push(agent_log_entry(status, &detail));
        run.seq += 1;
    }

    fn emit_status(&self, session: &AgentSession, seq: i64, status: &str, detail: Value) {
        if session.mode == TurnMode::Route {
            return;
        }
        let message_kind = agent_message_kind(status, &detail);
        if let Err(err) = self.events.publish(RunEventEnvelope {
            workspace_id: session.workspace_id.clone(),
            run_id: session.id.clone(),
            seq,
            server_time: event_server_time(),
            ev: if status == "agent.status.end" {
                "agent.status.end".to_owned()
            } else {
                "agent.status".to_owned()
            },
            data: json!({
                "session_id": session.id,
                "conversation_id": session.conversation_id,
                "turn_id": session.durable_turn_id,
                "message_kind": message_kind,
                "status": status,
                "detail": detail
            }),
        }) {
            eprintln!("failed to publish agent event: {err}");
        }
    }
}

struct RuntimeCancellationGuard<R>
where
    R: AgentRuntime + Clone + 'static,
{
    runtime: R,
    handle: RuntimeHandle,
    armed: bool,
}

impl<R> RuntimeCancellationGuard<R>
where
    R: AgentRuntime + Clone + 'static,
{
    fn new(runtime: R, handle: &RuntimeHandle) -> Self {
        Self {
            runtime,
            handle: handle.clone(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl<R> Drop for RuntimeCancellationGuard<R>
where
    R: AgentRuntime + Clone + 'static,
{
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let runtime = self.runtime.clone();
        let handle = self.handle.clone();
        if let Ok(tokio_runtime) = tokio::runtime::Handle::try_current() {
            tokio_runtime.spawn(async move {
                if let Err(error) = runtime.cancel(&handle).await {
                    eprintln!("failed to cancel dropped Agent routing turn: {error}");
                }
            });
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

fn agent_message_kind(status: &str, _detail: &Value) -> &'static str {
    if status.contains("failed") || status.contains("error") {
        return "agent_log:error";
    }
    if status == "agent.status.end" {
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
            "Canvas ops context ready: graph_nodes={}, selected_nodes={}, allowed_ops=[catalog, inspect, edit, run, wait]",
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
