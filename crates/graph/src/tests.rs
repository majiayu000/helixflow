use super::*;
use helixflow_registry::NodeRegistry;
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;

fn service() -> GraphService {
    GraphService::new(NodeRegistry::builtin())
}

fn sample_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([
            (
                "input".to_string(),
                GraphNode {
                    node_type: "input.text".to_string(),
                    title: "Text".to_string(),
                    params: json!({ "text": "make a product clip" }),
                    pos: [0.0, 0.0],
                },
            ),
            (
                "writer".to_string(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_string(),
                    title: "Prompt".to_string(),
                    params: json!({ "style": "product" }),
                    pos: [220.0, 0.0],
                },
            ),
            (
                "video".to_string(),
                GraphNode {
                    node_type: "video.text_to_video".to_string(),
                    title: "Video".to_string(),
                    params: json!({
                        "prompt": "clean product shot",
                        "duration_sec": 5,
                        "aspect_ratio": "9:16"
                    }),
                    pos: [440.0, 0.0],
                },
            ),
            (
                "output".to_string(),
                GraphNode {
                    node_type: "output.save".to_string(),
                    title: "Save".to_string(),
                    params: json!({}),
                    pos: [660.0, 0.0],
                },
            ),
        ]),
        edges: vec![
            GraphEdge {
                from: ["input".to_string(), "text".to_string()],
                to: ["writer".to_string(), "text".to_string()],
                edge_type: "text".to_string(),
            },
            GraphEdge {
                from: ["writer".to_string(), "prompt".to_string()],
                to: ["video".to_string(), "prompt".to_string()],
                edge_type: "text".to_string(),
            },
            GraphEdge {
                from: ["video".to_string(), "video".to_string()],
                to: ["output".to_string(), "artifact".to_string()],
                edge_type: "artifact".to_string(),
            },
        ],
    }
}

#[test]
fn reports_module_name() {
    assert_eq!(module_name(), "graph");
}

#[test]
fn serializes_workflow_graph_boundary() {
    let graph = sample_graph();
    let encoded = serde_json::to_value(&graph).expect("serialize graph");

    assert_eq!(encoded["schema_version"], 1);
    assert_eq!(encoded["nodes"]["input"]["node_type"], "input.text");
    assert_eq!(encoded["edges"][0]["edge_type"], "text");
}

#[test]
fn validates_node_schema_and_edges() {
    service()
        .validate_graph(&sample_graph())
        .expect("valid graph");

    let mut invalid = sample_graph();
    invalid.nodes.get_mut("video").unwrap().params = json!({
        "prompt": "too long",
        "duration_sec": 30,
        "aspect_ratio": "9:16"
    });

    assert!(matches!(
        service().validate_graph(&invalid),
        Err(GraphError::Registry(_))
    ));
}

#[test]
fn validates_proposal_ops_and_preview_graph() {
    let graph = sample_graph();
    let draft = ProposalDraft {
        base_version_id: "ver_1".to_string(),
        kind: ProposalKind::Modify,
        title: "Shorter clip".to_string(),
        summary: "Set duration to three seconds.".to_string(),
        ops: vec![ProposalOp::SetParam {
            id: "video".to_string(),
            key: "duration_sec".to_string(),
            prev: Some(json!(5)),
            value: json!(3),
        }],
        message_id: Some("msg_1".to_string()),
    };

    let proposal = service()
        .preview_proposal(&graph, "ver_1", draft)
        .expect("preview proposal");
    let applied = service()
        .apply_proposal(&graph, "ver_1", &proposal)
        .expect("apply proposal");

    assert_eq!(proposal.state, ProposalState::Pending);
    assert_eq!(applied.nodes["video"].params["duration_sec"], 3);
    assert_ne!(applied, graph);
}

#[test]
fn apply_rechecks_superseded_base_version() {
    let graph = sample_graph();
    let draft = ProposalDraft {
        base_version_id: "ver_1".to_string(),
        kind: ProposalKind::Modify,
        title: "Shorter clip".to_string(),
        summary: "Set duration to three seconds.".to_string(),
        ops: vec![ProposalOp::SetParam {
            id: "video".to_string(),
            key: "duration_sec".to_string(),
            prev: Some(json!(5)),
            value: json!(3),
        }],
        message_id: None,
    };
    let proposal = service()
        .preview_proposal(&graph, "ver_1", draft)
        .expect("preview proposal");

    assert!(matches!(
        service().apply_proposal(&graph, "ver_2", &proposal),
        Err(GraphError::ProposalSuperseded { .. })
    ));
}

#[test]
fn rejects_duplicate_input_edges_before_plan_compilation() {
    let mut graph = sample_graph();
    graph.edges.push(GraphEdge {
        from: ["input".to_string(), "text".to_string()],
        to: ["writer".to_string(), "text".to_string()],
        edge_type: "text".to_string(),
    });

    assert!(matches!(
        service().validate_graph(&graph),
        Err(GraphError::DuplicateInputConnection { .. })
    ));
}

#[test]
fn rejects_invalid_edge_type_label() {
    let mut graph = sample_graph();
    graph.edges[0].edge_type = "bogus".to_string();

    assert!(matches!(
        service().validate_graph(&graph),
        Err(GraphError::PortTypeMismatch { .. })
    ));
}

