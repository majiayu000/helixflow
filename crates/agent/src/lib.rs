use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use helixflow_gateway::ProviderCatalogSnapshot;
use helixflow_graph::WorkflowGraph;
use helixflow_registry::NodeRegistry;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

mod canvas_ops;
mod contract;
mod prompt_stack;
mod runtime;
mod service;
mod turn_mode;

pub use canvas_ops::{CanvasGateState, CanvasOpsContext, CanvasOpsContract, CanvasSelection};
pub use contract::{
    AgentLogEntry, RunRequestAction, RunRequestOutput, ValidatedAgentIntent,
    ValidatedAgentProposal, ValidatedAgentReply, ValidatedRunRequest, read_validated_intent,
    read_validated_proposal, read_validated_reply, read_validated_run_request,
};
pub use prompt_stack::{
    PromptSection, PromptSectionKey, PromptStack, PromptStackMetadata, build_prompt_stack,
};
pub use runtime::{
    AgentRuntime, AgentTurn, CodexRuntime, CommandSpec, RuntimeError, RuntimeEvent, RuntimeHandle,
    RuntimeResult,
};
pub use service::AgentService;
pub use turn_mode::{
    AgentSkill, OutputContract, TurnClassification, TurnMode, TurnModeSource, TurnRoutingError,
    classify_turn_mode,
};

pub fn module_name() -> &'static str {
    "agent"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSession {
    pub id: String,
    pub workspace_id: String,
    pub root_dir: PathBuf,
    pub ctx_dir: PathBuf,
    pub out_dir: PathBuf,
    pub base_version_id: String,
    pub mode: TurnMode,
    pub output_contract: OutputContract,
    pub prompt_metadata: PromptStackMetadata,
}

#[derive(Debug, Clone)]
pub struct AgentSessionRequest {
    pub workspace_id: String,
    pub base_version_id: String,
    pub user_message: String,
    /// Prior chat turns (oldest first) so agent replies can reference
    /// earlier context across requests (HF-013).
    pub history: Vec<AgentHistoryMessage>,
    pub graph: WorkflowGraph,
    pub provider_catalog: ProviderCatalogSnapshot,
    pub run_context: Option<String>,
    pub sessions_dir: PathBuf,
    pub mode: TurnMode,
    pub skill: AgentSkill,
    pub canvas_context: Option<CanvasOpsContext>,
    /// GH130 T6: graph-editing turns request the IntentPlan contract
    /// (`out/intent.json`) instead of low-level proposals. Rollback path:
    /// the server flips this off via HELIXFLOW_AGENT_INTENT_CONTRACT=0.
    pub use_intent_contract: bool,
}

#[derive(Debug, Clone)]
pub struct AgentHistoryMessage {
    pub role: String,
    pub text: String,
}

