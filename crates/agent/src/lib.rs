use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use helixflow_graph::{
    GraphService, PreparedProposal, ProposalDraft, ProposalKind, ProposalOp, WorkflowGraph,
};
use helixflow_registry::NodeRegistry;
use helixflow_run::{EventBus, RunEventEnvelope};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use uuid::Uuid;

pub fn module_name() -> &'static str {
    "agent"
}

#[async_trait]
pub trait AgentRuntime: Send + Sync {
    fn id(&self) -> &'static str;
    async fn start(&self, session: AgentSession) -> RuntimeResult<RuntimeHandle>;
    async fn send(&self, handle: &RuntimeHandle, turn: AgentTurn) -> RuntimeResult<()>;
    async fn next_event(&self, handle: &RuntimeHandle) -> Option<RuntimeEvent>;
    async fn cancel(&self, handle: &RuntimeHandle) -> RuntimeResult<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSession {
    pub id: String,
    pub workspace_id: String,
    pub root_dir: PathBuf,
    pub ctx_dir: PathBuf,
    pub out_dir: PathBuf,
    pub base_version_id: String,
}

#[derive(Debug, Clone)]
pub struct AgentSessionRequest {
    pub workspace_id: String,
    pub base_version_id: String,
    pub user_message: String,
    pub graph: WorkflowGraph,
    pub sessions_dir: PathBuf,
    pub skill: AgentSkill,
}

#[derive(Clone)]
pub struct RuntimeHandle {
    pub runtime_id: String,
    pub session_id: String,
    pub root_dir: PathBuf,
    pub out_dir: PathBuf,
    event_tx: mpsc::Sender<RuntimeEvent>,
    event_rx: Arc<AsyncMutex<mpsc::Receiver<RuntimeEvent>>>,
    cancel_tx: Arc<AsyncMutex<Option<oneshot::Sender<()>>>>,
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
        }
    }

    async fn next_event(&self) -> Option<RuntimeEvent> {
        self.event_rx.lock().await.recv().await
    }

    async fn install_cancel(&self, cancel_tx: oneshot::Sender<()>) {
        *self.cancel_tx.lock().await = Some(cancel_tx);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTurn {
    pub message: String,
    pub skill: AgentSkill,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSkill {
    CreateWorkflow,
    ModifyWorkflow,
    FixError,
    Sweep,
}

impl AgentSkill {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::CreateWorkflow => "create_workflow.md",
            Self::ModifyWorkflow => "modify_workflow.md",
            Self::FixError => "fix_error.md",
            Self::Sweep => "sweep.md",
        }
    }
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
                skill: AgentSkill::ModifyWorkflow,
            },
        )
    }

    fn command_spec_for_turn(&self, root_dir: &Path, turn: AgentTurn) -> CommandSpec {
        let prompt = format!(
            "\
Read ctx/instructions.md, ctx/graph.json, and ctx/node_defs/catalog.json.
Use the selected skill from ctx/skills/.
User turn:
{}

Write the graph proposal to out/proposal.json. Do not print secrets or write outside out/.",
            turn.message
        );

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
                if sender
                    .send(RuntimeEvent::Failed {
                        message: err.to_string(),
                    })
                    .await
                    .is_err()
                {
                    return;
                }
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
        let base_graph = request.graph.clone();
        let current_version_id = request.base_version_id.clone();
        let turn = AgentTurn {
            message: request.user_message.clone(),
            skill: request.skill,
        };
        let session = create_session_contract(&request)?;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            1,
            "ctx.created",
            json!({}),
        );

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

        let mut seq = 4;
        while let Some(event) = self.runtime.next_event(&handle).await {
            match event {
                RuntimeEvent::Status { message } => {
                    self.emit_status(
                        &session.workspace_id,
                        &session.id,
                        seq,
                        "runtime.status",
                        json!({ "message": message }),
                    );
                    seq += 1;
                }
                RuntimeEvent::Failed { message } => {
                    return Err(AgentError::Runtime(message));
                }
                RuntimeEvent::Finished => break,
            }
        }

        let proposal = read_validated_proposal(&session, &base_graph, &current_version_id)?;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            seq,
            "agent.status.end",
            json!({ "proposal_title": proposal.proposal.title }),
        );
        Ok(proposal)
    }

    fn emit_status(
        &self,
        workspace_id: &str,
        session_id: &str,
        seq: i64,
        status: &str,
        detail: Value,
    ) {
        let _ = self.events.publish(RunEventEnvelope {
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
                "status": status,
                "detail": detail
            }),
        });
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedAgentProposal {
    pub session_id: String,
    pub proposal: PreparedProposal,
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

pub fn create_session_contract(request: &AgentSessionRequest) -> AgentResult<AgentSession> {
    let session_id = format!("agent_{}", Uuid::now_v7().simple());
    let root_dir = request.sessions_dir.join(&session_id);
    let ctx_dir = root_dir.join("ctx");
    let out_dir = root_dir.join("out");
    let tmp_dir = root_dir.join("tmp");
    let skills_dir = ctx_dir.join("skills");
    let node_defs_dir = ctx_dir.join("node_defs");

    fs::create_dir_all(&skills_dir)?;
    fs::create_dir_all(&node_defs_dir)?;
    fs::create_dir_all(&out_dir)?;
    fs::create_dir_all(&tmp_dir)?;

    write_json(ctx_dir.join("graph.json"), &request.graph)?;
    write_json(
        node_defs_dir.join("catalog.json"),
        &NodeRegistry::builtin().export_catalog(),
    )?;
    fs::write(
        ctx_dir.join("instructions.md"),
        instructions_markdown(request),
    )?;
    fs::write(skills_dir.join("node_library.md"), node_library_skill())?;
    fs::write(
        skills_dir.join(request.skill.file_name()),
        selected_skill(request.skill),
    )?;
    fs::write(
        root_dir.join("transcript.jsonl"),
        format!(
            "{}\n",
            json!({
                "role": "user",
                "message": request.user_message,
                "skill": request.skill
            })
        ),
    )?;

    Ok(AgentSession {
        id: session_id,
        workspace_id: request.workspace_id.clone(),
        root_dir,
        ctx_dir,
        out_dir,
        base_version_id: request.base_version_id.clone(),
    })
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
        proposal,
    })
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> AgentResult<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn safe_runtime_env(
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
        if let Some(message) = runtime_status_from_line(&line) {
            if sender.send(RuntimeEvent::Status { message }).await.is_err() {
                break;
            }
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

fn event_server_time() -> String {
    let epoch_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("unix:{epoch_seconds}")
}

fn instructions_markdown(request: &AgentSessionRequest) -> String {
    format!(
        "\
# Helixflow Agent Turn

Workspace: {workspace_id}
Base version: {base_version_id}

Read `ctx/graph.json` and `ctx/node_defs/catalog.json`.
Write exactly one result file under `out/`.
For graph changes, write `out/proposal.json` matching the proposal schema.
Do not write credentials or local filesystem paths.

User request:
{user_message}
",
        workspace_id = request.workspace_id,
        base_version_id = request.base_version_id,
        user_message = request.user_message
    )
}

fn node_library_skill() -> &'static str {
    "# Node Library\nUse built-in node definitions from ctx/node_defs/catalog.json.\n"
}

fn selected_skill(skill: AgentSkill) -> &'static str {
    match skill {
        AgentSkill::CreateWorkflow => "# Create Workflow\nCreate a valid graph proposal.\n",
        AgentSkill::ModifyWorkflow => "# Modify Workflow\nMake the smallest valid proposal.\n",
        AgentSkill::FixError => "# Fix Error\nPropose the smallest graph fix.\n",
        AgentSkill::Sweep => "# Sweep\nWrite a sweep run plan when requested.\n",
    }
}

