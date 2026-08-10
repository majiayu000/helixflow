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
        let handle = RuntimeHandle::new(self.id(), session.id, session.root_dir, session.out_dir);
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
                }
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

    let thread_request = match handle.resume_thread_id().await {
        Some(thread_id) => json!({
            "method": "thread/resume",
            "id": 2,
            "params": {
                "threadId": thread_id,
                "cwd": handle.root_dir,
                "approvalPolicy": "never",
                "sandbox": "workspace-write",
                "serviceName": "helixflow"
            }
        }),
        None => json!({
            "method": "thread/start",
            "id": 2,
            "params": {
                "cwd": handle.root_dir,
                "approvalPolicy": "never",
                "sandbox": "workspace-write",
                "serviceName": "helixflow"
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
    use serde_json::json;

    use super::{completed_item_status, required_string};

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
}
