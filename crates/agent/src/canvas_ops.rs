use std::collections::BTreeSet;

use helixflow_graph::{GraphEdge, WorkflowGraph};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub(crate) const MAX_CANVAS_STATE_BYTES: usize = 256 * 1024;
const MAX_CANVAS_NODES: usize = 64;
const MAX_CANVAS_EDGES: usize = 96;
const MAX_SELECTION_NODES: usize = 64;
const MAX_IDENTIFIER_CHARS: usize = 96;
const MAX_TITLE_CHARS: usize = 120;
const MAX_PARAM_DEPTH: usize = 4;
const MAX_PARAM_ITEMS: usize = 12;
const MAX_PARAM_KEY_CHARS: usize = 64;
const MAX_PARAM_TEXT_CHARS: usize = 768;
const MAX_PARAM_VALUE_CHARS: usize = 240;

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
        let compact_graph = CompactCanvasGraph::from_graph(graph);
        let node_ids = compact_graph
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        Self {
            schema_version: 1,
            workspace_id: workspace_id.to_owned(),
            base_version_id: base_version_id.to_owned(),
            graph: compact_graph,
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
        let nodes = graph
            .nodes
            .iter()
            .filter(|(id, node)| {
                is_bounded_identifier(id) && is_bounded_identifier(&node.node_type)
            })
            .take(MAX_CANVAS_NODES)
            .map(|(id, node)| CompactCanvasNode {
                id: id.clone(),
                node_type: node.node_type.clone(),
                title: safe_canvas_text(&node.title, MAX_TITLE_CHARS),
                params: safe_canvas_params(&node.params),
                pos: node.pos,
            })
            .collect::<Vec<_>>();
        let projected_ids = nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        Self {
            node_count: graph.nodes.len(),
            edge_count: graph.edges.len(),
            nodes,
            edges: graph
                .edges
                .iter()
                .filter(|edge| {
                    projected_ids.contains(edge.from[0].as_str())
                        && projected_ids.contains(edge.to[0].as_str())
                        && is_bounded_identifier(&edge.from[1])
                        && is_bounded_identifier(&edge.to[1])
                        && is_bounded_identifier(&edge.edge_type)
                })
                .take(MAX_CANVAS_EDGES)
                .cloned()
                .collect(),
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

struct CanvasParamBudget {
    remaining_items: usize,
    remaining_text_chars: usize,
}

fn safe_canvas_params(value: &Value) -> Value {
    let mut budget = CanvasParamBudget {
        remaining_items: MAX_PARAM_ITEMS,
        remaining_text_chars: MAX_PARAM_TEXT_CHARS,
    };
    safe_canvas_value(None, value, 0, &mut budget)
}

fn safe_canvas_value(
    key: Option<&str>,
    value: &Value,
    depth: usize,
    budget: &mut CanvasParamBudget,
) -> Value {
    if depth >= MAX_PARAM_DEPTH || budget.remaining_items == 0 {
        return Value::String("[truncated]".to_owned());
    }
    budget.remaining_items -= 1;
    if key.is_some_and(is_sensitive_param_key) {
        return Value::String("[redacted]".to_owned());
    }
    match value {
        Value::Object(object) => {
            let mut projected = Map::new();
            for (key, value) in object {
                if budget.remaining_items == 0 {
                    break;
                }
                if is_bounded_component(key, MAX_PARAM_KEY_CHARS) {
                    projected.insert(
                        key.clone(),
                        safe_canvas_value(Some(key), value, depth + 1, budget),
                    );
                }
            }
            Value::Object(projected)
        }
        Value::Array(items) => {
            let mut projected = Vec::new();
            for item in items {
                if budget.remaining_items == 0 {
                    break;
                }
                projected.push(safe_canvas_value(None, item, depth + 1, budget));
            }
            Value::Array(projected)
        }
        Value::String(text) if is_sensitive_param_value(text) => {
            Value::String("[redacted]".to_owned())
        }
        Value::String(text) => {
            let limit = MAX_PARAM_VALUE_CHARS.min(budget.remaining_text_chars);
            let text = text.chars().take(limit).collect::<String>();
            budget.remaining_text_chars -= text.chars().count();
            Value::String(text)
        }
        scalar => scalar.clone(),
    }
}

fn safe_canvas_text(value: &str, max_chars: usize) -> String {
    if is_sensitive_param_value(value) {
        "[redacted]".to_owned()
    } else {
        value.chars().take(max_chars).collect()
    }
}

fn is_bounded_identifier(value: &str) -> bool {
    is_bounded_component(value, MAX_IDENTIFIER_CHARS)
}

fn is_bounded_component(value: &str, max_chars: usize) -> bool {
    value.len() <= max_chars
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
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
        "access_key",
        "private_key",
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
        || [
            "api_key",
            "apikey",
            "access_key",
            "private_key",
            "client_secret",
            "authorization",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
        || value.split_whitespace().any(looks_like_credential_segment)
        || value.starts_with("/Users/")
        || value.starts_with("/private/")
        || value.starts_with("file://")
}

fn looks_like_credential_segment(segment: &str) -> bool {
    let segment = segment.trim_matches(|character: char| {
        !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-' | '.')
    });
    let lower = segment.to_ascii_lowercase();
    let known_prefix = [
        "sk-",
        "sk_live_",
        "rk_live_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "hf_",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
        "npm_",
        "pypi-",
        "aiza",
        "ya29.",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));
    let aws_access_key = segment.len() == 20
        && (segment.starts_with("AKIA") || segment.starts_with("ASIA"))
        && segment
            .chars()
            .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit());
    let jwt = lower.starts_with("eyj") && segment.matches('.').count() == 2 && segment.len() >= 40;
    let opaque_token = segment.len() >= 32
        && !segment.chars().any(char::is_whitespace)
        && segment
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && segment.chars().any(|character| character.is_ascii_digit())
        && segment.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '=' | '+')
        });
    known_prefix || aws_access_key || jwt || opaque_token
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
                .take(MAX_SELECTION_NODES)
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
                        "prompt": "product shot\nwith soft shadows",
                        "duration_sec": 4,
                        "aspect_ratio": "9:16",
                        "api_key": "sk-secret",
                        "github_note": "ghp_1234567890abcdefghijklmnopqrstuvwxyz",
                        "aws_note": "AKIAIOSFODNN7EXAMPLE",
                        "opaque_note": "abcdefghijklmnopqrstuvwxyz1234567890AB",
                        "storage_uri": "/Users/example/private.mov",
                        "zz_samples": (0..40).collect::<Vec<_>>(),
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
        assert_eq!(params["prompt"], "product shot\nwith soft shadows");
        assert_eq!(params["duration_sec"], 4);
        assert_eq!(params["aspect_ratio"], "9:16");
        assert_eq!(params["api_key"], "[redacted]");
        assert_eq!(params["github_note"], "[redacted]");
        assert_eq!(params["aws_note"], "[redacted]");
        assert_eq!(params["opaque_note"], "[redacted]");
        assert_eq!(params["storage_uri"], "[redacted]");
        assert!(params["zz_samples"].as_array().expect("samples").len() < 32);
        let encoded = serde_json::to_string(params).expect("params json");
        assert!(!encoded.contains("sk-secret"));
        assert!(!encoded.contains("/Users/example"));
    }

    #[test]
    fn compact_canvas_stays_within_the_dynamic_tool_byte_limit() {
        let long_text = "x".repeat(500);
        let params = Value::Object(
            (0..40)
                .map(|index| {
                    (
                        format!("parameter_{index}_{}", "k".repeat(30)),
                        json!(long_text),
                    )
                })
                .collect(),
        );
        let nodes = (0..200)
            .map(|index| {
                let id = format!("node_{index:03}_{}", "n".repeat(70));
                (
                    id,
                    GraphNode {
                        node_type: "custom.node".to_owned(),
                        title: "t".repeat(500),
                        params: params.clone(),
                        pos: [index as f32, index as f32],
                        size: None,
                        semantics: None,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let node_ids = nodes.keys().cloned().collect::<Vec<_>>();
        let edges = node_ids
            .windows(2)
            .map(|pair| GraphEdge {
                from: [pair[0].clone(), "output".repeat(15)],
                to: [pair[1].clone(), "input".repeat(16)],
                edge_type: "artifact".repeat(15),
            })
            .collect();
        let graph = WorkflowGraph {
            schema_version: 1,
            catalog_revision: None,
            nodes,
            edges,
        };
        let context = CanvasOpsContext::from_graph(
            "ws_1",
            "ver_1",
            &graph,
            CanvasSelection { node_ids },
            CanvasGateState::default(),
        );

        let bytes = serde_json::to_vec(&context).expect("compact canvas json");
        assert!(
            bytes.len() <= 256 * 1024,
            "compact canvas is {} bytes",
            bytes.len()
        );
    }
}
