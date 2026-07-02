use std::{
    collections::hash_map::DefaultHasher,
    fmt, fs,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::Arc,
};

use helixflow_agent::{AgentService, AgentSessionRequest, AgentSkill, CodexRuntime};
use helixflow_gateway::Provider;
use helixflow_graph::{
    ApplyProposalVersion, GraphService, PreparedProposal, ProposalOp, ProposalState, WorkflowGraph,
};
use helixflow_registry::NodeRegistry;
use helixflow_run::{AgentRunRequest, EventBus, RunService};
use helixflow_store::{
    MessageRecord, NewMessage, NewProposal, ProposalRecord, Store, VersionRecord, VersionSource,
    WorkspaceRecord,
};
use serde_json::{Value, json};

use crate::agent_transcript::read_agent_messages;
use crate::chat_intent::{ChatIntent, classify};
use crate::provider::ConfiguredProvider;
use crate::workbench_helpers::{
    ensure_proposal_workspace, proposal_kind, proposal_kind_str, resolve_program,
};
use crate::workbench_view::{
    graph_nodes_json, history_json, message_json, output_json, pending_confirmation_json, run_json,
};

#[derive(Clone)]
pub struct Workbench {
    pub(crate) store: Store,
    graph: GraphService,
    pub(crate) runs: RunService<ConfiguredProvider>,
    provider: ConfiguredProvider,
    pub(crate) agent: AgentService<CodexRuntime>,
    pub(crate) data_dir: Arc<PathBuf>,
}
impl Workbench {
    pub async fn open() -> WorkbenchResult<Self> {
        let data_dir = std::env::var("HELIXFLOW_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".helixflow"));
        fs::create_dir_all(&data_dir)?;
        let data_dir = fs::canonicalize(data_dir)?;
        let database_url = std::env::var("HELIXFLOW_DATABASE_URL").unwrap_or_else(|_| {
            format!("sqlite://{}", data_dir.join("helixflow.sqlite").display())
        });
        let store = Store::open(&database_url).await?;
        let events = EventBus::default();
        let provider = ConfiguredProvider::from_env();
        let runs =
            RunService::with_provider_and_events(store.clone(), provider.clone(), events.clone());
        let codex_program = std::env::var("HELIXFLOW_CODEX").unwrap_or_else(|_| "codex".to_owned());
        let codex_program = resolve_program(&codex_program)?;
        let agent = AgentService::new(CodexRuntime::new(codex_program), events);

        Ok(Self {
            store,
            graph: GraphService::new(NodeRegistry::builtin()),
            runs,
            provider,
            agent,
            data_dir: Arc::new(data_dir),
        })
    }

    pub fn events(&self) -> EventBus {
        self.runs.events()
    }

    pub async fn provider_status(&self) -> Value {
        let health = self.provider.health().await;
        json!({
            "defaultProvider": "atlas",
            "providers": [
                {
                    "id": "atlas",
                    "displayName": "Atlas",
                    "configured": matches!(self.provider, ConfiguredProvider::Atlas(_)),
                    "enabled": health.ok,
                    "label": self.provider.status_label(),
                    "endpoint": self.provider.endpoint(),
                    "health": {
                        "ok": health.ok,
                        "message": health.message
                    }
                }
            ]
        })
    }

    pub async fn state(&self, workspace_id: &str) -> WorkbenchResult<Value> {
        let context = self.workspace_context(workspace_id).await?;
        self.state_from_context(context).await
    }

    pub async fn send_message(&self, workspace_id: &str, text: &str) -> WorkbenchResult<Value> {
        let text = text.trim();
        if text.is_empty() {
            return Err(WorkbenchError::BadRequest("message is empty".to_owned()));
        }

        let context = self.workspace_context(workspace_id).await?;
        let user_message = self
            .create_message(workspace_id, "user", text, "text", None)
            .await?;
        if matches!(classify(text), ChatIntent::ChatOnly) {
            let reply = match self
                .agent
                .answer_chat(AgentSessionRequest {
                    workspace_id: workspace_id.to_owned(),
                    base_version_id: context.version.id.clone(),
                    user_message: text.to_owned(),
                    graph: context.graph.clone(),
                    sessions_dir: self.data_dir.join("agent-sessions"),
                    skill: AgentSkill::Chat,
                })
                .await
            {
                Ok(reply) => reply,
                Err(err) => {
                    let message = format!("Agent chat failed: {err}");
                    self.create_message(workspace_id, "system", &message, "error", None)
                        .await?;
                    return Err(WorkbenchError::Agent(message));
                }
            };
            for message in read_agent_messages(&self.data_dir, &reply.session_id)? {
                let kind = format!("agent_log:{}", message.kind);
                self.create_message(
                    workspace_id,
                    "agent",
                    &message.text,
                    &kind,
                    Some(&message.label),
                )
                .await?;
            }
            self.create_message(
                workspace_id,
                "agent",
                &reply.message,
                "chat",
                Some(&reply.session_id),
            )
            .await?;
            return self.state(workspace_id).await;
        }

        if context.graph.nodes.is_empty()
            && crate::design_artifact::should_create_design_artifact(text)
        {
            return self
                .create_design_artifact_from_message(
                    workspace_id,
                    &context.version.id,
                    context.graph,
                    text,
                    &user_message.id,
                )
                .await;
        }

        let agent_skill = if context.graph.nodes.is_empty() {
            AgentSkill::CreateWorkflow
        } else {
            AgentSkill::ModifyWorkflow
        };
        let proposal = match self
            .agent
            .propose_graph_change(AgentSessionRequest {
                workspace_id: workspace_id.to_owned(),
                base_version_id: context.version.id.clone(),
                user_message: text.to_owned(),
                graph: context.graph.clone(),
                sessions_dir: self.data_dir.join("agent-sessions"),
                skill: agent_skill,
            })
            .await
        {
            Ok(proposal) => proposal,
            Err(err) => {
                let message = format!("Agent failed: {err}");
                self.create_message(workspace_id, "system", &message, "error", None)
                    .await?;
                return Err(WorkbenchError::Agent(message));
            }
        };

        let proposal_dir = format!(
            "workspaces/{workspace_id}/proposals/{}",
            proposal.session_id
        );
        for message in read_agent_messages(&self.data_dir, &proposal.session_id)? {
            let kind = format!("agent_log:{}", message.kind);
            self.create_message(
                workspace_id,
                "agent",
                &message.text,
                &kind,
                Some(&message.label),
            )
            .await?;
        }
        let ops_path = format!("{proposal_dir}/ops.json");
        let preview_graph_path = format!("{proposal_dir}/preview.json");
        self.write_json(&ops_path, &proposal.proposal.ops)?;
        self.write_graph(&preview_graph_path, &proposal.proposal.preview_graph)?;
        let record = self
            .store
            .create_proposal(NewProposal {
                workspace_id,
                base_version_id: &proposal.proposal.base_version_id,
                kind: proposal_kind_str(proposal.proposal.kind),
                title: &proposal.proposal.title,
                summary: &proposal.proposal.summary,
                ops_path: &ops_path,
                preview_graph_path: Some(&preview_graph_path),
                message_id: Some(&user_message.id),
            })
            .await?;

        self.create_message(
            workspace_id,
            "agent",
            &format!(
                "Proposal ready: {}. {}",
                proposal.proposal.title, proposal.proposal.summary
            ),
            "proposal_pending",
            Some(&record.id),
        )
        .await?;
        self.state(workspace_id).await
    }

    pub async fn request_run(&self, workspace_id: &str) -> WorkbenchResult<Value> {
        let context = self.workspace_context(workspace_id).await?;
        if context.graph.nodes.is_empty() {
            return Err(WorkbenchError::BadRequest(
                "create a workflow through chat before running Queue".to_owned(),
            ));
        }
        match self
            .runs
            .request_agent_run(AgentRunRequest {
                workspace_id: workspace_id.to_owned(),
                version_id: context.version.id.clone(),
                group_id: None,
                label: "Manual run".to_owned(),
                graph: context.graph,
            })
            .await
        {
            Ok(_) => self.state(workspace_id).await,
            Err(err) => {
                let message = format!("Run request failed: {err}");
                self.create_message(workspace_id, "system", &message, "error", None)
                    .await?;
                Err(WorkbenchError::Run(message))
            }
        }
    }

    pub async fn approve_run(&self, workspace_id: &str, run_id: &str) -> WorkbenchResult<Value> {
        match self.runs.confirm_run(run_id).await {
            Ok(outcome) => {
                self.attach_run_artifacts_to_canvas(workspace_id, &outcome.artifacts)
                    .await?;
                self.state(workspace_id).await
            }
            Err(err) => {
                let message = format!("Run failed: {err}");
                self.create_message(workspace_id, "system", &message, "error", Some(run_id))
                    .await?;
                Err(WorkbenchError::Run(message))
            }
        }
    }

    pub async fn hold_run(&self, workspace_id: &str, run_id: &str) -> WorkbenchResult<Value> {
        let error_json = json!({ "reason": "held by user" }).to_string();
        self.store
            .update_run_status(run_id, "interrupted", Some(&error_json))
            .await?;
        self.create_message(
            workspace_id,
            "system",
            "Run was held.",
            "run_held",
            Some(run_id),
        )
        .await?;
        self.state(workspace_id).await
    }

    pub async fn apply_proposal(
        &self,
        workspace_id: &str,
        proposal_id: &str,
    ) -> WorkbenchResult<Value> {
        let context = self.workspace_context(workspace_id).await?;
        let proposal = self.store.proposal(proposal_id).await?;
        ensure_proposal_workspace(workspace_id, &proposal)?;
        if proposal.state != "pending" {
            return Err(WorkbenchError::BadRequest(format!(
                "proposal `{proposal_id}` is not pending"
            )));
        }
        if proposal.base_version_id != context.version.id {
            self.store
                .resolve_proposal(proposal_id, "superseded", None)
                .await?;
            return Err(WorkbenchError::BadRequest(format!(
                "proposal `{proposal_id}` is based on `{}`, current version is `{}`",
                proposal.base_version_id, context.version.id
            )));
        }

        let prepared = self.prepared_proposal(&proposal)?;
        let graph_path = self.graph_path(workspace_id, &format!("proposal-{proposal_id}"));
        let graph_hash = graph_hash(&prepared.preview_graph)?;
        let applied = self
            .graph
            .apply_proposal_version(
                &self.store,
                ApplyProposalVersion {
                    workspace_id,
                    base_graph: &context.graph,
                    current_version_id: &context.version.id,
                    proposal: &prepared,
                    version_label: &prepared.title,
                    graph_path: &graph_path,
                    graph_hash: &graph_hash,
                },
            )
            .await?;
        self.write_graph(&graph_path, &applied.graph)?;
        self.store
            .resolve_proposal(proposal_id, "applied", Some(&applied.version.id))
            .await?;
        self.create_message(
            workspace_id,
            "system",
            &format!("Applied proposal: {}", prepared.title),
            "proposal_applied",
            Some(&applied.version.id),
        )
        .await?;
        self.state(workspace_id).await
    }

    pub async fn dismiss_proposal(
        &self,
        workspace_id: &str,
        proposal_id: &str,
    ) -> WorkbenchResult<Value> {
        let proposal = self.store.proposal(proposal_id).await?;
        ensure_proposal_workspace(workspace_id, &proposal)?;
        if proposal.state != "pending" {
            return Err(WorkbenchError::BadRequest(format!(
                "proposal `{proposal_id}` is not pending"
            )));
        }
        self.store
            .resolve_proposal(proposal_id, "dismissed", None)
            .await?;
        self.create_message(
            workspace_id,
            "system",
            &format!("Dismissed proposal: {}", proposal.title),
            "proposal_dismissed",
            Some(proposal_id),
        )
        .await?;
        self.state(workspace_id).await
    }

    pub(crate) async fn workspace_context(
        &self,
        workspace_id: &str,
    ) -> WorkbenchResult<WorkspaceContext> {
        let workspace = self
            .store
            .ensure_workspace(workspace_id, "Helixflow Workspace")
            .await?;
        let version = if let Some(version_id) = &workspace.cur_version_id {
            self.store.version(version_id).await?
        } else {
            let graph = empty_graph();
            let graph_path = self.graph_path(workspace_id, "initial");
            let graph_hash = graph_hash(&graph)?;
            self.write_graph(&graph_path, &graph)?;
            self.store
                .create_version(helixflow_store::NewVersion {
                    workspace_id,
                    label: "Empty workflow",
                    source: VersionSource::Manual,
                    graph_path: &graph_path,
                    graph_hash: &graph_hash,
                    parent_id: None,
                })
                .await?
        };
        let graph = self.read_graph(&version.graph_path)?;
        Ok(WorkspaceContext {
            workspace: self.store.workspace(workspace_id).await?,
            version,
            graph,
        })
    }

    async fn state_from_context(&self, context: WorkspaceContext) -> WorkbenchResult<Value> {
        let messages = self.store.workspace_messages(&context.workspace.id).await?;
        let runs = self.store.workspace_runs(&context.workspace.id).await?;
        let latest_run = runs.last().cloned();
        let steps = if let Some(run) = &latest_run {
            self.store.run_steps(&run.id).await?
        } else {
            Vec::new()
        };
        let artifacts = self
            .store
            .workspace_artifacts(&context.workspace.id)
            .await?;
        let versions = self.store.workspace_versions(&context.workspace.id).await?;
        let proposal_history = self
            .store
            .workspace_proposal_history(&context.workspace.id)
            .await?;
        let pending_proposal = self
            .store
            .workspace_pending_proposal(&context.workspace.id)
            .await?;
        let plan = self
            .graph
            .compile_plan(&context.graph, &context.version.id)?;

        Ok(json!({
            "eventSeq": latest_run.as_ref().map(|run| run.created_at.len() as i64).unwrap_or(0),
            "workspace": {
                "id": context.workspace.id,
                "name": context.workspace.name,
                "versionId": context.version.id,
                "updatedAt": context.workspace.updated_at
            },
            "providers": self.provider_status().await,
            "chat": {
                "messages": messages.iter().map(message_json).collect::<Vec<_>>()
            },
            "graph": {
                "nodes": graph_nodes_json(&context.graph, &steps),
                "edges": context.graph.edges.iter().enumerate().map(|(index, edge)| {
                    json!({
                        "id": format!("edge_{index}"),
                        "from": { "nodeId": edge.from[0], "port": edge.from[1] },
                        "to": { "nodeId": edge.to[0], "port": edge.to[1] },
                        "kind": edge.edge_type
                    })
                }).collect::<Vec<_>>()
            },
            "run": run_json(latest_run.as_ref(), &steps, &plan),
            "outputs": artifacts.iter().map(|artifact| output_json(artifact, &self.data_dir)).collect::<Vec<_>>(),
            "history": history_json(&versions, &runs, &proposal_history),
            "pendingProposal": self.pending_proposal_json(pending_proposal.as_ref())?,
            "pendingConfirmation": pending_confirmation_json(latest_run.as_ref())
        }))
    }

    pub(crate) async fn create_message(
        &self,
        workspace_id: &str,
        role: &str,
        text: &str,
        kind: &str,
        ref_id: Option<&str>,
    ) -> WorkbenchResult<MessageRecord> {
        Ok(self
            .store
            .create_message(NewMessage {
                workspace_id,
                role,
                text: Some(text),
                kind,
                ref_id,
                attachment_ids_json: None,
            })
            .await?)
    }

    fn graph_path(&self, workspace_id: &str, name: &str) -> String {
        format!("workspaces/{workspace_id}/graphs/{name}.json")
    }

    fn graph_file(&self, graph_path: &str) -> PathBuf {
        self.data_dir.join(graph_path)
    }

    fn write_graph(&self, graph_path: &str, graph: &WorkflowGraph) -> WorkbenchResult<()> {
        let path = self.graph_file(graph_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(graph)?)?;
        Ok(())
    }

    fn write_json<T: serde::Serialize>(&self, path: &str, value: &T) -> WorkbenchResult<()> {
        let path = self.graph_file(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(value)?)?;
        Ok(())
    }

    fn read_graph(&self, graph_path: &str) -> WorkbenchResult<WorkflowGraph> {
        let path = self.graph_file(graph_path);
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    fn read_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> WorkbenchResult<T> {
        let path = self.graph_file(path);
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    fn prepared_proposal(&self, proposal: &ProposalRecord) -> WorkbenchResult<PreparedProposal> {
        let ops: Vec<ProposalOp> = self.read_json(&proposal.ops_path)?;
        let Some(preview_graph_path) = proposal.preview_graph_path.as_deref() else {
            return Err(WorkbenchError::BadRequest(format!(
                "proposal `{}` has no preview graph",
                proposal.id
            )));
        };
        Ok(PreparedProposal {
            base_version_id: proposal.base_version_id.clone(),
            kind: proposal_kind(&proposal.kind)?,
            title: proposal.title.clone(),
            summary: proposal.summary.clone(),
            ops: ops.clone(),
            diff_summary: proposal_diff_summary(&ops),
            preview_graph: self.read_graph(preview_graph_path)?,
            state: ProposalState::Pending,
            message_id: proposal.message_id.clone(),
        })
    }

    fn pending_proposal_json(&self, proposal: Option<&ProposalRecord>) -> WorkbenchResult<Value> {
        let Some(proposal) = proposal else {
            return Ok(Value::Null);
        };
        let prepared = self.prepared_proposal(proposal)?;
        Ok(json!({
            "id": proposal.id,
            "title": proposal.title,
            "summary": proposal.summary,
            "diffSummary": prepared.diff_summary,
            "previewGraph": {
                "nodes": graph_nodes_json(&prepared.preview_graph, &[]),
                "edges": prepared.preview_graph.edges.iter().enumerate().map(|(index, edge)| {
                    json!({
                        "id": format!("edge_{index}"),
                        "from": { "nodeId": edge.from[0], "port": edge.from[1] },
                        "to": { "nodeId": edge.to[0], "port": edge.to[1] },
                        "kind": edge.edge_type
                    })
                }).collect::<Vec<_>>()
            }
        }))
    }
}

pub(crate) struct WorkspaceContext {
    pub(crate) workspace: WorkspaceRecord,
    pub(crate) version: VersionRecord,
    pub(crate) graph: WorkflowGraph,
}
fn empty_graph() -> WorkflowGraph {
    use std::collections::BTreeMap;

    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
    }
}
fn graph_hash(graph: &WorkflowGraph) -> WorkbenchResult<String> {
    let encoded = serde_json::to_vec(graph)?;
    let mut hasher = DefaultHasher::new();
    encoded.hash(&mut hasher);
    Ok(format!("stdhash:{:x}", hasher.finish()))
}
fn proposal_diff_summary(ops: &[ProposalOp]) -> Vec<String> {
    ops.iter()
        .map(|op| match op {
            ProposalOp::AddNode { id, .. } => format!("+ add node {id}"),
            ProposalOp::RemoveNode { id } => format!("- remove node {id}"),
            ProposalOp::SetParam { id, key, .. } => format!("~ update {id}.{key}"),
            ProposalOp::AddEdge { edge } => format!(
                "+ connect {}.{} -> {}.{}",
                edge.from[0], edge.from[1], edge.to[0], edge.to[1]
            ),
            ProposalOp::RemoveEdge { edge } => format!(
                "- remove {}.{} -> {}.{}",
                edge.from[0], edge.from[1], edge.to[0], edge.to[1]
            ),
            ProposalOp::MoveNode { id, .. } => format!("~ move node {id}"),
        })
        .collect()
}
pub type WorkbenchResult<T> = Result<T, WorkbenchError>;

#[derive(Debug)]
pub enum WorkbenchError {
    BadRequest(String),
    Agent(String),
    Run(String),
    Config(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Store(helixflow_store::StoreError),
    Graph(helixflow_graph::GraphError),
    Canvas(helixflow_graph::CanvasError),
}
impl WorkbenchError {
    pub fn status_code(&self) -> axum::http::StatusCode {
        match self {
            Self::BadRequest(_) => axum::http::StatusCode::BAD_REQUEST,
            Self::Agent(_) | Self::Run(_) => axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            Self::Config(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Self::Canvas(_) => axum::http::StatusCode::BAD_REQUEST,
            Self::Io(_) | Self::Json(_) | Self::Store(_) | Self::Graph(_) => {
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}
impl fmt::Display for WorkbenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(message)
            | Self::Agent(message)
            | Self::Run(message)
            | Self::Config(message) => {
                write!(f, "{message}")
            }
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::Store(err) => write!(f, "{err}"),
            Self::Graph(err) => write!(f, "{err}"),
            Self::Canvas(err) => write!(f, "{err}"),
        }
    }
}
impl std::error::Error for WorkbenchError {}
impl From<std::io::Error> for WorkbenchError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}
impl From<serde_json::Error> for WorkbenchError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}
impl From<helixflow_store::StoreError> for WorkbenchError {
    fn from(err: helixflow_store::StoreError) -> Self {
        Self::Store(err)
    }
}
impl From<helixflow_graph::GraphError> for WorkbenchError {
    fn from(err: helixflow_graph::GraphError) -> Self {
        Self::Graph(err)
    }
}
impl From<helixflow_graph::CanvasError> for WorkbenchError {
    fn from(err: helixflow_graph::CanvasError) -> Self {
        Self::Canvas(err)
    }
}
