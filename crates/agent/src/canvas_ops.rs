use std::collections::BTreeSet;

use helixflow_graph::{GraphEdge, WorkflowGraph};
use serde::{Deserialize, Serialize};

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
    pub pos: [f32; 2],
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
                    "Write run_request.json; backend creates pending confirmation.",
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
