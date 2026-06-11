use std::collections::BTreeMap;

pub fn module_name() -> &'static str {
    "graph"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkflowGraph {
    pub schema_version: u32,
    pub nodes: BTreeMap<String, GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphNode {
    pub node_type: String,
    pub title: String,
    pub params: serde_json::Value,
    pub pos: [f32; 2],
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphEdge {
    pub from: [String; 2],
    pub to: [String; 2],
    pub edge_type: String,
}
