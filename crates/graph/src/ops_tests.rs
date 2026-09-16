use std::collections::BTreeMap;

use helixflow_registry::NodeRegistry;
use serde_json::json;

use super::{GraphNode, GraphService, ProposalOp, WorkflowGraph, matching_connection_ports};

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

#[test]
fn matching_ports_prefer_execution_input_on_image_edit() {
    let registry = NodeRegistry::builtin();
    let ports = matching_connection_ports(
        registry.definition("input.image").expect("input.image"),
        registry.definition("image.edit").expect("image.edit"),
    )
    .expect("lineage");
    assert_eq!(ports.source_port, "image");
    assert_eq!(ports.target_port, "image");
    assert_eq!(ports.edge_type, "image");
}

#[test]
fn matching_ports_use_reference_in_on_text_to_image() {
    let registry = NodeRegistry::builtin();
    let ports = matching_connection_ports(
        registry.definition("input.image").expect("input.image"),
        registry
            .definition("image.generate")
            .expect("image.generate"),
    )
    .expect("lineage");
    assert_eq!(ports.source_port, "image");
    assert_eq!(ports.target_port, "in");
    assert_eq!(ports.edge_type, "image");
}

#[test]
fn matching_ports_use_reference_in_on_image_to_image() {
    let registry = NodeRegistry::builtin();
    let ports = matching_connection_ports(
        registry.definition("input.image").expect("input.image"),
        registry.definition("input.image").expect("input.image"),
    )
    .expect("lineage");
    assert_eq!(ports.source_port, "image");
    assert_eq!(ports.target_port, "in");
    assert_eq!(ports.edge_type, "image");
}

#[test]
fn spawn_node_writes_lineage_edge_from_registry_ports() {
    let service = GraphService::new(NodeRegistry::builtin());
    let mut graph = WorkflowGraph {
        schema_version: 1,
        catalog_revision: None,
        nodes: BTreeMap::from([(
            "photo".to_owned(),
            GraphNode {
                node_type: "input.image".to_owned(),
                title: "Photo".to_owned(),
                params: json!({ "storage_uri": "upload://a" }),
                pos: [0.0, 0.0],
                size: None,
                semantics: None,
            },
        )]),
        edges: Vec::new(),
    };

    service
        .apply_op_in_place(
            &mut graph,
            &ProposalOp::SpawnNode {
                id: "image_generate".to_owned(),
                from: "photo".to_owned(),
                node: GraphNode {
                    node_type: "image.generate".to_owned(),
                    title: "Generate".to_owned(),
                    params: json!({ "prompt": "hero", "aspect_ratio": "1:1" }),
                    pos: [320.0, 0.0],
                    size: None,
                    semantics: None,
                },
            },
        )
        .expect("spawn applies");

    assert!(graph.nodes.contains_key("image_generate"));
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(
        graph.edges[0].from,
        ["photo".to_owned(), "image".to_owned()]
    );
    assert_eq!(
        graph.edges[0].to,
        ["image_generate".to_owned(), "in".to_owned()]
    );
    assert_eq!(graph.edges[0].edge_type, "image");
}

#[test]
fn spawn_image_edit_uses_execution_image_port() {
    let service = GraphService::new(NodeRegistry::builtin());
    let mut graph = WorkflowGraph {
        schema_version: 1,
        catalog_revision: None,
        nodes: BTreeMap::from([(
            "photo".to_owned(),
            GraphNode {
                node_type: "input.image".to_owned(),
                title: "Photo".to_owned(),
                params: json!({ "storage_uri": "upload://a" }),
                pos: [0.0, 0.0],
                size: None,
                semantics: None,
            },
        )]),
        edges: Vec::new(),
    };

    service
        .apply_op_in_place(
            &mut graph,
            &ProposalOp::SpawnNode {
                id: "image_edit".to_owned(),
                from: "photo".to_owned(),
                node: GraphNode {
                    node_type: "image.edit".to_owned(),
                    title: "Edit".to_owned(),
                    params: json!({ "prompt": "night" }),
                    pos: [320.0, 0.0],
                    size: None,
                    semantics: None,
                },
            },
        )
        .expect("spawn applies");

    assert_eq!(
        graph.edges[0].to,
        ["image_edit".to_owned(), "image".to_owned()]
    );
}

#[test]
fn spawn_node_rejects_missing_source() {
    let service = GraphService::new(NodeRegistry::builtin());
    let mut graph = WorkflowGraph {
        schema_version: 1,
        catalog_revision: None,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
    };
    let err = service
        .apply_op_in_place(
            &mut graph,
            &ProposalOp::SpawnNode {
                id: "image_generate".to_owned(),
                from: "photo".to_owned(),
                node: GraphNode {
                    node_type: "image.generate".to_owned(),
                    title: "Generate".to_owned(),
                    params: json!({ "prompt": "hero", "aspect_ratio": "1:1" }),
                    pos: [0.0, 0.0],
                    size: None,
                    semantics: None,
                },
            },
        )
        .expect_err("missing source");
    assert!(err.to_string().contains("missing node: photo"));
}

#[test]
fn spawn_grid_tiles_write_lineage_edges_and_validate() {
    let service = GraphService::new(NodeRegistry::builtin());
    let mut graph = WorkflowGraph {
        schema_version: 1,
        catalog_revision: None,
        nodes: BTreeMap::from([(
            "photo".to_owned(),
            GraphNode {
                node_type: "input.image".to_owned(),
                title: "Photo".to_owned(),
                params: json!({ "storage_uri": "upload://a" }),
                pos: [0.0, 0.0],
                size: None,
                semantics: None,
            },
        )]),
        edges: Vec::new(),
    };

    for (id, pos) in [
        ("image_grid_r1c1", [400.0, 0.0]),
        ("image_grid_r1c2", [680.0, 0.0]),
        ("image_grid_r2c1", [400.0, 220.0]),
        ("image_grid_r2c2", [680.0, 220.0]),
    ] {
        service
            .apply_op_in_place(
                &mut graph,
                &ProposalOp::SpawnNode {
                    id: id.to_owned(),
                    from: "photo".to_owned(),
                    node: GraphNode {
                        node_type: "input.image".to_owned(),
                        title: id.to_owned(),
                        params: json!({ "storage_uri": format!("upload://{id}") }),
                        pos,
                        size: None,
                        semantics: None,
                    },
                },
            )
            .expect("spawn tile");
    }

    service
        .validate_graph(&graph)
        .expect("grid split graph is valid");
    assert_eq!(graph.nodes.len(), 5);
    assert_eq!(graph.edges.len(), 4);
    assert!(graph.edges.iter().all(|edge| {
        edge.from == ["photo".to_owned(), "image".to_owned()]
            && edge.to[1] == "in"
            && edge.edge_type == "image"
    }));
}
