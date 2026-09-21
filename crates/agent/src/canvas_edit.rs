//! Agent-facing canvas edit contract: typed node/edge operations that compile
//! into backend `ProposalOp`s. The canvas is the artifact; IntentPlan is not
//! the agent write path.

use helixflow_graph::semantics::NodeSemanticsEntry;
use helixflow_graph::{
    GraphEdge, GraphNode, GraphService, ProposalOp, WorkflowGraph, port_type_label,
};
use helixflow_registry::catalog::ImplementationSelection;
use helixflow_registry::resolver::{CapabilityResolver, ConnectorAvailability, ResolveRequest};
use helixflow_registry::{NodeRegistry, RegistryError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const MAX_OPERATIONS: usize = 64;
const MAX_ID_CHARS: usize = 96;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasEditPlan {
    pub operations: Vec<CanvasEditOp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CanvasEditOp {
    AddNode {
        id: String,
        node_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<CanvasPosition>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default)]
        params: Value,
    },
    UpdateNode {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
    },
    MoveNode {
        id: String,
        position: CanvasPosition,
    },
    RemoveNode {
        id: String,
    },
    Connect {
        source: String,
        source_handle: String,
        target: String,
        target_handle: String,
    },
    Disconnect {
        source: String,
        source_handle: String,
        target: String,
        target_handle: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasPosition {
    pub x: f32,
    pub y: f32,
}

impl CanvasPosition {
    fn as_pos(self) -> [f32; 2] {
        [self.x, self.y]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasEditError {
    Empty,
    TooManyOperations(usize),
    InvalidId(String),
    UnknownNodeType(String),
    UnknownNode(String),
    ParamsNotObject(String),
    MissingParam {
        node_id: String,
        param: String,
    },
    PortMismatch {
        from: String,
        to: String,
    },
    MissingPort {
        node_id: String,
        port: String,
    },
    Resolve {
        node_id: String,
        code: String,
        reason: String,
    },
    Graph(String),
}

impl CanvasEditError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingParam { .. } => "REQUIRED_INPUT_MISSING",
            Self::Resolve { code, .. } => match code.as_str() {
                "BINDING_NOT_FOUND" => "BINDING_NOT_FOUND",
                "MODEL_NOT_FOUND" => "MODEL_NOT_FOUND",
                "MODEL_AMBIGUOUS" => "MODEL_AMBIGUOUS",
                "BINDING_AMBIGUOUS" => "BINDING_AMBIGUOUS",
                "BINDING_UNAVAILABLE" => "BINDING_UNAVAILABLE",
                "CAPABILITY_NOT_FOUND" => "CAPABILITY_NOT_FOUND",
                _ => "CANVAS_EDIT_INVALID",
            },
            _ => "CANVAS_EDIT_INVALID",
        }
    }

    pub fn is_clarify(&self) -> bool {
        match self {
            Self::MissingParam { .. } => true,
            Self::Resolve { code, .. } => matches!(
                code.as_str(),
                "BINDING_NOT_FOUND"
                    | "MODEL_NOT_FOUND"
                    | "MODEL_AMBIGUOUS"
                    | "BINDING_AMBIGUOUS"
                    | "BINDING_UNAVAILABLE"
            ),
            _ => false,
        }
    }
}

impl std::fmt::Display for CanvasEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "canvas edit has no operations"),
            Self::TooManyOperations(count) => {
                write!(
                    f,
                    "canvas edit has {count} operations; the limit is {MAX_OPERATIONS}"
                )
            }
            Self::InvalidId(id) => write!(f, "invalid canvas node id `{id}`"),
            Self::UnknownNodeType(node_type) => write!(f, "unknown node type `{node_type}`"),
            Self::UnknownNode(id) => write!(f, "unknown node `{id}`"),
            Self::ParamsNotObject(id) => write!(f, "node `{id}` params must be an object"),
            Self::MissingParam { node_id, param } => {
                write!(f, "REQUIRED_INPUT_MISSING {node_id}.{param}")
            }
            Self::PortMismatch { from, to } => {
                write!(f, "handle type mismatch from `{from}` to `{to}`")
            }
            Self::MissingPort { node_id, port } => {
                write!(f, "missing handle `{port}` on node `{node_id}`")
            }
            Self::Resolve {
                node_id,
                code,
                reason,
            } => write!(f, "{code} on `{node_id}`: {reason}"),
            Self::Graph(reason) => write!(f, "{reason}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompiledCanvasEdit {
    pub ops: Vec<ProposalOp>,
    pub preview: WorkflowGraph,
}

