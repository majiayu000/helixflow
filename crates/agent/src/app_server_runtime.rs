use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdout, Command};
use tokio::sync::oneshot;

use crate::runtime::{codex_turn_prompt, safe_runtime_env};
use crate::{
    AgentRuntime, AgentSession, AgentTurn, CodexRuntime, RuntimeError, RuntimeEvent, RuntimeHandle,
    RuntimeResult,
};

#[derive(Debug, Clone)]
pub struct CodexAppServerRuntime {
    program: PathBuf,
}

impl CodexAppServerRuntime {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum CodexBackendRuntime {
    AppServer(CodexAppServerRuntime),
    Exec(CodexRuntime),
}

impl CodexBackendRuntime {
    pub fn app_server(program: impl Into<PathBuf>) -> Self {
        Self::AppServer(CodexAppServerRuntime::new(program))
    }

    pub fn exec(program: impl Into<PathBuf>) -> Self {
        Self::Exec(CodexRuntime::new(program))
    }
}

#[async_trait]
impl AgentRuntime for CodexBackendRuntime {
    fn id(&self) -> &'static str {
        match self {
            Self::AppServer(runtime) => runtime.id(),
            Self::Exec(runtime) => runtime.id(),
        }
    }

    async fn start(&self, session: AgentSession) -> RuntimeResult<RuntimeHandle> {
        match self {
            Self::AppServer(runtime) => runtime.start(session).await,
            Self::Exec(runtime) => runtime.start(session).await,
        }
    }

    async fn send(&self, handle: &RuntimeHandle, turn: AgentTurn) -> RuntimeResult<()> {
        match self {
            Self::AppServer(runtime) => runtime.send(handle, turn).await,
            Self::Exec(runtime) => runtime.send(handle, turn).await,
        }
    }

    async fn next_event(&self, handle: &RuntimeHandle) -> Option<RuntimeEvent> {
        match self {
            Self::AppServer(runtime) => runtime.next_event(handle).await,
            Self::Exec(runtime) => runtime.next_event(handle).await,
        }
    }

    async fn cancel(&self, handle: &RuntimeHandle) -> RuntimeResult<()> {
        match self {
            Self::AppServer(runtime) => runtime.cancel(handle).await,
            Self::Exec(runtime) => runtime.cancel(handle).await,
        }
    }
}

#[async_trait]
impl AgentRuntime for CodexAppServerRuntime {
    fn id(&self) -> &'static str {
        "codex_app_server"
    }

    async fn start(&self, session: AgentSession) -> RuntimeResult<RuntimeHandle> {
        let resume_thread_id = session.codex_thread_id.clone();
        let mut handle =
            RuntimeHandle::new(self.id(), session.id, session.root_dir, session.out_dir);
        handle.output_contract = Some(session.output_contract);
        handle.set_resume_thread_id(resume_thread_id).await;
        Ok(handle)
    }

    async fn send(&self, handle: &RuntimeHandle, turn: AgentTurn) -> RuntimeResult<()> {
        let sender = handle.event_tx.clone();
        let runtime_handle = handle.clone();
        let program = self.program.clone();
        let prompt = codex_turn_prompt(&turn);
        let (cancel_tx, cancel_rx) = oneshot::channel();
        handle.install_cancel(cancel_tx).await;

        tokio::spawn(async move {
            if let Err(error) =
                run_app_server_turn(program, runtime_handle, prompt, cancel_rx).await
            {
                let _ = sender
                    .send(RuntimeEvent::Failed {
                        message: error.to_string(),
                    })
                    .await;
            }
        });
        Ok(())
    }

    async fn next_event(&self, handle: &RuntimeHandle) -> Option<RuntimeEvent> {
        handle.next_event().await
    }

    async fn cancel(&self, handle: &RuntimeHandle) -> RuntimeResult<()> {
        if let Some(cancel_tx) = handle.cancel_tx.lock().await.take() {
            let _ = cancel_tx.send(());
        }
        Ok(())
    }
}

