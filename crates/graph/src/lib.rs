use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use helixflow_registry::{NodeRegistry, PortType};
use helixflow_store::{NewVersion, Store, VersionRecord, VersionSource};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod semantics;

use semantics::NodeSemanticsEntry;

pub fn module_name() -> &'static str {
    "graph"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowGraph {
    pub schema_version: u32,
    /// Catalog revision the embedded node semantics were resolved against.
    /// `None` on legacy graphs that carry no semantic layer yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_revision: Option<String>,
    pub nodes: BTreeMap<String, GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphNode {
    pub node_type: String,
    pub title: String,
    pub params: Value,
    pub pos: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
    /// Catalog-pinned semantic layer (GH145). `None` on legacy graphs and on
    /// non-executable nodes; executable nodes without an entry resolve through
    /// configured policy defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantics: Option<NodeSemanticsEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GraphEdge {
    pub from: [String; 2],
    pub to: [String; 2],
    pub edge_type: String,
}

#[derive(Debug, Clone)]
pub struct GraphService {
    registry: NodeRegistry,
}

impl GraphService {
    pub fn new(registry: NodeRegistry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &NodeRegistry {
        &self.registry
    }

    pub fn validate_graph(&self, graph: &WorkflowGraph) -> GraphResult<()> {
        if graph.schema_version != 1 {
            return Err(GraphError::UnsupportedSchema(graph.schema_version));
        }

        for (node_id, node) in &graph.nodes {
            let definition = self.registry.definition(&node.node_type)?;
            self.registry
                .validate_node_params(&node.node_type, &node.params)?;
            if let Some(size) = node.size
                && (size.iter().any(|value| !value.is_finite()) || size[0] <= 0.0 || size[1] <= 0.0)
            {
                return Err(GraphError::InvalidNodeSize(node_id.clone()));
            }

            for required_input in definition.inputs.iter().filter(|input| input.required) {
                let connected = graph
                    .edges
                    .iter()
                    .any(|edge| edge.to[0] == *node_id && edge.to[1] == required_input.name);
                let provided_by_param = node
                    .params
                    .as_object()
                    .and_then(|params| params.get(&required_input.name))
                    .is_some_and(|value| !value.is_null());
                if !connected && !provided_by_param {
                    return Err(GraphError::MissingRequiredInput {
                        node_id: node_id.clone(),
                        port: required_input.name.clone(),
                    });
                }
            }
        }

        let mut input_connections = BTreeSet::new();
        for edge in &graph.edges {
            if !input_connections.insert(edge.to.clone()) {
                return Err(GraphError::DuplicateInputConnection {
                    node_id: edge.to[0].clone(),
                    port: edge.to[1].clone(),
                });
            }

            let from_node = graph
                .nodes
                .get(&edge.from[0])
                .ok_or_else(|| GraphError::MissingEndpoint(edge.from[0].clone()))?;
            let to_node = graph
                .nodes
                .get(&edge.to[0])
                .ok_or_else(|| GraphError::MissingEndpoint(edge.to[0].clone()))?;
            let from_def = self.registry.definition(&from_node.node_type)?;
            let to_def = self.registry.definition(&to_node.node_type)?;
            let from_port = from_def
                .outputs
                .iter()
                .find(|port| port.name == edge.from[1])
                .ok_or_else(|| GraphError::MissingPort {
                    node_id: edge.from[0].clone(),
                    port: edge.from[1].clone(),
                })?;
            let to_port = to_def
                .inputs
                .iter()
                .find(|port| port.name == edge.to[1])
                .ok_or_else(|| GraphError::MissingPort {
                    node_id: edge.to[0].clone(),
                    port: edge.to[1].clone(),
                })?;

            if !edge_type_matches(from_port.port_type, to_port.port_type, &edge.edge_type) {
                return Err(GraphError::PortTypeMismatch {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                });
            }
        }

        if has_cycle(graph) {
            return Err(GraphError::CycleDetected);
        }

        Ok(())
    }

    pub fn preview_proposal(
        &self,
        base_graph: &WorkflowGraph,
        current_version_id: &str,
        draft: ProposalDraft,
    ) -> GraphResult<PreparedProposal> {
        if draft.base_version_id != current_version_id {
            return Err(GraphError::ProposalSuperseded {
                base_version_id: draft.base_version_id,
                current_version_id: current_version_id.to_owned(),
            });
        }

        let preview_graph = self.apply_ops(base_graph, &draft.ops)?;
        self.validate_graph(&preview_graph)?;

        Ok(PreparedProposal {
            base_version_id: draft.base_version_id,
            kind: draft.kind,
            title: draft.title,
            summary: draft.summary,
            ops: draft.ops,
            diff_summary: summarize_diff(base_graph, &preview_graph),
            preview_graph,
            state: ProposalState::Pending,
            message_id: draft.message_id,
        })
    }

    pub fn apply_proposal(
        &self,
        base_graph: &WorkflowGraph,
        current_version_id: &str,
        proposal: &PreparedProposal,
    ) -> GraphResult<WorkflowGraph> {
        ensure_proposal_applies_to_current(proposal, current_version_id)?;
        let graph = self.apply_ops(base_graph, &proposal.ops)?;
        self.validate_graph(&graph)?;
        Ok(graph)
    }

    pub async fn apply_proposal_version(
        &self,
        store: &Store,
        input: ApplyProposalVersion<'_>,
    ) -> GraphResult<AppliedProposalVersion> {
        let graph =
            self.apply_proposal(input.base_graph, input.current_version_id, input.proposal)?;
        let version = store
            .create_version_after(
                NewVersion {
                    workspace_id: input.workspace_id,
                    label: input.version_label,
                    source: VersionSource::Proposal,
                    graph_path: input.graph_path,
                    graph_hash: input.graph_hash,
                    parent_id: Some(&input.proposal.base_version_id),
                    semantics_json: input.semantics_json,
                },
                &input.proposal.base_version_id,
            )
            .await?;

        Ok(AppliedProposalVersion { graph, version })
    }

    pub fn dismiss_proposal(&self, base_graph: &WorkflowGraph) -> WorkflowGraph {
        base_graph.clone()
    }

    pub fn apply_ops(
        &self,
        base_graph: &WorkflowGraph,
        ops: &[ProposalOp],
    ) -> GraphResult<WorkflowGraph> {
        let mut graph = base_graph.clone();

        for op in ops {
            match op {
                ProposalOp::AddNode { id, node } => {
                    if graph.nodes.insert(id.clone(), node.clone()).is_some() {
                        return Err(GraphError::DuplicateNode(id.clone()));
                    }
                }
                ProposalOp::RemoveNode { id } => {
                    graph
                        .nodes
                        .remove(id)
                        .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
                    graph
                        .edges
                        .retain(|edge| edge.from[0] != *id && edge.to[0] != *id);
                }
                ProposalOp::SetParam {
                    id,
                    key,
                    prev,
                    value,
                } => {
                    let node = graph
                        .nodes
                        .get_mut(id)
                        .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
                    let params = node
                        .params
                        .as_object_mut()
                        .ok_or_else(|| GraphError::ParamsNotObject(id.clone()))?;
                    let current = params.get(key).cloned();
                    if let Some(expected) = prev
                        && current.as_ref() != Some(expected)
                    {
                        return Err(GraphError::SetParamConflict {
                            node_id: id.clone(),
                            key: key.clone(),
                        });
                    }
                    params.insert(key.clone(), value.clone());
                }
                ProposalOp::AddEdge { edge } => graph.edges.push(edge.clone()),
                ProposalOp::RemoveEdge { edge } => {
                    let before = graph.edges.len();
                    graph.edges.retain(|candidate| candidate != edge);
                    if graph.edges.len() == before {
                        return Err(GraphError::MissingEdge(edge.clone()));
                    }
                }
                ProposalOp::MoveNode { id, pos } => {
                    let node = graph
                        .nodes
                        .get_mut(id)
                        .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
                    node.pos = *pos;
                }
                ProposalOp::ResizeNode { id, size } => {
                    let node = graph
                        .nodes
                        .get_mut(id)
                        .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
                    node.size = Some(*size);
                }
                ProposalOp::SetSemantics { id, semantics } => {
                    let node = graph
                        .nodes
                        .get_mut(id)
                        .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
                    node.semantics = semantics.clone();
                }
            }
        }

        Ok(graph)
    }

    pub fn compile_plan(
        &self,
        graph: &WorkflowGraph,
        version_id: &str,
        provider_id: &str,
    ) -> GraphResult<ExecutionPlan> {
        self.validate_graph(graph)?;
        let mut steps = Vec::new();

        for node_id in topological_order(graph)? {
            let node = graph
                .nodes
                .get(&node_id)
                .ok_or_else(|| GraphError::MissingNode(node_id.clone()))?;
            let definition = self.registry.definition(&node.node_type)?;
            let inputs = graph
                .edges
                .iter()
                .filter(|edge| edge.to[0] == node_id)
                .map(|edge| (edge.to[1].clone(), edge.from.clone()))
                .collect();

            steps.push(ExecutionStep {
                node_id,
                node_type: node.node_type.clone(),
                provider: definition
                    .capability
                    .as_ref()
                    .map(|_| provider_id.to_owned()),
                capability: definition.capability.clone(),
                inputs,
                params: node.params.clone(),
                resolved: None,
            });
        }

        Ok(ExecutionPlan {
            schema_version: 1,
            version_id: version_id.to_owned(),
            catalog_revision: None,
            steps,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProposalDraft {
    pub base_version_id: String,
    pub kind: ProposalKind,
    pub title: String,
    pub summary: String,
    pub ops: Vec<ProposalOp>,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreparedProposal {
    pub base_version_id: String,
    pub kind: ProposalKind,
    pub title: String,
    pub summary: String,
    pub ops: Vec<ProposalOp>,
    pub diff_summary: Vec<String>,
    pub preview_graph: WorkflowGraph,
    pub state: ProposalState,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApplyProposalVersion<'a> {
    pub workspace_id: &'a str,
    pub base_graph: &'a WorkflowGraph,
    pub current_version_id: &'a str,
    pub proposal: &'a PreparedProposal,
    pub version_label: &'a str,
    pub graph_path: &'a str,
    pub graph_hash: &'a str,
    pub semantics_json: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppliedProposalVersion {
    pub graph: WorkflowGraph,
    pub version: VersionRecord,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    Create,
    Modify,
    Fix,
    Sweep,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalState {
    Pending,
    Applied,
    Dismissed,
    Superseded,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ProposalOp {
    AddNode {
        id: String,
        node: GraphNode,
    },
    RemoveNode {
        id: String,
    },
    SetParam {
        id: String,
        key: String,
        #[serde(default)]
        prev: Option<Value>,
        value: Value,
    },
    AddEdge {
        edge: GraphEdge,
    },
    RemoveEdge {
        edge: GraphEdge,
    },
    MoveNode {
        id: String,
        pos: [f32; 2],
    },
    ResizeNode {
        id: String,
        size: [f32; 2],
    },
    /// Replaces the embedded semantic layer of an existing node (GH145).
    /// `None` clears the entry; the compiler emits this when a recompile
    /// changes a kept node's capability binding.
    SetSemantics {
        id: String,
        semantics: Option<NodeSemanticsEntry>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionPlan {
    pub schema_version: u32,
    pub version_id: String,
    /// Catalog revision the step bindings were resolved against (GH130 T4).
    /// Persisted with the run via `plan_json`, so the snapshot is immutable
    /// and later catalog updates cannot change this run (P11/P12).
    #[serde(default)]
    pub catalog_revision: Option<String>,
    pub steps: Vec<ExecutionStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionStep {
    pub node_id: String,
    pub node_type: String,
    pub provider: Option<String>,
    pub capability: Option<String>,
    pub inputs: BTreeMap<String, [String; 2]>,
    pub params: Value,
    /// Immutable implementation resolved at run creation. `None` only for
    /// steps whose provider is not a catalog connector (e.g. mock) or for
    /// non-model steps; catalog providers refuse to execute without it.
    #[serde(default)]
    pub resolved: Option<ResolvedStepBinding>,
}

/// The per-step slice of the run's implementation snapshot (tech.md §8).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedStepBinding {
    pub capability_id: String,
    pub requested_model_id: Option<String>,
    pub resolved_model_id: String,
    pub binding_id: String,
    pub binding_revision: String,
    pub connector_id: String,
    pub operation_id: String,
}

pub type GraphResult<T> = Result<T, GraphError>;

#[derive(Debug, Clone, PartialEq)]
pub enum GraphError {
    Registry(helixflow_registry::RegistryError),
    UnsupportedSchema(u32),
    MissingRequiredInput {
        node_id: String,
        port: String,
    },
    MissingEndpoint(String),
    MissingPort {
        node_id: String,
        port: String,
    },
    DuplicateInputConnection {
        node_id: String,
        port: String,
    },
    PortTypeMismatch {
        from: [String; 2],
        to: [String; 2],
    },
    CycleDetected,
    DuplicateNode(String),
    MissingNode(String),
    MissingEdge(GraphEdge),
    InvalidNodeSize(String),
    ParamsNotObject(String),
    SetParamConflict {
        node_id: String,
        key: String,
    },
    ProposalSuperseded {
        base_version_id: String,
        current_version_id: String,
    },
    ProposalNotPending,
    Store(String),
}

impl From<helixflow_registry::RegistryError> for GraphError {
    fn from(err: helixflow_registry::RegistryError) -> Self {
        Self::Registry(err)
    }
}

impl From<helixflow_store::StoreError> for GraphError {
    fn from(err: helixflow_store::StoreError) -> Self {
        Self::Store(err.to_string())
    }
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(err) => write!(f, "{err}"),
            Self::UnsupportedSchema(version) => {
                write!(f, "unsupported graph schema version: {version}")
            }
            Self::MissingRequiredInput { node_id, port } => {
                write!(f, "missing required input `{port}` on node `{node_id}`")
            }
            Self::MissingEndpoint(node_id) => write!(f, "missing edge endpoint node: {node_id}"),
            Self::MissingPort { node_id, port } => {
                write!(f, "missing port `{port}` on node `{node_id}`")
            }
            Self::DuplicateInputConnection { node_id, port } => {
                write!(f, "duplicate input connection for `{node_id}.{port}`")
            }
            Self::PortTypeMismatch { from, to } => {
                write!(f, "edge port type mismatch from {from:?} to {to:?}")
            }
            Self::CycleDetected => write!(f, "graph contains a cycle"),
            Self::DuplicateNode(node_id) => write!(f, "duplicate node id: {node_id}"),
            Self::MissingNode(node_id) => write!(f, "missing node: {node_id}"),
            Self::MissingEdge(edge) => write!(f, "missing edge: {edge:?}"),
            Self::InvalidNodeSize(node_id) => write!(f, "invalid node size: {node_id}"),
            Self::ParamsNotObject(node_id) => {
                write!(f, "node params must be an object: {node_id}")
            }
            Self::SetParamConflict { node_id, key } => {
                write!(f, "set_param conflict on `{node_id}.{key}`")
            }
            Self::ProposalSuperseded {
                base_version_id,
                current_version_id,
            } => write!(
                f,
                "proposal base `{base_version_id}` is superseded by `{current_version_id}`"
            ),
            Self::ProposalNotPending => write!(f, "proposal is not pending"),
            Self::Store(err) => write!(f, "store error while applying proposal: {err}"),
        }
    }
}

impl std::error::Error for GraphError {}

fn ensure_proposal_applies_to_current(
    proposal: &PreparedProposal,
    current_version_id: &str,
) -> GraphResult<()> {
    if proposal.state != ProposalState::Pending {
        return Err(GraphError::ProposalNotPending);
    }

    if proposal.base_version_id != current_version_id {
        return Err(GraphError::ProposalSuperseded {
            base_version_id: proposal.base_version_id.clone(),
            current_version_id: current_version_id.to_owned(),
        });
    }

    Ok(())
}

fn edge_type_matches(from: PortType, to: PortType, edge_type: &str) -> bool {
    if edge_type == "artifact" {
        return to == PortType::Json;
    }

    from == to && edge_type == port_type_label(from)
}

pub fn port_type_label(port_type: PortType) -> &'static str {
    match port_type {
        PortType::Text => "text",
        PortType::Image => "image",
        PortType::Video => "video",
        PortType::Audio => "audio",
        PortType::Mask => "mask",
        PortType::Json => "json",
    }
}

fn has_cycle(graph: &WorkflowGraph) -> bool {
    topological_order(graph).is_err()
}

fn topological_order(graph: &WorkflowGraph) -> GraphResult<Vec<String>> {
    let mut incoming: BTreeMap<String, usize> = graph
        .nodes
        .keys()
        .map(|node_id| (node_id.clone(), 0))
        .collect();
    let mut outgoing: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for edge in &graph.edges {
        *incoming
            .get_mut(&edge.to[0])
            .ok_or_else(|| GraphError::MissingEndpoint(edge.to[0].clone()))? += 1;
        outgoing
            .entry(edge.from[0].clone())
            .or_default()
            .push(edge.to[0].clone());
    }

    let mut ready: BTreeSet<String> = incoming
        .iter()
        .filter_map(|(node_id, count)| (*count == 0).then_some(node_id.clone()))
        .collect();
    let mut ordered = Vec::with_capacity(graph.nodes.len());

    while let Some(node_id) = ready.pop_first() {
        ordered.push(node_id.clone());
        for next in outgoing.get(&node_id).into_iter().flatten() {
            let count = incoming
                .get_mut(next)
                .ok_or_else(|| GraphError::MissingEndpoint(next.clone()))?;
            *count -= 1;
            if *count == 0 {
                ready.insert(next.clone());
            }
        }
    }

    if ordered.len() != graph.nodes.len() {
        return Err(GraphError::CycleDetected);
    }

    Ok(ordered)
}

fn summarize_diff(base: &WorkflowGraph, preview: &WorkflowGraph) -> Vec<String> {
    let added = preview.nodes.len().saturating_sub(base.nodes.len());
    let removed = base.nodes.len().saturating_sub(preview.nodes.len());
    let edge_delta = preview.edges.len() as isize - base.edges.len() as isize;
    vec![
        format!("nodes_added={added}"),
        format!("nodes_removed={removed}"),
        format!("edge_delta={edge_delta}"),
    ]
}

#[cfg(test)]
mod tests;