pub fn compile_canvas_edit(
    plan: &CanvasEditPlan,
    base: &WorkflowGraph,
    registry: &NodeRegistry,
) -> Result<CompiledCanvasEdit, CanvasEditError> {
    let catalog = helixflow_run::shared_catalog();
    let availability: ConnectorAvailability = catalog
        .connectors
        .iter()
        .map(|connector| (connector.connector_id.clone(), true))
        .collect();
    compile_canvas_edit_with(plan, base, registry, &availability, None)
}

pub fn compile_canvas_edit_with(
    plan: &CanvasEditPlan,
    base: &WorkflowGraph,
    registry: &NodeRegistry,
    availability: &ConnectorAvailability,
    connector_preference: Option<&str>,
) -> Result<CompiledCanvasEdit, CanvasEditError> {
    if plan.operations.is_empty() {
        return Err(CanvasEditError::Empty);
    }
    if plan.operations.len() > MAX_OPERATIONS {
        return Err(CanvasEditError::TooManyOperations(plan.operations.len()));
    }

    let catalog = helixflow_run::shared_catalog();
    let resolver = CapabilityResolver::new(catalog);
    let service = GraphService::new(registry.clone());
    let mut graph = base.clone();
    let mut ops = Vec::new();

    for operation in &plan.operations {
        let next = translate_op(
            operation,
            &graph,
            registry,
            &resolver,
            availability,
            connector_preference,
        )?;
        for op in next {
            service
                .apply_op_in_place(&mut graph, &op)
                .map_err(|error| CanvasEditError::Graph(error.to_string()))?;
            ops.push(op);
        }
    }

    for (node_id, node) in &graph.nodes {
        validate_node_params(registry, node_id, node)?;
    }

    Ok(CompiledCanvasEdit {
        ops,
        preview: graph,
    })
}

fn translate_op(
    operation: &CanvasEditOp,
    graph: &WorkflowGraph,
    registry: &NodeRegistry,
    resolver: &CapabilityResolver<'_>,
    availability: &ConnectorAvailability,
    connector_preference: Option<&str>,
) -> Result<Vec<ProposalOp>, CanvasEditError> {
    match operation {
        CanvasEditOp::AddNode {
            id,
            node_type,
            title,
            position,
            model,
            params,
        } => {
            ensure_id(id)?;
            let definition = registry
                .definition(node_type)
                .map_err(|_| CanvasEditError::UnknownNodeType(node_type.clone()))?;
            let mut params = params_object(id, params)?;
            let semantics = resolve_semantics(
                id,
                definition.capability.as_deref(),
                model.as_deref(),
                &mut params,
                resolver,
                availability,
                connector_preference,
            )?;
            Ok(vec![ProposalOp::AddNode {
                id: id.clone(),
                node: GraphNode {
                    node_type: node_type.clone(),
                    title: title
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or(&definition.title)
                        .to_owned(),
                    params: Value::Object(params),
                    pos: position.map(CanvasPosition::as_pos).unwrap_or([0.0, 0.0]),
                    size: None,
                    semantics,
                },
            }])
        }
        CanvasEditOp::UpdateNode { id, params } => {
            ensure_id(id)?;
            if !graph.nodes.contains_key(id) {
                return Err(CanvasEditError::UnknownNode(id.clone()));
            }
            let Some(params) = params else {
                return Ok(Vec::new());
            };
            let object = params_object(id, params)?;
            Ok(object
                .into_iter()
                .map(|(key, value)| ProposalOp::SetParam {
                    id: id.clone(),
                    key,
                    prev: None,
                    value,
                })
                .collect())
        }
        CanvasEditOp::MoveNode { id, position } => {
            ensure_id(id)?;
            if !graph.nodes.contains_key(id) {
                return Err(CanvasEditError::UnknownNode(id.clone()));
            }
            Ok(vec![ProposalOp::MoveNode {
                id: id.clone(),
                pos: position.as_pos(),
            }])
        }
        CanvasEditOp::RemoveNode { id } => {
            ensure_id(id)?;
            if !graph.nodes.contains_key(id) {
                return Err(CanvasEditError::UnknownNode(id.clone()));
            }
            Ok(vec![ProposalOp::RemoveNode { id: id.clone() }])
        }
        CanvasEditOp::Connect {
            source,
            source_handle,
            target,
            target_handle,
        } => {
            ensure_id(source)?;
            ensure_id(target)?;
            let edge = typed_edge(
                graph,
                registry,
                source,
                source_handle,
                target,
                target_handle,
            )?;
            Ok(vec![ProposalOp::AddEdge { edge }])
        }
        CanvasEditOp::Disconnect {
            source,
            source_handle,
            target,
            target_handle,
        } => {
            ensure_id(source)?;
            ensure_id(target)?;
            let edge = graph
                .edges
                .iter()
                .find(|edge| {
                    edge.from[0] == *source
                        && edge.from[1] == *source_handle
                        && edge.to[0] == *target
                        && edge.to[1] == *target_handle
                })
                .cloned()
                .ok_or_else(|| {
                    CanvasEditError::Graph(format!(
                        "no edge from `{source}.{source_handle}` to `{target}.{target_handle}`"
                    ))
                })?;
            Ok(vec![ProposalOp::RemoveEdge { edge }])
        }
    }
}