async fn run_app_server_turn(
    program: PathBuf,
    handle: RuntimeHandle,
    prompt: String,
    mut cancel_rx: oneshot::Receiver<()>,
) -> RuntimeResult<()> {
    let mut command = Command::new(program);
    command
        .args([
            "app-server",
            "--stdio",
            "-c",
            "shell_environment_policy.inherit=none",
        ])
        .current_dir(&handle.root_dir)
        .env_clear()
        .envs(safe_runtime_env(std::env::vars(), &handle.root_dir))
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| RuntimeError::Failed(format!("start codex app-server: {error}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| RuntimeError::Failed("codex app-server stdin is unavailable".to_owned()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RuntimeError::Failed("codex app-server stdout is unavailable".to_owned()))?;
    let mut lines = BufReader::new(stdout).lines();

    write_rpc(
        &mut stdin,
        &json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": {
                    "name": "helixflow",
                    "title": "Helixflow",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "experimentalApi": true }
            }
        }),
    )
    .await?;
    await_response(&mut lines, &mut cancel_rx, 1).await?;
    write_rpc(
        &mut stdin,
        &json!({ "method": "initialized", "params": {} }),
    )
    .await?;

    let dynamic_tools = canvas_dynamic_tools(&handle.root_dir, handle.output_contract);
    let thread_request = match handle.resume_thread_id().await {
        Some(thread_id) => json!({
            "method": "thread/resume",
            "id": 2,
            "params": {
                "threadId": thread_id,
                "cwd": handle.root_dir,
                "approvalPolicy": "never",
                "sandbox": "workspace-write",
                "serviceName": "helixflow",
                "dynamicTools": dynamic_tools
            }
        }),
        None => json!({
            "method": "thread/start",
            "id": 2,
            "params": {
                "cwd": handle.root_dir,
                "approvalPolicy": "never",
                "sandbox": "workspace-write",
                "serviceName": "helixflow",
                "dynamicTools": dynamic_tools
            }
        }),
    };
    write_rpc(&mut stdin, &thread_request).await?;
    let thread_response = await_response(&mut lines, &mut cancel_rx, 2).await?;
    let thread_id = required_string(&thread_response, "/result/thread/id")?;

    write_rpc(
        &mut stdin,
        &json!({
            "method": "turn/start",
            "id": 3,
            "params": {
                "threadId": thread_id,
                "input": [{ "type": "text", "text": prompt }],
                "cwd": handle.root_dir,
                "approvalPolicy": "never",
                "sandboxPolicy": {
                    "type": "workspaceWrite",
                    "writableRoots": [handle.root_dir],
                    "networkAccess": false
                }
            }
        }),
    )
    .await?;
    let turn_response = await_response(&mut lines, &mut cancel_rx, 3).await?;
    let turn_id = required_string(&turn_response, "/result/turn/id")?;
    handle
        .set_identity(thread_id.clone(), turn_id.clone())
        .await;
    handle.set_resume_thread_id(Some(thread_id.clone())).await;

    loop {
        let line = next_line(&mut lines, &mut cancel_rx).await?;
        let value: Value = serde_json::from_str(&line)
            .map_err(|error| RuntimeError::Failed(format!("invalid app-server JSON: {error}")))?;
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match method {
            "item/tool/call" => {
                respond_to_dynamic_tool_call(
                    &mut stdin,
                    &value,
                    &handle.root_dir,
                    &handle.out_dir,
                    handle.output_contract,
                    &thread_id,
                    &turn_id,
                )
                .await?;
            }
            "turn/completed" => {
                let completed_turn_id = value
                    .pointer("/params/turn/id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if completed_turn_id != turn_id {
                    continue;
                }
                let status = value
                    .pointer("/params/turn/status")
                    .and_then(Value::as_str)
                    .unwrap_or("failed");
                if status == "completed" {
                    let _ = handle.event_tx.send(RuntimeEvent::Finished).await;
                    let _ = child.kill().await;
                    return Ok(());
                }
                let message = value
                    .pointer("/params/turn/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or(status);
                let _ = child.kill().await;
                return Err(RuntimeError::Failed(format!(
                    "codex app-server turn {status}: {message}"
                )));
            }
            "item/completed" => {
                if let Some(message) = completed_item_status(&value) {
                    let _ = handle.event_tx.send(RuntimeEvent::Status { message }).await;
                }
            }
            "error" => {
                let message = value
                    .pointer("/params/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("codex app-server error");
                let _ = child.kill().await;
                return Err(RuntimeError::Failed(message.to_owned()));
            }
            _ => {}
        }
    }
}

fn canvas_dynamic_tools(
    root_dir: &std::path::Path,
    output_contract: Option<crate::OutputContract>,
) -> Vec<Value> {
    if !root_dir.join("ctx/canvas_state.json").is_file() {
        return Vec::new();
    }
    let mut tools = vec![json!({
        "type": "function",
        "name": "get_state",
        "description": "Return the current compact graph, selection, version, and gate state.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }
    })];
    if output_contract == Some(crate::OutputContract::ProposalJson) {
        tools.push(json!({
            "type": "function",
            "name": "submit_proposal",
            "description": "Submit bounded graph operations for backend validation. This does not apply the proposal.",
            "inputSchema": {
                "type": "object",
                "required": ["base_version_id", "kind", "title", "summary", "ops"],
                "properties": {
                    "base_version_id": { "type": "string" },
                    "kind": { "type": "string", "enum": ["create", "modify", "fix", "sweep"] },
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "ops": { "type": "array", "items": { "type": "object" } },
                    "message_id": { "type": "string" }
                },
                "additionalProperties": false
            }
        }));
    }
    vec![json!({
        "type": "namespace",
        "name": "canvas",
        "description": "Read the current Helixflow canvas and submit validated change proposals.",
        "tools": tools
    })]
}

async fn respond_to_dynamic_tool_call<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request: &Value,
    root_dir: &std::path::Path,
    out_dir: &std::path::Path,
    output_contract: Option<crate::OutputContract>,
    thread_id: &str,
    turn_id: &str,
) -> RuntimeResult<()> {
    let request_id = request
        .get("id")
        .cloned()
        .ok_or_else(|| RuntimeError::Failed("dynamic tool request is missing `id`".to_owned()))?;
    let params = request.get("params").ok_or_else(|| {
        RuntimeError::Failed("dynamic tool request is missing `params`".to_owned())
    })?;
    let matches_turn = params.get("threadId").and_then(Value::as_str) == Some(thread_id)
        && params.get("turnId").and_then(Value::as_str) == Some(turn_id);
    let namespace = params.get("namespace").and_then(Value::as_str);
    let tool = params.get("tool").and_then(Value::as_str);
    let result = match (matches_turn, namespace, tool) {
        (true, Some("canvas"), Some("get_state")) => canvas_state_tool_result(root_dir).await,
        (true, Some("canvas"), Some("submit_proposal"))
            if output_contract == Some(crate::OutputContract::ProposalJson) =>
        {
            submit_proposal_tool_result(out_dir, &params["arguments"]).await
        }
        _ => failed_tool_result("Unsupported or stale Helixflow dynamic tool call."),
    };
    write_rpc(writer, &json!({ "id": request_id, "result": result })).await
}