pub fn create_session_contract(request: &AgentSessionRequest) -> AgentResult<AgentSession> {
    ensure_request_consistency(request)?;
    let session_id = format!("agent_{}", Uuid::now_v7().simple());
    let root_dir = request.sessions_dir.join(&session_id);
    let ctx_dir = root_dir.join("ctx");
    let out_dir = root_dir.join("out");
    let tmp_dir = root_dir.join("tmp");
    let skills_dir = ctx_dir.join("skills");
    let node_defs_dir = ctx_dir.join("node_defs");
    let workflow_backends_dir = ctx_dir.join("workflow_backends");
    let runtime_providers_dir = ctx_dir.join("runtime_providers");
    let api_connectors_dir = ctx_dir.join("api_connectors");

    fs::create_dir_all(&ctx_dir)?;
    fs::create_dir_all(&out_dir)?;
    fs::create_dir_all(&tmp_dir)?;

    let prompt_stack = build_prompt_stack(request);
    let prompt_metadata = prompt_stack.metadata();
    fs::write(ctx_dir.join("instructions.md"), prompt_stack.render())?;
    write_json(root_dir.join("prompt_metadata.json"), &prompt_metadata)?;
    if request.mode.uses_graph_context() {
        fs::create_dir_all(&skills_dir)?;
        fs::create_dir_all(&node_defs_dir)?;
        fs::create_dir_all(&workflow_backends_dir)?;
        fs::create_dir_all(&runtime_providers_dir)?;
        fs::create_dir_all(&api_connectors_dir)?;
        write_json(ctx_dir.join("graph.json"), &request.graph)?;
        write_json(
            ctx_dir.join("canvas_state.json"),
            &canvas_context_from_request(request),
        )?;
        write_json(ctx_dir.join("canvas_ops.json"), &CanvasOpsContract::v1())?;
        write_json(
            node_defs_dir.join("catalog.json"),
            &NodeRegistry::builtin().export_catalog(),
        )?;
        write_json(
            workflow_backends_dir.join("catalog.json"),
            &request.provider_catalog.workflow_backends,
        )?;
        write_json(
            runtime_providers_dir.join("catalog.json"),
            &request.provider_catalog.runtime_providers,
        )?;
        write_json(
            api_connectors_dir.join("catalog.json"),
            &request.provider_catalog.api_connectors,
        )?;
        fs::write(skills_dir.join("node_library.md"), node_library_skill())?;
        fs::write(
            skills_dir.join(request.skill.file_name()),
            selected_skill(request.skill),
        )?;
    }
    fs::write(
        root_dir.join("transcript.jsonl"),
        format!(
            "{}\n",
            json!({
                "role": "user",
                "message": request.user_message,
                "mode": request.mode,
                "skill": request.skill,
                "output_contract": request.mode.output_contract_with(request.use_intent_contract)
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
        mode: request.mode,
        output_contract: request
            .mode
            .output_contract_with(request.use_intent_contract),
        prompt_metadata,
    })
}

fn ensure_request_consistency(request: &AgentSessionRequest) -> AgentResult<()> {
    let expected_skill = request.mode.agent_skill();
    if request.skill != expected_skill {
        return Err(AgentError::InvalidMode {
            mode: request.mode,
            expected: request.mode.output_contract(),
            actual: request.mode.output_contract(),
        });
    }
    if let Some(message) = request
        .history
        .iter()
        .find(|message| !matches!(message.role.as_str(), "user" | "agent"))
    {
        return Err(AgentError::InvalidPromptContext(format!(
            "unsupported conversation role `{}`",
            message.role
        )));
    }
    Ok(())
}

fn canvas_context_from_request(request: &AgentSessionRequest) -> CanvasOpsContext {
    request.canvas_context.clone().unwrap_or_else(|| {
        CanvasOpsContext::from_graph(
            &request.workspace_id,
            &request.base_version_id,
            &request.graph,
            CanvasSelection::default(),
            CanvasGateState::default(),
        )
    })
}

fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> AgentResult<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn node_library_skill() -> &'static str {
    "# Node Library\nUse built-in node definitions from ctx/node_defs/catalog.json.\n"
}

fn selected_skill(skill: AgentSkill) -> &'static str {
    match skill {
        AgentSkill::Chat => "# Chat\nWrite a concise assistant reply to out/reply.json.\n",
        AgentSkill::CreateWorkflow => "# Create Workflow\nCreate a valid graph proposal.\n",
        AgentSkill::ModifyWorkflow => "# Modify Workflow\nMake the smallest valid proposal.\n",
        AgentSkill::FixError => "# Fix Error\nPropose the smallest graph fix.\n",
        AgentSkill::RunRequest => {
            "# Run Request\nWrite a backend run request to out/run_request.json.\n"
        }
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
    const MAX_AGENT_OUTPUT_BYTES: u64 = 1024 * 1024;

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

    let file = fs::File::open(&canonical_output)?;
    if file.metadata()?.len() > MAX_AGENT_OUTPUT_BYTES {
        return Err(AgentError::InvalidOutputFile {
            path: output_path.to_path_buf(),
            reason: format!("agent output exceeds the {MAX_AGENT_OUTPUT_BYTES} byte limit"),
        });
    }
    let mut bytes = Vec::new();
    file.take(MAX_AGENT_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_AGENT_OUTPUT_BYTES {
        return Err(AgentError::InvalidOutputFile {
            path: output_path.to_path_buf(),
            reason: format!("agent output exceeds the {MAX_AGENT_OUTPUT_BYTES} byte limit"),
        });
    }
    Ok(bytes)
}

pub type AgentResult<T> = Result<T, AgentError>;
#[derive(Debug)]
pub enum AgentError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Graph(helixflow_graph::GraphError),
    PathOutsideSession(PathBuf),
    InvalidOutputFile {
        path: PathBuf,
        reason: String,
    },
    InvalidMode {
        mode: TurnMode,
        expected: OutputContract,
        actual: OutputContract,
    },
    InvalidPromptContext(String),
    ProposalRetryExhausted {
        rounds: usize,
        last_error: String,
    },
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
            Self::InvalidMode {
                mode,
                expected,
                actual,
            } => write!(
                f,
                "agent turn mode `{mode}` uses `{actual}` but this path requires `{expected}`"
            ),
            Self::InvalidPromptContext(message) => {
                write!(f, "invalid agent prompt context: {message}")
            }
            Self::ProposalRetryExhausted { rounds, last_error } => write!(
                f,
                "proposal retry exhausted after {rounds} rounds: {last_error}"
            ),
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

#[cfg(test)]
mod retry_tests;
#[cfg(test)]
mod tests;