fn typed_edge(
    graph: &WorkflowGraph,
    registry: &NodeRegistry,
    source: &str,
    source_handle: &str,
    target: &str,
    target_handle: &str,
) -> Result<GraphEdge, CanvasEditError> {
    let source_node = graph
        .nodes
        .get(source)
        .ok_or_else(|| CanvasEditError::UnknownNode(source.to_owned()))?;
    let target_node = graph
        .nodes
        .get(target)
        .ok_or_else(|| CanvasEditError::UnknownNode(target.to_owned()))?;
    let source_def = registry
        .definition(&source_node.node_type)
        .map_err(|_| CanvasEditError::UnknownNodeType(source_node.node_type.clone()))?;
    let target_def = registry
        .definition(&target_node.node_type)
        .map_err(|_| CanvasEditError::UnknownNodeType(target_node.node_type.clone()))?;
    let output = source_def
        .outputs
        .iter()
        .find(|port| port.name == source_handle)
        .ok_or_else(|| CanvasEditError::MissingPort {
            node_id: source.to_owned(),
            port: source_handle.to_owned(),
        })?;
    let input = target_def
        .inputs
        .iter()
        .find(|port| port.name == target_handle)
        .ok_or_else(|| CanvasEditError::MissingPort {
            node_id: target.to_owned(),
            port: target_handle.to_owned(),
        })?;
    if output.port_type != input.port_type {
        return Err(CanvasEditError::PortMismatch {
            from: format!("{source}.{source_handle}"),
            to: format!("{target}.{target_handle}"),
        });
    }
    Ok(GraphEdge {
        from: [source.to_owned(), source_handle.to_owned()],
        to: [target.to_owned(), target_handle.to_owned()],
        edge_type: port_type_label(output.port_type).to_owned(),
    })
}

fn resolve_semantics(
    node_id: &str,
    capability_id: Option<&str>,
    requested_model: Option<&str>,
    params: &mut Map<String, Value>,
    resolver: &CapabilityResolver<'_>,
    availability: &ConnectorAvailability,
    connector_preference: Option<&str>,
) -> Result<Option<NodeSemanticsEntry>, CanvasEditError> {
    let Some(capability_id) = capability_id else {
        return Ok(None);
    };
    let model = requested_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| params.get("model").and_then(Value::as_str))
        .map(str::to_owned);
    params.remove("model");
    let request = ResolveRequest {
        capability_id: capability_id.to_owned(),
        requested_model: model,
        connector_preference: connector_preference.map(str::to_owned),
    };
    match resolver.resolve(&request, availability) {
        Ok(resolved) => {
            if let Some(binding) = helixflow_run::shared_catalog().binding(&resolved.binding_id) {
                merge_defaults(params, &binding.defaults);
            }
            Ok(Some(NodeSemanticsEntry {
                capability_id: capability_id.to_owned(),
                mode: capability_id.to_owned(),
                implementation: ImplementationSelection::Pinned {
                    requested_model_id: resolved.resolved_model_id,
                    binding_id: resolved.binding_id,
                },
            }))
        }
        Err(error) => Err(CanvasEditError::Resolve {
            node_id: node_id.to_owned(),
            code: error.code().to_owned(),
            reason: error.to_string(),
        }),
    }
}

