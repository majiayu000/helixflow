use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    fs,
    hash::{Hash, Hasher},
    path::PathBuf,
};

use helixflow_graph::{
    CanvasActor, CanvasActorKind, CanvasDocument, CanvasEdge, CanvasEdgeKind, CanvasEndpoint,
    CanvasNode, CanvasNodeKind, CanvasNodeRuntime, CanvasNodeUi, CanvasOpEnvelope, CanvasOpKind,
    CanvasPoint, CanvasPorts, WorkflowGraph,
};
use helixflow_run::AgentRunRequest;
use helixflow_store::{
    ArtifactRecord, CanvasOpRecord, CanvasRecord, NewCanvas, NewCanvasOp, NewCanvasPresence,
    UpdatedCanvasSnapshot,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::workbench::{Workbench, WorkbenchError, WorkbenchResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanvasOpsRequest {
    #[serde(default)]
    pub ops: Vec<CanvasOpDraft>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanvasOpDraft {
    #[serde(alias = "baseSeq")]
    pub base_seq: u64,
    pub actor: CanvasActor,
    pub kind: CanvasOpKind,
    pub payload: Value,
    #[serde(alias = "idempotencyKey")]
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanvasPresenceRequest {
    #[serde(alias = "actorId")]
    pub actor_id: String,
    #[serde(default)]
    pub cursor: Option<Value>,
    #[serde(default)]
    pub selection: Option<Value>,
    #[serde(default)]
    pub viewport: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanvasEventsQuery {
    #[serde(default, alias = "afterSeq")]
    pub after_seq: Option<u64>,
}

impl Workbench {
    pub(crate) async fn canvas_for_workspace(&self, workspace_id: &str) -> WorkbenchResult<Value> {
        let context = self.ensure_canvas_for_workspace(workspace_id).await?;
        Ok(json!({
            "canvas": context.document
        }))
    }

    pub(crate) async fn append_canvas_ops(
        &self,
        canvas_id: &str,
        request: CanvasOpsRequest,
    ) -> WorkbenchResult<Value> {
        if request.ops.is_empty() {
            return Err(WorkbenchError::BadRequest(
                "canvas ops request is empty".to_owned(),
            ));
        }

        let mut context = self.canvas_context(canvas_id).await?;
        let mut accepted = Vec::new();
        let mut runs = Vec::new();

        for draft in request.ops {
            if let Some(existing) = self
                .store
                .canvas_op_by_idempotency_key(canvas_id, &draft.idempotency_key)
                .await?
            {
                accepted.push(canvas_op_envelope(existing)?);
                continue;
            }

            let preview = CanvasOpEnvelope {
                op_id: "preview".to_owned(),
                canvas_id: canvas_id.to_owned(),
                seq: context.document.seq + 1,
                base_seq: draft.base_seq,
                actor: draft.actor.clone(),
                kind: draft.kind,
                payload: draft.payload.clone(),
                idempotency_key: draft.idempotency_key.clone(),
                created_at: context.document.updated_at.clone(),
            };
            context.document.clone().apply_op(&preview)?;

            let record = self
                .store
                .append_canvas_op(NewCanvasOp {
                    canvas_id,
                    base_seq: seq_to_i64(draft.base_seq)?,
                    actor_json: &serde_json::to_string(&draft.actor)?,
                    kind: canvas_op_kind_str(draft.kind),
                    payload_json: &serde_json::to_string(&draft.payload)?,
                    idempotency_key: &draft.idempotency_key,
                })
                .await?;
            let envelope = canvas_op_envelope(record)?;
            let should_apply = envelope.seq > context.document.seq;
            if should_apply {
                context.document.apply_op(&envelope)?;
                if envelope.kind == CanvasOpKind::RunRequest {
                    runs.push(
                        self.request_canvas_run(&context.document, &envelope)
                            .await?,
                    );
                }
            }
            accepted.push(envelope);
        }

        self.write_canvas_snapshot(&context.record, &context.document)
            .await?;

        Ok(json!({
            "canvas": context.document,
            "ops": accepted,
            "runs": runs
        }))
    }

    pub(crate) async fn canvas_events_after(
        &self,
        canvas_id: &str,
        after_seq: u64,
    ) -> WorkbenchResult<Value> {
        let ops = self
            .store
            .canvas_ops_after(canvas_id, seq_to_i64(after_seq)?)
            .await?
            .into_iter()
            .map(canvas_op_envelope)
            .collect::<WorkbenchResult<Vec<_>>>()?;

        Ok(json!({ "ops": ops }))
    }

    pub(crate) async fn upsert_canvas_presence(
        &self,
        canvas_id: &str,
        request: CanvasPresenceRequest,
    ) -> WorkbenchResult<Value> {
        let cursor_json = request
            .cursor
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let selection_json = request
            .selection
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let viewport_json = request
            .viewport
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let presence = self
            .store
            .upsert_canvas_presence(NewCanvasPresence {
                canvas_id,
                actor_id: &request.actor_id,
                cursor_json: cursor_json.as_deref(),
                selection_json: selection_json.as_deref(),
                viewport_json: viewport_json.as_deref(),
            })
            .await?;

        Ok(json!({ "presence": presence }))
    }

    pub(crate) async fn attach_run_artifacts_to_canvas(
        &self,
        workspace_id: &str,
        artifacts: &[ArtifactRecord],
    ) -> WorkbenchResult<()> {
        let relevant: Vec<(&ArtifactRecord, &String)> = artifacts
            .iter()
            .filter_map(|artifact| artifact.node_id.as_ref().map(|node_id| (artifact, node_id)))
            .collect();
        if relevant.is_empty() {
            return Ok(());
        }

        let context = self.ensure_canvas_for_workspace(workspace_id).await?;
        let ops = relevant
            .into_iter()
            .map(|(artifact, node_id)| CanvasOpDraft {
                base_seq: context.document.seq,
                actor: CanvasActor {
                    id: "system".to_owned(),
                    kind: CanvasActorKind::System,
                },
                kind: CanvasOpKind::ArtifactAttach,
                payload: json!({
                    "node_id": node_id,
                    "artifact_id": artifact.id,
                    "run_id": artifact.run_id,
                    "run_step_id": artifact.run_step_id,
                    "status": "succeeded"
                }),
                idempotency_key: format!("artifact_attach:{}", artifact.id),
            })
            .collect();

        self.append_canvas_ops(&context.record.id, CanvasOpsRequest { ops })
            .await?;
        Ok(())
    }

    async fn request_canvas_run(
        &self,
        document: &CanvasDocument,
        op: &CanvasOpEnvelope,
    ) -> WorkbenchResult<Value> {
        let graph = document.project_workflow_graph()?;
        if graph.nodes.is_empty() {
            return Err(WorkbenchError::BadRequest(
                "canvas has no executable workflow nodes".to_owned(),
            ));
        }
        let version_id = document.base_graph_version_id.clone().ok_or_else(|| {
            WorkbenchError::BadRequest("canvas has no base graph version".to_owned())
        })?;
        let label = op
            .payload
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("Canvas run");
        let pending = self
            .runs
            .request_agent_run(AgentRunRequest {
                workspace_id: document.workspace_id.clone(),
                version_id,
                group_id: None,
                label: label.to_owned(),
                graph,
            })
            .await
            .map_err(|err| WorkbenchError::Run(err.to_string()))?;

        Ok(json!({
            "id": pending.run.id,
            "status": pending.run.status,
            "estimate": pending.estimate
        }))
    }

    async fn ensure_canvas_for_workspace(
        &self,
        workspace_id: &str,
    ) -> WorkbenchResult<CanvasContext> {
        let workspace = self.workspace_context(workspace_id).await?;
        if let Some(record) = self.store.workspace_canvas(workspace_id).await? {
            return self.canvas_context_from_record(record).await;
        }

        let snapshot_path = canvas_path(workspace_id);
        let mut record = self
            .store
            .create_canvas(NewCanvas {
                workspace_id,
                title: &workspace.workspace.name,
                snapshot_path: &snapshot_path,
                snapshot_hash: "pending",
                current_version_id: Some(&workspace.version.id),
            })
            .await?;
        let document = canvas_document_from_graph(&record, &workspace.graph);
        let snapshot_hash = canvas_hash(&document)?;
        self.write_canvas_file(&record.snapshot_path, &document)?;
        record = self
            .store
            .update_canvas_snapshot(UpdatedCanvasSnapshot {
                canvas_id: &record.id,
                seq: seq_to_i64(document.seq)?,
                snapshot_path: &record.snapshot_path,
                snapshot_hash: &snapshot_hash,
                current_version_id: Some(&workspace.version.id),
            })
            .await?;

        Ok(CanvasContext { record, document })
    }

    async fn canvas_context(&self, canvas_id: &str) -> WorkbenchResult<CanvasContext> {
        let record = self.store.canvas(canvas_id).await?;
        self.canvas_context_from_record(record).await
    }

    async fn canvas_context_from_record(
        &self,
        record: CanvasRecord,
    ) -> WorkbenchResult<CanvasContext> {
        let mut document = self.read_canvas_file(&record.snapshot_path)?;
        if document.canvas_id != record.id {
            return Err(WorkbenchError::BadRequest(format!(
                "canvas snapshot `{}` belongs to `{}`",
                record.snapshot_path, document.canvas_id
            )));
        }
        let record_seq = u64::try_from(record.seq)
            .map_err(|_| WorkbenchError::BadRequest("canvas seq is negative".to_owned()))?;
        if document.seq > record_seq {
            return Err(WorkbenchError::BadRequest(format!(
                "canvas snapshot seq `{}` is ahead of store seq `{}`",
                document.seq, record.seq
            )));
        }
        if document.seq < record_seq {
            let ops = self
                .store
                .canvas_ops_after(&record.id, seq_to_i64(document.seq)?)
                .await?
                .into_iter()
                .map(canvas_op_envelope)
                .collect::<WorkbenchResult<Vec<_>>>()?;
            document.replay_ops(&ops)?;
            self.write_canvas_snapshot(&record, &document).await?;
        }
        Ok(CanvasContext { record, document })
    }

    async fn write_canvas_snapshot(
        &self,
        record: &CanvasRecord,
        document: &CanvasDocument,
    ) -> WorkbenchResult<CanvasRecord> {
        let snapshot_hash = canvas_hash(document)?;
        self.write_canvas_file(&record.snapshot_path, document)?;
        Ok(self
            .store
            .update_canvas_snapshot(UpdatedCanvasSnapshot {
                canvas_id: &record.id,
                seq: seq_to_i64(document.seq)?,
                snapshot_path: &record.snapshot_path,
                snapshot_hash: &snapshot_hash,
                current_version_id: document.base_graph_version_id.as_deref(),
            })
            .await?)
    }

    fn read_canvas_file(&self, snapshot_path: &str) -> WorkbenchResult<CanvasDocument> {
        Ok(serde_json::from_slice(&fs::read(
            self.canvas_file(snapshot_path),
        )?)?)
    }

    fn write_canvas_file(
        &self,
        snapshot_path: &str,
        document: &CanvasDocument,
    ) -> WorkbenchResult<()> {
        let path = self.canvas_file(snapshot_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(document)?)?;
        Ok(())
    }

    fn canvas_file(&self, snapshot_path: &str) -> PathBuf {
        self.data_dir.join(snapshot_path)
    }
}

struct CanvasContext {
    record: CanvasRecord,
    document: CanvasDocument,
}

fn canvas_document_from_graph(record: &CanvasRecord, graph: &WorkflowGraph) -> CanvasDocument {
    CanvasDocument {
        schema_version: 1,
        canvas_id: record.id.clone(),
        workspace_id: record.workspace_id.clone(),
        document_version_id: None,
        title: record.title.clone(),
        seq: record.seq as u64,
        base_graph_version_id: record.current_version_id.clone(),
        viewport: Default::default(),
        nodes: graph
            .nodes
            .iter()
            .map(|(id, node)| {
                (
                    id.clone(),
                    CanvasNode {
                        id: id.clone(),
                        kind: CanvasNodeKind::Workflow,
                        node_type: Some(node.node_type.clone()),
                        title: node.title.clone(),
                        position: CanvasPoint {
                            x: node.pos[0],
                            y: node.pos[1],
                        },
                        size: None,
                        ports: CanvasPorts::default(),
                        params: node.params.clone(),
                        content: json!({}),
                        media: Vec::new(),
                        runtime: CanvasNodeRuntime::default(),
                        ui: CanvasNodeUi::default(),
                        created_at: record.created_at.clone(),
                        updated_at: record.updated_at.clone(),
                    },
                )
            })
            .collect(),
        edges: graph
            .edges
            .iter()
            .enumerate()
            .map(|(index, edge)| {
                let id = format!("edge_{index}");
                (
                    id.clone(),
                    CanvasEdge {
                        id,
                        from: CanvasEndpoint {
                            node_id: edge.from[0].clone(),
                            port: edge.from[1].clone(),
                        },
                        to: CanvasEndpoint {
                            node_id: edge.to[0].clone(),
                            port: edge.to[1].clone(),
                        },
                        kind: canvas_edge_kind(&edge.edge_type),
                        edge_type: (edge.edge_type != "artifact" && edge.edge_type != "control")
                            .then(|| edge.edge_type.clone()),
                        label: None,
                        metadata: json!({}),
                        created_at: record.created_at.clone(),
                        updated_at: record.updated_at.clone(),
                    },
                )
            })
            .collect(),
        comments: BTreeMap::new(),
        metadata: json!({}),
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
    }
}

fn canvas_edge_kind(edge_type: &str) -> CanvasEdgeKind {
    match edge_type {
        "artifact" => CanvasEdgeKind::Artifact,
        "control" => CanvasEdgeKind::Control,
        _ => CanvasEdgeKind::Data,
    }
}

fn canvas_path(workspace_id: &str) -> String {
    format!("workspaces/{workspace_id}/canvas/main.json")
}

fn canvas_hash(document: &CanvasDocument) -> WorkbenchResult<String> {
    let encoded = serde_json::to_vec(document)?;
    let mut hasher = DefaultHasher::new();
    encoded.hash(&mut hasher);
    Ok(format!("stdhash:{:x}", hasher.finish()))
}

fn canvas_op_envelope(record: CanvasOpRecord) -> WorkbenchResult<CanvasOpEnvelope> {
    Ok(CanvasOpEnvelope {
        op_id: record.id,
        canvas_id: record.canvas_id,
        seq: u64::try_from(record.seq)
            .map_err(|_| WorkbenchError::BadRequest("canvas op seq is negative".to_owned()))?,
        base_seq: u64::try_from(record.base_seq)
            .map_err(|_| WorkbenchError::BadRequest("canvas op base_seq is negative".to_owned()))?,
        actor: serde_json::from_str(&record.actor_json)?,
        kind: serde_json::from_value(Value::String(record.kind))?,
        payload: serde_json::from_str(&record.payload_json)?,
        idempotency_key: record.idempotency_key,
        created_at: record.created_at,
    })
}

fn canvas_op_kind_str(kind: CanvasOpKind) -> &'static str {
    match kind {
        CanvasOpKind::NodeAdd => "node_add",
        CanvasOpKind::NodePatch => "node_patch",
        CanvasOpKind::NodeMove => "node_move",
        CanvasOpKind::NodeResize => "node_resize",
        CanvasOpKind::NodeDelete => "node_delete",
        CanvasOpKind::EdgeAdd => "edge_add",
        CanvasOpKind::EdgeDelete => "edge_delete",
        CanvasOpKind::CommentAdd => "comment_add",
        CanvasOpKind::CommentPatch => "comment_patch",
        CanvasOpKind::CommentDelete => "comment_delete",
        CanvasOpKind::RunRequest => "run_request",
        CanvasOpKind::ArtifactAttach => "artifact_attach",
        CanvasOpKind::ProposalApply => "proposal_apply",
    }
}

fn seq_to_i64(seq: u64) -> WorkbenchResult<i64> {
    i64::try_from(seq)
        .map_err(|_| WorkbenchError::BadRequest(format!("canvas seq `{seq}` is too large")))
}
