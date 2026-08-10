use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};

use crate::{AgentRuntimeIdentity, AgentSession, AgentSkill, OutputContract, TurnMode};

#[async_trait]
pub trait AgentRuntime: Send + Sync {
    fn id(&self) -> &'static str;
    async fn start(&self, session: AgentSession) -> RuntimeResult<RuntimeHandle>;
    async fn send(&self, handle: &RuntimeHandle, turn: AgentTurn) -> RuntimeResult<()>;
    async fn next_event(&self, handle: &RuntimeHandle) -> Option<RuntimeEvent>;
    async fn cancel(&self, handle: &RuntimeHandle) -> RuntimeResult<()>;
}

#[derive(Clone)]
pub struct RuntimeHandle {
    pub runtime_id: String,
    pub session_id: String,
    pub root_dir: PathBuf,
    pub out_dir: PathBuf,
    pub(crate) event_tx: mpsc::Sender<RuntimeEvent>,
    event_rx: Arc<AsyncMutex<mpsc::Receiver<RuntimeEvent>>>,
    pub(crate) cancel_tx: Arc<AsyncMutex<Option<oneshot::Sender<()>>>>,
    identity: Arc<AsyncMutex<Option<AgentRuntimeIdentity>>>,
    resume_thread_id: Arc<AsyncMutex<Option<String>>>,
}

impl RuntimeHandle {
    pub fn new(
        runtime_id: impl Into<String>,
        session_id: impl Into<String>,
        root_dir: PathBuf,
        out_dir: PathBuf,
    ) -> Self {
        let (event_tx, event_rx) = mpsc::channel(64);
        Self {
            runtime_id: runtime_id.into(),
            session_id: session_id.into(),
            root_dir,
            out_dir,
            event_tx,
            event_rx: Arc::new(AsyncMutex::new(event_rx)),
            cancel_tx: Arc::new(AsyncMutex::new(None)),
            identity: Arc::new(AsyncMutex::new(None)),
            resume_thread_id: Arc::new(AsyncMutex::new(None)),
        }
    }

    pub(crate) async fn next_event(&self) -> Option<RuntimeEvent> {
        self.event_rx.lock().await.recv().await
    }

    pub(crate) async fn install_cancel(&self, cancel_tx: oneshot::Sender<()>) {
        *self.cancel_tx.lock().await = Some(cancel_tx);
    }

    pub async fn set_identity(&self, thread_id: String, turn_id: String) {
        *self.identity.lock().await = Some(AgentRuntimeIdentity { thread_id, turn_id });
    }

    pub async fn identity(&self) -> Option<AgentRuntimeIdentity> {
        self.identity.lock().await.clone()
    }

    pub async fn set_resume_thread_id(&self, thread_id: Option<String>) {
        *self.resume_thread_id.lock().await = thread_id;
    }

