use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{GraphEdge, GraphNode, WorkflowGraph};

pub type CanvasResult<T> = Result<T, CanvasError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasDocument {
    pub schema_version: u32,
    pub canvas_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub document_version_id: Option<String>,
    pub title: String,
    pub seq: u64,
    #[serde(default)]
    pub base_graph_version_id: Option<String>,
    #[serde(default)]
    pub viewport: CanvasViewport,
    #[serde(default)]
    pub nodes: BTreeMap<String, CanvasNode>,
    #[serde(default)]
    pub edges: BTreeMap<String, CanvasEdge>,
    #[serde(default)]
    pub comments: BTreeMap<String, CanvasComment>,
    #[serde(default = "empty_object")]
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

impl CanvasDocument {
    pub fn project_workflow_graph(&self) -> CanvasResult<WorkflowGraph> {
        if self.schema_version != 1 {
            return Err(CanvasError::UnsupportedSchema(self.schema_version));
        }

        let mut nodes = BTreeMap::new();
        for (node_key, node) in &self.nodes {
            ensure_id_matches("node", node_key, &node.id)?;
            if node.kind != CanvasNodeKind::Workflow {
                continue;
            }
            let node_type = node
                .node_type
                .clone()
                .ok_or_else(|| CanvasError::MissingExecutableNodeType(node.id.clone()))?;
            if !node.params.is_object() {
                return Err(CanvasError::ParamsNotObject(node.id.clone()));
            }
            nodes.insert(
                node.id.clone(),
                GraphNode {
                    node_type,
                    title: node.title.clone(),
                    params: node.params.clone(),
                    pos: [node.position.x, node.position.y],
                },
            );
        }

        let mut edges = Vec::new();
        for (edge_key, edge) in &self.edges {
            ensure_id_matches("edge", edge_key, &edge.id)?;
            let Some(edge_type) = edge.kind.projected_edge_type(edge.edge_type.as_deref())? else {
                continue;
            };
            if !nodes.contains_key(&edge.from.node_id) {
                return Err(CanvasError::MissingEndpoint {
                    edge_id: edge.id.clone(),
                    node_id: edge.from.node_id.clone(),
                });
            }
            if !nodes.contains_key(&edge.to.node_id) {
                return Err(CanvasError::MissingEndpoint {
                    edge_id: edge.id.clone(),
                    node_id: edge.to.node_id.clone(),
                });
            }
            edges.push(GraphEdge {
                from: [edge.from.node_id.clone(), edge.from.port.clone()],
                to: [edge.to.node_id.clone(), edge.to.port.clone()],
                edge_type,
            });
        }

        Ok(WorkflowGraph {
            schema_version: 1,
            nodes,
            edges,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasViewport {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

impl Default for CanvasViewport {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasNode {
    pub id: String,
    pub kind: CanvasNodeKind,
    #[serde(default)]
    pub node_type: Option<String>,
    pub title: String,
    pub position: CanvasPoint,
    #[serde(default)]
    pub size: Option<CanvasSize>,
    #[serde(default)]
    pub ports: CanvasPorts,
    #[serde(default = "empty_object")]
    pub params: Value,
    #[serde(default = "empty_object")]
    pub content: Value,
    #[serde(default)]
    pub media: Vec<CanvasMediaRef>,
    #[serde(default)]
    pub runtime: CanvasNodeRuntime,
    #[serde(default)]
    pub ui: CanvasNodeUi,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CanvasNodeKind {
    Workflow,
    Text,
    Image,
    Video,
    Audio,
    Comment,
    Group,
    Artifact,
    Config,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasPorts {
    #[serde(default)]
    pub inputs: Vec<CanvasPort>,
    #[serde(default)]
    pub outputs: Vec<CanvasPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasPort {
    pub name: String,
    #[serde(rename = "type")]
    pub port_type: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasMediaRef {
    pub id: String,
    pub kind: CanvasMediaKind,
    pub uri: String,
    #[serde(default)]
    pub artifact_id: Option<String>,
    #[serde(default)]
    pub upload_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CanvasMediaKind {
    Upload,
    Artifact,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasNodeRuntime {
    pub status: CanvasRuntimeStatus,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub run_step_id: Option<String>,
    #[serde(default)]
    pub artifact_ids: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl Default for CanvasNodeRuntime {
    fn default() -> Self {
        Self {
            status: CanvasRuntimeStatus::Idle,
            run_id: None,
            run_step_id: None,
            artifact_ids: Vec::new(),
            error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CanvasRuntimeStatus {
    Idle,
    Queued,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasNodeUi {
    pub z_index: i64,
    pub collapsed: bool,
    pub locked: bool,
    #[serde(default)]
    pub color: Option<String>,
}

impl Default for CanvasNodeUi {
    fn default() -> Self {
        Self {
            z_index: 0,
            collapsed: false,
            locked: false,
            color: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasEdge {
    pub id: String,
    pub from: CanvasEndpoint,
    pub to: CanvasEndpoint,
    pub kind: CanvasEdgeKind,
    #[serde(default)]
    pub edge_type: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "empty_object")]
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasEndpoint {
    pub node_id: String,
    pub port: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CanvasEdgeKind {
    Data,
    Artifact,
    Control,
    Reference,
    Visual,
}

impl CanvasEdgeKind {
    fn projected_edge_type(self, edge_type: Option<&str>) -> CanvasResult<Option<String>> {
        match self {
            Self::Data => edge_type
                .map(|value| Some(value.to_owned()))
                .ok_or(CanvasError::MissingDataEdgeType),
            Self::Artifact => Ok(Some("artifact".to_owned())),
            Self::Control => Ok(Some("control".to_owned())),
            Self::Reference | Self::Visual => Ok(None),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasComment {
    pub id: String,
    pub anchor: CanvasCommentAnchor,
    pub body: String,
    pub resolved: bool,
    pub author_id: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasCommentAnchor {
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub edge_id: Option<String>,
    #[serde(default)]
    pub position: Option<CanvasPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasOpEnvelope {
    pub op_id: String,
    pub canvas_id: String,
    pub seq: u64,
    pub base_seq: u64,
    pub actor: CanvasActor,
    pub kind: CanvasOpKind,
    pub payload: Value,
    pub idempotency_key: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasActor {
    pub id: String,
    pub kind: CanvasActorKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CanvasActorKind {
    User,
    Agent,
    System,
    JobWorker,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CanvasOpKind {
    NodeAdd,
    NodePatch,
    NodeMove,
    NodeResize,
    NodeDelete,
    EdgeAdd,
    EdgeDelete,
    CommentAdd,
    CommentPatch,
    CommentDelete,
    RunRequest,
    ArtifactAttach,
    ProposalApply,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasError {
    UnsupportedSchema(u32),
    CanvasIdMismatch {
        expected: String,
        actual: String,
    },
    UnexpectedSeq {
        expected: u64,
        actual: u64,
    },
    FutureBaseSeq {
        base_seq: u64,
        current_seq: u64,
    },
    IdMismatch {
        entity: &'static str,
        key: String,
        id: String,
    },
    DuplicateNode(String),
    MissingNode(String),
    DuplicateEdge(String),
    MissingEdge(String),
    DuplicateComment(String),
    MissingComment(String),
    MissingExecutableNodeType(String),
    ParamsNotObject(String),
    MissingEndpoint {
        edge_id: String,
        node_id: String,
    },
    MissingDataEdgeType,
    InvalidPayload {
        kind: CanvasOpKind,
        message: String,
    },
    PatchNotObject {
        entity: &'static str,
        id: String,
    },
    UnsupportedPatchField {
        entity: &'static str,
        field: String,
    },
    PatchConflict {
        entity: &'static str,
        id: String,
        field: String,
    },
}

impl fmt::Display for CanvasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(version) => {
                write!(f, "unsupported canvas schema version: {version}")
            }
            Self::CanvasIdMismatch { expected, actual } => {
                write!(
                    f,
                    "canvas op targets `{actual}` but document is `{expected}`"
                )
            }
            Self::UnexpectedSeq { expected, actual } => {
                write!(f, "expected canvas op seq `{expected}` but got `{actual}`")
            }
            Self::FutureBaseSeq {
                base_seq,
                current_seq,
            } => write!(
                f,
                "canvas op base_seq `{base_seq}` is ahead of current seq `{current_seq}`"
            ),
            Self::IdMismatch { entity, key, id } => {
                write!(f, "{entity} map key `{key}` does not match id `{id}`")
            }
            Self::DuplicateNode(node_id) => write!(f, "duplicate canvas node: {node_id}"),
            Self::MissingNode(node_id) => write!(f, "missing canvas node: {node_id}"),
            Self::DuplicateEdge(edge_id) => write!(f, "duplicate canvas edge: {edge_id}"),
            Self::MissingEdge(edge_id) => write!(f, "missing canvas edge: {edge_id}"),
            Self::DuplicateComment(comment_id) => {
                write!(f, "duplicate canvas comment: {comment_id}")
            }
            Self::MissingComment(comment_id) => write!(f, "missing canvas comment: {comment_id}"),
            Self::MissingExecutableNodeType(node_id) => {
                write!(f, "executable canvas node `{node_id}` is missing node_type")
            }
            Self::ParamsNotObject(node_id) => {
                write!(
                    f,
                    "executable canvas node `{node_id}` params must be an object"
                )
            }
            Self::MissingEndpoint { edge_id, node_id } => {
                write!(
                    f,
                    "canvas edge `{edge_id}` references missing node `{node_id}`"
                )
            }
            Self::MissingDataEdgeType => {
                write!(
                    f,
                    "data canvas edges must declare edge_type for workflow projection"
                )
            }
            Self::InvalidPayload { kind, message } => {
                write!(f, "invalid payload for `{kind:?}`: {message}")
            }
            Self::PatchNotObject { entity, id } => {
                write!(f, "{entity} patch for `{id}` must be an object")
            }
            Self::UnsupportedPatchField { entity, field } => {
                write!(f, "{entity} patch field `{field}` is not supported")
            }
            Self::PatchConflict { entity, id, field } => {
                write!(f, "{entity} patch conflict on `{id}.{field}`")
            }
        }
    }
}

impl std::error::Error for CanvasError {}

fn ensure_id_matches(entity: &'static str, key: &str, id: &str) -> CanvasResult<()> {
    if key == id {
        return Ok(());
    }
    Err(CanvasError::IdMismatch {
        entity,
        key: key.to_owned(),
        id: id.to_owned(),
    })
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}
