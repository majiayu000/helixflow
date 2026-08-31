use std::collections::BTreeSet;

use helixflow_graph::{GraphEdge, WorkflowGraph};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CanvasOpsContext {
    pub schema_version: u32,
    pub workspace_id: String,
    pub base_version_id: String,
    pub graph: CompactCanvasGraph,
    pub selection: CanvasSelection,
    pub gates: CanvasGateState,
}

impl CanvasOpsContext {
    pub fn from_graph(
        workspace_id: &str,
        base_version_id: &str,
        graph: &WorkflowGraph,
        selection: CanvasSelection,
        gates: CanvasGateState,
    ) -> Self {
        let node_ids = graph.nodes.keys().cloned().collect::<BTreeSet<_>>();
        Self {
            schema_version: 1,
            workspace_id: workspace_id.to_owned(),
            base_version_id: base_version_id.to_owned(),
            graph: CompactCanvasGraph::from_graph(graph),
            selection: selection.filtered(&node_ids),
            gates,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompactCanvasGraph {
    pub node_count: usize,
    pub edge_count: usize,
    pub nodes: Vec<CompactCanvasNode>,
    pub edges: Vec<GraphEdge>,
}

impl CompactCanvasGraph {
    fn from_graph(graph: &WorkflowGraph) -> Self {
        Self {
            node_count: graph.nodes.len(),
            edge_count: graph.edges.len(),
            nodes: graph
                .nodes
                .iter()
                .map(|(id, node)| CompactCanvasNode {
                    id: id.clone(),
                    node_type: node.node_type.clone(),
                    title: node.title.clone(),
                    params: safe_canvas_params(&node.params, 0),
                    pos: node.pos,
                })
                .collect(),
            edges: graph.edges.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompactCanvasNode {
    pub id: String,
    pub node_type: String,
    pub title: String,
    pub params: Value,
    pub pos: [f32; 2],
}

fn safe_canvas_params(value: &Value, depth: usize) -> Value {
    const MAX_DEPTH: usize = 4;
    const MAX_ITEMS: usize = 32;
    const MAX_TEXT_CHARS: usize = 240;
    if depth >= MAX_DEPTH {
        return Value::String("[truncated]".to_owned());
    }
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .take(MAX_ITEMS)
                .map(|(key, value)| {
                    let value = if is_sensitive_param_key(key) {
                        Value::String("[redacted]".to_owned())
                    } else {
                        safe_canvas_params(value, depth + 1)
                    };
                    (key.clone(), value)
                })
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(MAX_ITEMS)
                .map(|item| safe_canvas_params(item, depth + 1))
                .collect(),
        ),
        Value::String(text) if is_sensitive_param_value(text) => {
            Value::String("[redacted]".to_owned())
        }
        Value::String(text) => Value::String(
            text.lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(MAX_TEXT_CHARS)
                .collect(),
        ),
        scalar => scalar.clone(),
    }
}

fn is_sensitive_param_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "authorization",
        "api_key",
        "apikey",
        "credential",
        "cookie",
        "url",
        "uri",
        "path",
    ]
    .iter()
    .any(|candidate| key.contains(candidate))
}

fn is_sensitive_param_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("bearer ")
        || lower
            .split(|character: char| {
                character.is_whitespace()
                    || matches!(character, '"' | '\'' | '`' | ',' | ':' | ';' | '(' | '[')
            })
            .any(|segment| segment.starts_with("sk-"))
        || value.starts_with("/Users/")
        || value.starts_with("/private/")
        || value.starts_with("file://")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasSelection {
    pub node_ids: Vec<String>,
}

impl CanvasSelection {
    fn filtered(self, known_node_ids: &BTreeSet<String>) -> Self {
        let mut seen = BTreeSet::new();
        Self {
            node_ids: self
                .node_ids
                .into_iter()
                .filter(|id| known_node_ids.contains(id) && seen.insert(id.clone()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasGateState {
    pub pending_proposal: bool,
    pub pending_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasOpsContract {
    pub schema_version: u32,
    pub allowed_ops: Vec<CanvasOpSpec>,
}

impl CanvasOpsContract {
    pub fn v1() -> Self {
        Self {
            schema_version: 1,
            allowed_ops: vec![
                CanvasOpSpec::new("read_state", "Read ctx/canvas_state.json."),
                CanvasOpSpec::new(
                    "read_selection",
                    "Read ctx/canvas_state.json selection.node_ids.",
                ),
                CanvasOpSpec::new(
                    "propose_layout",
                    "Write proposal.json with move_node ops; do not call layout save.",
                ),
                CanvasOpSpec::new(
                    "propose_graph_ops",
                    "Write proposal.json with bounded proposal ops.",
                ),
                CanvasOpSpec::new(
                    "run_selected_workflow",
                    "Write run_request.json; backend estimates cost and decides auto-start or confirmation.",
                ),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanvasOpSpec {
    pub op: String,
    pub behavior: String,
}

impl CanvasOpSpec {
    fn new(op: &str, behavior: &str) -> Self {
        Self {
            op: op.to_owned(),
            behavior: behavior.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use helixflow_graph::GraphNode;
    use serde_json::json;

    #[test]
    fn compact_canvas_exposes_bounded_redacted_node_params() {
        let graph = WorkflowGraph {
            schema_version: 1,
            catalog_revision: None,
            nodes: BTreeMap::from([(
                "video".to_owned(),
                GraphNode {
                    node_type: "video.text_to_video".to_owned(),
                    title: "Video".to_owned(),
                    params: json!({
                        "model": "seedance-v1.5-pro",
                        "prompt": "product shot",
                        "duration_sec": 4,
                        "aspect_ratio": "9:16",
                        "api_key": "sk-secret",
                        "storage_uri": "/Users/example/private.mov",
                        "samples": (0..40).collect::<Vec<_>>(),
                    }),
                    pos: [0.0, 0.0],
                    size: None,
                    semantics: None,
                },
            )]),
            edges: Vec::new(),
        };

        let value = serde_json::to_value(CanvasOpsContext::from_graph(
            "ws_1",
            "ver_1",
            &graph,
            CanvasSelection::default(),
            CanvasGateState::default(),
        ))
        .expect("canvas context");
        let params = &value["graph"]["nodes"][0]["params"];

        assert_eq!(params["model"], "seedance-v1.5-pro");
        assert_eq!(params["prompt"], "product shot");
        assert_eq!(params["duration_sec"], 4);
        assert_eq!(params["aspect_ratio"], "9:16");
        assert_eq!(params["api_key"], "[redacted]");
        assert_eq!(params["storage_uri"], "[redacted]");
        assert_eq!(params["samples"].as_array().expect("samples").len(), 32);
        let encoded = serde_json::to_string(params).expect("params json");
        assert!(!encoded.contains("sk-secret"));
        assert!(!encoded.contains("/Users/example"));
    }
}
