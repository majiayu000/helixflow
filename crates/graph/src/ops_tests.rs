use std::collections::BTreeMap;

use helixflow_registry::NodeRegistry;
use serde_json::json;

use super::{GraphNode, GraphService, ProposalOp, WorkflowGraph};

#[test]
fn applies_one_operation_to_the_existing_graph() {
    let service = GraphService::new(NodeRegistry::builtin());
    let mut graph = WorkflowGraph {
        schema_version: 1,
        catalog_revision: None,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
    };
    let operation = ProposalOp::AddNode {
        id: "text".to_owned(),
        node: GraphNode {
            node_type: "input.text".to_owned(),
            title: "Text".to_owned(),
            params: json!({ "text": "hello" }),
            pos: [40.0, 80.0],
            size: None,
            semantics: None,
        },
    };

    service
        .apply_op_in_place(&mut graph, &operation)
        .expect("operation applies");

    assert_eq!(graph.nodes["text"].pos, [40.0, 80.0]);
}