async fn submit_proposal_tool_result(out_dir: &std::path::Path, arguments: &Value) -> Value {
    const MAX_PROPOSAL_BYTES: usize = 256 * 1024;
    let bytes = match serde_json::to_vec(arguments) {
        Ok(bytes) if arguments.is_object() && bytes.len() <= MAX_PROPOSAL_BYTES => bytes,
        Ok(_) => {
            return failed_tool_result(
                "Proposal arguments must be an object no larger than 256 KiB.",
            );
        }
        Err(error) => {
            return failed_tool_result(&format!("Proposal arguments are invalid: {error}"));
        }
    };
    let path = out_dir.join("proposal.json");
    let write = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await?;
        file.write_all(&bytes).await?;
        file.flush().await
    }
    .await;
    match write {
        Ok(()) => json!({
            "contentItems": [{
                "type": "inputText",
                "text": "Proposal captured for backend validation; it has not been applied."
            }],
            "success": true
        }),
        Err(error) => failed_tool_result(&format!("Proposal could not be captured: {error}")),
    }
}

fn failed_tool_result(message: &str) -> Value {
    json!({
        "contentItems": [{ "type": "inputText", "text": message }],
        "success": false
    })
}

async fn canvas_state_tool_result(root_dir: &std::path::Path) -> Value {
    const MAX_CANVAS_STATE_BYTES: u64 = 256 * 1024;
    let path = root_dir.join("ctx/canvas_state.json");
    let read = async {
        let metadata = tokio::fs::metadata(&path).await?;
        if metadata.len() > MAX_CANVAS_STATE_BYTES {
            return Err(std::io::Error::other("canvas state exceeds 256 KiB"));
        }
        let bytes = tokio::fs::read(&path).await?;
        let value: Value = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        serde_json::to_string(&value).map_err(std::io::Error::other)
    }
    .await;
    match read {
        Ok(text) => json!({
            "contentItems": [{ "type": "inputText", "text": text }],
            "success": true
        }),
        Err(error) => json!({
            "contentItems": [{
                "type": "inputText",
                "text": format!("Canvas state is unavailable: {error}")
            }],
            "success": false
        }),
    }
}

async fn write_rpc<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> RuntimeResult<()> {
    writer
        .write_all(format!("{value}\n").as_bytes())
        .await
        .map_err(|error| RuntimeError::Failed(format!("write app-server request: {error}")))?;
    writer
        .flush()
        .await
        .map_err(|error| RuntimeError::Failed(format!("flush app-server request: {error}")))
}