#[test]
fn required_inputs_can_be_provided_by_params() {
    let mut graph = sample_graph();
    graph
        .edges
        .retain(|edge| edge.to != ["video".to_string(), "prompt".to_string()]);

    service()
        .validate_graph(&graph)
        .expect("video prompt param satisfies required input");
}

#[test]
fn dismissing_proposal_leaves_graph_unchanged() {
    let graph = sample_graph();

    assert_eq!(service().dismiss_proposal(&graph), graph);
}

#[test]
fn superseded_base_versions_return_conflict() {
    let graph = sample_graph();
    let draft = ProposalDraft {
        base_version_id: "ver_1".to_string(),
        kind: ProposalKind::Modify,
        title: "Stale".to_string(),
        summary: "This proposal is based on an old version.".to_string(),
        ops: vec![],
        message_id: None,
    };

    assert!(matches!(
        service().preview_proposal(&graph, "ver_2", draft),
        Err(GraphError::ProposalSuperseded { .. })
    ));
}

#[test]
fn compiles_execution_plan_in_topological_order() {
    let plan = service()
        .compile_plan(&sample_graph(), "ver_1", "atlas")
        .expect("compile plan");

    assert_eq!(plan.schema_version, 1);
    assert_eq!(plan.version_id, "ver_1");
    assert_eq!(plan.steps[0].node_id, "input");
    assert_eq!(plan.steps[1].provider.as_deref(), Some("atlas"));
    assert_eq!(plan.steps[2].provider.as_deref(), Some("atlas"));
    assert_eq!(plan.steps[2].capability.as_deref(), Some("text_to_video"));
}

#[tokio::test]
async fn applying_proposal_can_create_immutable_child_version() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("helixflow-test-store");
    let store = Store::open(&format!("sqlite://{}", db_path.display()))
        .await
        .expect("open store");
    let workspace = store
        .create_workspace("Proposal workspace")
        .await
        .expect("create workspace");
    let base_version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base graph",
            source: VersionSource::Manual,
            graph_path: "graphs/base.json",
            graph_hash: "sha256:base",
            parent_id: None,
        })
        .await
        .expect("create base version");
    let graph = sample_graph();
    let draft = ProposalDraft {
        base_version_id: base_version.id.clone(),
        kind: ProposalKind::Modify,
        title: "Shorter clip".to_string(),
        summary: "Set duration to four seconds.".to_string(),
        ops: vec![ProposalOp::SetParam {
            id: "video".to_string(),
            key: "duration_sec".to_string(),
            prev: Some(json!(5)),
            value: json!(4),
        }],
        message_id: None,
    };
    let proposal = service()
        .preview_proposal(&graph, &base_version.id, draft)
        .expect("preview proposal");

    let applied = service()
        .apply_proposal_version(
            &store,
            ApplyProposalVersion {
                workspace_id: &workspace.id,
                base_graph: &graph,
                current_version_id: &base_version.id,
                proposal: &proposal,
                version_label: "Applied proposal",
                graph_path: "graphs/applied.json",
                graph_hash: "sha256:applied",
            },
        )
        .await
        .expect("apply proposal version");

    let child_version = applied.version;
    assert_eq!(applied.graph.nodes["video"].params["duration_sec"], 4);
    assert_eq!(child_version.idx, 2);
    assert_eq!(child_version.source, "proposal");
    assert_eq!(
        child_version.parent_id.as_deref(),
        Some(base_version.id.as_str())
    );
    assert_ne!(child_version.id, base_version.id);
}

#[tokio::test]
async fn store_backed_apply_rejects_stale_workspace_version() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("helixflow-test-store");
    let store = Store::open(&format!("sqlite://{}", db_path.display()))
        .await
        .expect("open store");
    let workspace = store
        .create_workspace("Proposal workspace")
        .await
        .expect("create workspace");
    let base_version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base graph",
            source: VersionSource::Manual,
            graph_path: "graphs/base.json",
            graph_hash: "sha256:base",
            parent_id: None,
        })
        .await
        .expect("create base version");
    store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace.id,
                label: "Concurrent graph",
                source: VersionSource::Manual,
                graph_path: "graphs/concurrent.json",
                graph_hash: "sha256:concurrent",
                parent_id: Some(&base_version.id),
            },
            &base_version.id,
        )
        .await
        .expect("create concurrent version");
    let graph = sample_graph();
    let draft = ProposalDraft {
        base_version_id: base_version.id.clone(),
        kind: ProposalKind::Modify,
        title: "Shorter clip".to_string(),
        summary: "Set duration to four seconds.".to_string(),
        ops: vec![ProposalOp::SetParam {
            id: "video".to_string(),
            key: "duration_sec".to_string(),
            prev: Some(json!(5)),
            value: json!(4),
        }],
        message_id: None,
    };
    let proposal = service()
        .preview_proposal(&graph, &base_version.id, draft)
        .expect("preview proposal");
    let err = service()
        .apply_proposal_version(
            &store,
            ApplyProposalVersion {
                workspace_id: &workspace.id,
                base_graph: &graph,
                current_version_id: &base_version.id,
                proposal: &proposal,
                version_label: "Stale proposal",
                graph_path: "graphs/stale.json",
                graph_hash: "sha256:stale",
            },
        )
        .await
        .expect_err("stale store version should fail");

    assert!(
        matches!(err, GraphError::Store(message) if message.contains("expected current version"))
    );
}