fn ensure_direct_child(parent: &Path, candidate: &Path) -> AgentResult<()> {
    if candidate.parent() == Some(parent) {
        Ok(())
    } else {
        Err(AgentError::PathOutsideSession(candidate.to_path_buf()))
    }
}

fn read_output_file(out_dir: &Path, output_path: &Path) -> AgentResult<Vec<u8>> {
    ensure_direct_child(out_dir, output_path)?;

    let out_type = fs::symlink_metadata(out_dir)?.file_type();
    if out_type.is_symlink() || !out_type.is_dir() {
        return Err(AgentError::InvalidOutputFile {
            path: out_dir.to_path_buf(),
            reason: "out directory is not a real directory".to_owned(),
        });
    }

    let output_type = fs::symlink_metadata(output_path)?.file_type();
    if output_type.is_symlink() || !output_type.is_file() {
        return Err(AgentError::InvalidOutputFile {
            path: output_path.to_path_buf(),
            reason: "proposal output is not a regular file".to_owned(),
        });
    }

    let canonical_out = fs::canonicalize(out_dir)?;
    let canonical_output = fs::canonicalize(output_path)?;
    if canonical_output.parent() != Some(canonical_out.as_path()) {
        return Err(AgentError::PathOutsideSession(output_path.to_path_buf()));
    }

    Ok(fs::read(canonical_output)?)
}

pub type AgentResult<T> = Result<T, AgentError>;
pub type RuntimeResult<T> = Result<T, RuntimeError>;

#[derive(Debug)]
pub enum AgentError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Graph(helixflow_graph::GraphError),
    PathOutsideSession(PathBuf),
    InvalidOutputFile { path: PathBuf, reason: String },
    Runtime(String),
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::Graph(err) => write!(f, "{err}"),
            Self::PathOutsideSession(path) => {
                write!(
                    f,
                    "agent output path is outside session: {}",
                    path.display()
                )
            }
            Self::InvalidOutputFile { path, reason } => {
                write!(f, "invalid agent output file {}: {reason}", path.display())
            }
            Self::Runtime(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<std::io::Error> for AgentError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for AgentError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<helixflow_graph::GraphError> for AgentError {
    fn from(err: helixflow_graph::GraphError) -> Self {
        Self::Graph(err)
    }
}

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

#[cfg(test)]
mod tests;