    pub async fn resume_thread_id(&self) -> Option<String> {
        self.resume_thread_id.lock().await.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTurn {
    pub message: String,
    pub mode: TurnMode,
    pub output_contract: OutputContract,
    pub skill: AgentSkill,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    Status { message: String },
    Failed { message: String },
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env_clear: bool,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct CodexRuntime {
    program: PathBuf,
}

impl CodexRuntime {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn command_spec(&self, session: &AgentSession) -> CommandSpec {
        self.command_spec_for_turn(
            &session.root_dir,
            AgentTurn {
                message: "Read ctx/instructions.md and write the requested artifact under out/."
                    .to_owned(),
                mode: session.mode,
                output_contract: session.output_contract,
                skill: session.mode.agent_skill(),
            },
        )
    }

    fn command_spec_for_turn(&self, root_dir: &Path, turn: AgentTurn) -> CommandSpec {
        let prompt = codex_turn_prompt(&turn);

        CommandSpec {
            program: self.program.clone(),
            args: vec![
                "exec".to_owned(),
                "--json".to_owned(),
                "--sandbox".to_owned(),
                "workspace-write".to_owned(),
                "--skip-git-repo-check".to_owned(),
                "--ignore-user-config".to_owned(),
                "--cd".to_owned(),
                root_dir.display().to_string(),
                "-c".to_owned(),
                "shell_environment_policy.inherit=none".to_owned(),
                prompt,
            ],
            cwd: root_dir.to_path_buf(),
            env_clear: true,
            env: safe_runtime_env(std::env::vars(), root_dir),
        }
    }
}

pub(crate) fn codex_turn_prompt(turn: &AgentTurn) -> String {
    let context_hint = if turn.mode.uses_graph_context() {
        "Read ctx/instructions.md plus declared graph/catalog files under ctx/."
    } else {
        "Read ctx/instructions.md only. Do not inspect graph/catalog files or the filesystem."
    };
    format!(
        "\
{context_hint}
Turn mode: {mode}
Output contract: out/{output_file}
Selected skill: {skill:?}
User turn:
{message}

Write exactly one result file under out/. Do not print secrets or write outside out/.",
        context_hint = context_hint,
        mode = turn.mode,
        output_file = turn.output_contract.file_name(),
        skill = turn.skill,
        message = turn.message
    )
}

#[async_trait]
impl AgentRuntime for CodexRuntime {
    fn id(&self) -> &'static str {
        "codex"
    }

    async fn start(&self, session: AgentSession) -> RuntimeResult<RuntimeHandle> {
        let spec = self.command_spec(&session);
        if spec.program.as_os_str().is_empty() {
            return Err(RuntimeError::InvalidCommand(
                "empty Codex program".to_owned(),
            ));
        }

        Ok(RuntimeHandle::new(
            self.id(),
            session.id,
            session.root_dir,
            session.out_dir,
        ))
    }

    async fn send(&self, handle: &RuntimeHandle, turn: AgentTurn) -> RuntimeResult<()> {
        let spec = self.command_spec_for_turn(&handle.root_dir, turn);
        let sender = handle.event_tx.clone();
        let (cancel_tx, cancel_rx) = oneshot::channel();
        handle.install_cancel(cancel_tx).await;

        tokio::spawn(async move {
            if let Err(err) = run_codex_process(spec, sender.clone(), cancel_rx).await {
                let _ = sender
                    .send(RuntimeEvent::Failed {
                        message: err.to_string(),
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

pub(crate) fn safe_runtime_env(
    source: impl IntoIterator<Item = (String, String)>,
    root_dir: &Path,
) -> BTreeMap<String, String> {
    let source: BTreeMap<String, String> = source.into_iter().collect();
    let mut env = BTreeMap::new();

    if let Some(path) = source.get("PATH") {
        env.insert("PATH".to_owned(), path.clone());
    }
    let codex_home = source
        .get("CODEX_HOME")
        .cloned()
        .or_else(|| source.get("HOME").map(|home| format!("{home}/.codex")));
    if let Some(codex_home) = codex_home {
        env.insert("CODEX_HOME".to_owned(), codex_home);
    }
    if let Some(lang) = source.get("LANG") {
        env.insert("LANG".to_owned(), lang.clone());
    }
    if let Some(locale) = source.get("LC_ALL") {
        env.insert("LC_ALL".to_owned(), locale.clone());
    }

    env.insert("HOME".to_owned(), root_dir.display().to_string());
    env.insert(
        "TMPDIR".to_owned(),
        root_dir.join("tmp").display().to_string(),
    );
    env
}

async fn run_codex_process(
    spec: CommandSpec,
    sender: mpsc::Sender<RuntimeEvent>,
    mut cancel_rx: oneshot::Receiver<()>,
) -> RuntimeResult<()> {
    if spec.program.as_os_str().is_empty() {
        return Err(RuntimeError::InvalidCommand(
            "empty Codex program".to_owned(),
        ));
    }

    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if spec.env_clear {
        command.env_clear();
    }
    command.envs(&spec.env);

    let mut child = command
        .spawn()
        .map_err(|err| RuntimeError::Failed(err.to_string()))?;
    if let Some(stdout) = child.stdout.take() {
        let stdout_sender = sender.clone();
        tokio::spawn(async move {
            read_runtime_stdout(stdout, stdout_sender).await;
        });
    }

    tokio::select! {
        status = child.wait() => {
            let status = status.map_err(|err| RuntimeError::Failed(err.to_string()))?;
            if status.success() {
                sender
                    .send(RuntimeEvent::Finished)
                    .await
                    .map_err(|err| RuntimeError::Failed(err.to_string()))?;
                Ok(())
            } else {
                Err(RuntimeError::Failed(format!("codex exited with status {status}")))
            }
        }
        _ = &mut cancel_rx => {
            child
                .kill()
                .await
                .map_err(|err| RuntimeError::Failed(err.to_string()))?;
            Err(RuntimeError::Failed("codex runtime cancelled".to_owned()))
        }
    }
}

async fn read_runtime_stdout<R>(stdout: R, sender: mpsc::Sender<RuntimeEvent>)
where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(message) = runtime_status_from_line(&line)
            && sender.send(RuntimeEvent::Status { message }).await.is_err()
        {
            break;
        }
    }
}

fn runtime_status_from_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Some(truncate_status(line));
    };
    let event_type = value
        .get("type")
        .or_else(|| value.get("event"))
        .and_then(Value::as_str)
        .unwrap_or("codex.event");
    let message = value
        .get("message")
        .or_else(|| value.get("text"))
        .or_else(|| value.pointer("/item/text"))
        .and_then(Value::as_str)
        .map(truncate_status);

    Some(match message {
        Some(message) if !message.is_empty() => format!("{event_type}: {message}"),
        _ => event_type.to_owned(),
    })
}

fn truncate_status(value: &str) -> String {
    value.chars().take(180).collect()
}

pub type RuntimeResult<T> = Result<T, RuntimeError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    InvalidCommand(String),
    Failed(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCommand(message) => write!(f, "invalid agent runtime command: {message}"),
            Self::Failed(message) => write!(f, "agent runtime failed: {message}"),
        }
    }
}

impl std::error::Error for RuntimeError {}