async fn await_response(
    lines: &mut Lines<BufReader<ChildStdout>>,
    cancel_rx: &mut oneshot::Receiver<()>,
    id: i64,
) -> RuntimeResult<Value> {
    loop {
        let line = next_line(lines, cancel_rx).await?;
        let value: Value = serde_json::from_str(&line)
            .map_err(|error| RuntimeError::Failed(format!("invalid app-server JSON: {error}")))?;
        if value.get("id").and_then(Value::as_i64) != Some(id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(RuntimeError::Failed(format!(
                "app-server request {id} failed: {error}"
            )));
        }
        return Ok(value);
    }
}

async fn next_line(
    lines: &mut Lines<BufReader<ChildStdout>>,
    cancel_rx: &mut oneshot::Receiver<()>,
) -> RuntimeResult<String> {
    tokio::select! {
        line = lines.next_line() => line
            .map_err(|error| RuntimeError::Failed(format!("read app-server response: {error}")))?
            .ok_or_else(|| RuntimeError::Failed("codex app-server closed before turn completion".to_owned())),
        _ = cancel_rx => Err(RuntimeError::Failed("codex app-server turn cancelled".to_owned())),
    }
}

fn required_string(value: &Value, pointer: &str) -> RuntimeResult<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| RuntimeError::Failed(format!("app-server response missing `{pointer}`")))
}

fn completed_item_status(value: &Value) -> Option<String> {
    let item = value.pointer("/params/item")?;
    let item_type = item.get("type")?.as_str()?;
    let summary = match item_type {
        "agentMessage" => item.get("text").and_then(Value::as_str),
        "commandExecution" => item.get("command").and_then(Value::as_str),
        "mcpToolCall" | "dynamicToolCall" => item.get("tool").and_then(Value::as_str),
        _ => None,
    };
    Some(match summary {
        Some(summary) => format!(
            "{item_type}: {}",
            summary.chars().take(180).collect::<String>()
        ),
        None => item_type.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};

    use super::{
        canvas_dynamic_tools, canvas_state_tool_result, completed_item_status, required_string,
        submit_proposal_tool_result,
    };

    #[test]
    fn parses_thread_identity_and_completed_items() {
        let response = json!({ "result": { "thread": { "id": "thr_1" } } });
        assert_eq!(
            required_string(&response, "/result/thread/id").unwrap(),
            "thr_1"
        );
        assert_eq!(
            completed_item_status(&json!({
                "params": { "item": { "type": "dynamicToolCall", "tool": "canvas.get_state" } }
            }))
            .as_deref(),
            Some("dynamicToolCall: canvas.get_state")
        );
    }

    #[tokio::test]
    async fn exposes_bounded_canvas_state_as_a_dynamic_tool() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::create_dir(dir.path().join("ctx")).expect("ctx dir");
        fs::write(
            dir.path().join("ctx/canvas_state.json"),
            r#"{ "workspace_id": "ws_1", "graph": { "node_count": 1 } }"#,
        )
        .expect("canvas state");

        let tools = canvas_dynamic_tools(dir.path(), Some(crate::OutputContract::ProposalJson));
        assert_eq!(tools[0]["name"], "canvas");
        assert_eq!(tools[0]["tools"][0]["name"], "get_state");
        assert_eq!(tools[0]["tools"][1]["name"], "submit_proposal");
        let result = canvas_state_tool_result(dir.path()).await;
        assert_eq!(result["success"], true);
        assert!(
            result["contentItems"][0]["text"]
                .as_str()
                .expect("text")
                .contains("ws_1")
        );
    }

    #[tokio::test]
    async fn captures_proposals_without_applying_them() {
        let dir = tempfile::tempdir().expect("temp dir");
        let out_dir = dir.path().join("out");
        fs::create_dir(&out_dir).expect("out dir");
        let proposal = json!({
            "base_version_id": "ver_1",
            "kind": "modify",
            "title": "Move node",
            "summary": "Move one node.",
            "ops": [{ "op": "move_node", "id": "video", "pos": [10, 20] }]
        });

        let result = submit_proposal_tool_result(&out_dir, &proposal).await;

        assert_eq!(result["success"], true);
        let captured: Value = serde_json::from_slice(
            &fs::read(out_dir.join("proposal.json")).expect("captured proposal"),
        )
        .expect("proposal json");
        assert_eq!(captured, proposal);
    }
}