fn merge_defaults(params: &mut Map<String, Value>, defaults: &Value) {
    let Some(object) = defaults.as_object() else {
        return;
    };
    for (key, value) in object {
        params.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

fn validate_node_params(
    registry: &NodeRegistry,
    node_id: &str,
    node: &GraphNode,
) -> Result<(), CanvasEditError> {
    match registry.validate_node_params(&node.node_type, &node.params) {
        Ok(()) => Ok(()),
        Err(RegistryError::MissingRequiredParam { param, .. }) => {
            Err(CanvasEditError::MissingParam {
                node_id: node_id.to_owned(),
                param,
            })
        }
        Err(RegistryError::ParamsNotObject(_)) => {
            Err(CanvasEditError::ParamsNotObject(node_id.to_owned()))
        }
        Err(error) => Err(CanvasEditError::Graph(error.to_string())),
    }
}

fn params_object(node_id: &str, params: &Value) -> Result<Map<String, Value>, CanvasEditError> {
    if params.is_null() {
        return Ok(Map::new());
    }
    params
        .as_object()
        .cloned()
        .ok_or_else(|| CanvasEditError::ParamsNotObject(node_id.to_owned()))
}

fn ensure_id(id: &str) -> Result<(), CanvasEditError> {
    if is_bounded_identifier(id) {
        Ok(())
    } else {
        Err(CanvasEditError::InvalidId(id.to_owned()))
    }
}

fn is_bounded_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub fn compact_catalog_index(catalog: &Value) -> Value {
    let nodes = catalog
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let types = nodes
        .iter()
        .filter_map(|node| {
            let node_type = node.get("type")?.as_str()?;
            Some(json!({
                "type": node_type,
                "category": node.get("category").and_then(Value::as_str).unwrap_or(""),
                "title": node.get("title").and_then(Value::as_str).unwrap_or(node_type),
                "inputs": port_names(node.get("inputs")),
                "outputs": port_names(node.get("outputs")),
                "description": node.get("description").and_then(Value::as_str).unwrap_or(""),
            }))
        })
        .collect::<Vec<_>>();
    json!({ "types": types })
}

pub fn catalog_entries(catalog: &Value, types: &[String]) -> Value {
    if types.is_empty() {
        return compact_catalog_index(catalog);
    }
    let wanted = types
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let nodes = catalog
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|node| {
            node.get("type")
                .and_then(Value::as_str)
                .is_some_and(|node_type| wanted.contains(node_type))
        })
        .collect::<Vec<_>>();
    json!({ "types": nodes })
}

fn port_names(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|port| port.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

pub fn inspect_canvas(state: &Value, arguments: &Value) -> Value {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("nodes");
    let graph = state.get("graph").cloned().unwrap_or(json!({}));
    match action {
        "node" => {
            let node_id = arguments
                .get("node_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let node = graph
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|node| node.get("id").and_then(Value::as_str) == Some(node_id))
                .cloned();
            json!({ "node": node, "seq": state.get("base_version_id") })
        }
        "edges" => {
            let ids = string_list(arguments.get("node_ids"));
            let edges = graph
                .get("edges")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|edge| ids.is_empty() || edge_touches(edge, &ids))
                .collect::<Vec<_>>();
            json!({ "edges": edges })
        }
        _ => {
            let query = arguments
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            let types = string_list(arguments.get("node_types"));
            let ids = string_list(arguments.get("node_ids"));
            let limit = arguments
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .clamp(1, 200) as usize;
            let nodes = graph
                .get("nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|node| {
                    let id = node.get("id").and_then(Value::as_str).unwrap_or("");
                    let node_type = node.get("node_type").and_then(Value::as_str).unwrap_or("");
                    let title = node.get("title").and_then(Value::as_str).unwrap_or("");
                    if !ids.is_empty() && !ids.iter().any(|wanted| wanted == id) {
                        return false;
                    }
                    if !types.is_empty() && !types.iter().any(|wanted| wanted == node_type) {
                        return false;
                    }
                    if query.is_empty() {
                        return true;
                    }
                    id.to_ascii_lowercase().contains(&query)
                        || node_type.to_ascii_lowercase().contains(&query)
                        || title.to_ascii_lowercase().contains(&query)
                })
                .take(limit)
                .collect::<Vec<_>>();
            json!({
                "workspace_id": state.get("workspace_id"),
                "nodes": nodes,
                "node_count": graph.get("node_count").cloned().unwrap_or(json!(nodes.len())),
                "selection": state.get("selection"),
                "gates": state.get("gates"),
            })
        }
    }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn edge_touches(edge: &Value, ids: &[String]) -> bool {
    let from = edge
        .pointer("/from/0")
        .and_then(Value::as_str)
        .unwrap_or("");
    let to = edge.pointer("/to/0").and_then(Value::as_str).unwrap_or("");
    ids.iter().any(|id| id == from || id == to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn compiles_prompt_to_image_to_video_with_typed_handles() {
        let plan: CanvasEditPlan = serde_json::from_value(json!({
            "operations": [
                {
                    "op": "add_node",
                    "id": "s1",
                    "node_type": "image.generate",
                    "model": "Nano Banana",
                    "params": { "prompt": "a mug" }
                },
                {
                    "op": "add_node",
                    "id": "s2",
                    "node_type": "video.image_to_video",
                    "model": "Seedance 2",
                    "position": { "x": 280, "y": 0 },
                    "params": {}
                },
                {
                    "op": "connect",
                    "source": "s1",
                    "source_handle": "image",
                    "target": "s2",
                    "target_handle": "image"
                }
            ]
        }))
        .expect("plan");
        let compiled = compile_canvas_edit(
            &plan,
            &WorkflowGraph {
                schema_version: 1,
                catalog_revision: None,
                nodes: BTreeMap::new(),
                edges: Vec::new(),
            },
            &NodeRegistry::builtin(),
        )
        .expect("compile");
        assert_eq!(compiled.preview.nodes.len(), 2);
        assert_eq!(compiled.preview.edges.len(), 1);
        assert_eq!(compiled.preview.edges[0].edge_type, "image");
        let image = &compiled.preview.nodes["s1"];
        assert_eq!(image.params["prompt"], "a mug");
        assert_eq!(image.params["aspect_ratio"], "1:1");
        assert!(image.params.get("model").is_none());
        let semantics = image.semantics.as_ref().expect("image semantics");
        assert_eq!(semantics.capability_id, "text_to_image");
        assert!(matches!(
            &semantics.implementation,
            ImplementationSelection::Pinned { requested_model_id, .. }
                if requested_model_id == "google/nano-banana-2"
        ));
        let video = compiled.preview.nodes["s2"]
            .semantics
            .as_ref()
            .expect("video");
        assert_eq!(video.capability_id, "image_to_video");
        assert!(matches!(
            &video.implementation,
            ImplementationSelection::Pinned { requested_model_id, .. }
                if requested_model_id == "bytedance/seedance-2.0-fast"
        ));
    }

    #[test]
    fn missing_prompt_is_a_clarify_error() {
        let plan: CanvasEditPlan = serde_json::from_value(json!({
            "operations": [{
                "op": "add_node",
                "id": "s1",
                "node_type": "image.generate",
                "params": {}
            }]
        }))
        .expect("plan");
        let error = compile_canvas_edit(
            &plan,
            &WorkflowGraph {
                schema_version: 1,
                catalog_revision: None,
                nodes: BTreeMap::new(),
                edges: Vec::new(),
            },
            &NodeRegistry::builtin(),
        )
        .expect_err("missing prompt");
        assert!(error.is_clarify());
        assert_eq!(error.code(), "REQUIRED_INPUT_MISSING");
        assert!(error.to_string().contains("s1.prompt"));
    }

    #[test]
    fn rejects_mismatched_handles() {
        let plan: CanvasEditPlan = serde_json::from_value(json!({
            "operations": [
                {
                    "op": "add_node",
                    "id": "text",
                    "node_type": "input.text",
                    "params": { "text": "hello" }
                },
                {
                    "op": "add_node",
                    "id": "image",
                    "node_type": "image.generate",
                    "params": { "prompt": "hi" }
                },
                {
                    "op": "connect",
                    "source": "text",
                    "source_handle": "text",
                    "target": "image",
                    "target_handle": "in"
                }
            ]
        }))
        .expect("plan");
        let error = compile_canvas_edit(
            &plan,
            &WorkflowGraph {
                schema_version: 1,
                catalog_revision: None,
                nodes: BTreeMap::new(),
                edges: Vec::new(),
            },
            &NodeRegistry::builtin(),
        )
        .expect_err("type mismatch");
        assert!(!error.is_clarify());
        assert_eq!(error.code(), "CANVAS_EDIT_INVALID");
    }
}
