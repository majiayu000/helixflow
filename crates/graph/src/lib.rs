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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_module_name() {
        assert_eq!(module_name(), "graph");
    }

    #[test]
    fn serializes_workflow_graph_boundary() {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            "prompt".to_string(),
            GraphNode {
                node_type: "text.prompt".to_string(),
                title: "Prompt".to_string(),
                params: serde_json::json!({ "text": "hello" }),
                pos: [12.0, 24.0],
            },
        );
        let graph = WorkflowGraph {
            schema_version: 1,
            nodes,
            edges: vec![GraphEdge {
                from: ["prompt".to_string(), "out".to_string()],
                to: ["render".to_string(), "in".to_string()],
                edge_type: "artifact".to_string(),
            }],
        };

        let encoded = serde_json::to_value(&graph).expect("serialize graph");

        assert_eq!(encoded["schema_version"], 1);
        assert_eq!(encoded["nodes"]["prompt"]["node_type"], "text.prompt");
        assert_eq!(encoded["edges"][0]["edge_type"], "artifact");
    }
}
