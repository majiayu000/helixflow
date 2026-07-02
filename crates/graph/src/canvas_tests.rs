use std::collections::BTreeMap;

use helixflow_registry::NodeRegistry;
use serde_json::{Value, json};

use crate::{
    CanvasActor, CanvasActorKind, CanvasComment, CanvasDocument, CanvasEdge, CanvasEdgeKind,
    CanvasEndpoint, CanvasError, CanvasNode, CanvasNodeKind, CanvasNodeRuntime, CanvasNodeUi,
    CanvasOpEnvelope, CanvasOpKind, CanvasPoint, CanvasPorts, CanvasSize, CanvasViewport,
    GraphService,
};

const NOW: &str = "2026-07-01T00:00:00Z";

fn document(
    nodes: BTreeMap<String, CanvasNode>,
    edges: BTreeMap<String, CanvasEdge>,
) -> CanvasDocument {
    CanvasDocument {
        schema_version: 1,
        canvas_id: "canvas_1".to_owned(),
        workspace_id: "ws_1".to_owned(),
        document_version_id: Some("canvas_ver_1".to_owned()),
        title: "Canvas".to_owned(),
        seq: 7,
        base_graph_version_id: Some("ver_1".to_owned()),
        viewport: CanvasViewport::default(),
        nodes,
        edges,
        comments: BTreeMap::new(),
        metadata: json!({}),
        created_at: NOW.to_owned(),
        updated_at: NOW.to_owned(),
    }
}

fn workflow_node(id: &str, node_type: &str, title: &str, x: f32, params: Value) -> CanvasNode {
    CanvasNode {
        id: id.to_owned(),
        kind: CanvasNodeKind::Workflow,
        node_type: Some(node_type.to_owned()),
        title: title.to_owned(),
        position: CanvasPoint { x, y: 0.0 },
        size: Some(CanvasSize {
            width: 240.0,
            height: 160.0,
        }),
        ports: CanvasPorts::default(),
        params,
        content: json!({}),
        media: Vec::new(),
        runtime: CanvasNodeRuntime::default(),
        ui: CanvasNodeUi::default(),
        created_at: NOW.to_owned(),
        updated_at: NOW.to_owned(),
    }
}

fn text_node(id: &str) -> CanvasNode {
    CanvasNode {
        id: id.to_owned(),
        kind: CanvasNodeKind::Text,
        node_type: None,
        title: "Prompt note".to_owned(),
        position: CanvasPoint { x: 20.0, y: 160.0 },
        size: None,
        ports: CanvasPorts::default(),
        params: json!({}),
        content: json!({ "text": "draft prompt" }),
        media: Vec::new(),
        runtime: CanvasNodeRuntime::default(),
        ui: CanvasNodeUi::default(),
        created_at: NOW.to_owned(),
        updated_at: NOW.to_owned(),
    }
}

fn edge(
    id: &str,
    from: (&str, &str),
    to: (&str, &str),
    kind: CanvasEdgeKind,
    edge_type: Option<&str>,
) -> CanvasEdge {
    CanvasEdge {
        id: id.to_owned(),
        from: CanvasEndpoint {
            node_id: from.0.to_owned(),
            port: from.1.to_owned(),
        },
        to: CanvasEndpoint {
            node_id: to.0.to_owned(),
            port: to.1.to_owned(),
        },
        kind,
        edge_type: edge_type.map(str::to_owned),
        label: None,
        metadata: json!({}),
        created_at: NOW.to_owned(),
        updated_at: NOW.to_owned(),
    }
}

#[test]
fn projects_only_executable_canvas_nodes() {
    let doc = document(
        BTreeMap::from([
            (
                "input".to_owned(),
                workflow_node(
                    "input",
                    "input.text",
                    "Text",
                    0.0,
                    json!({ "text": "make a product clip" }),
                ),
            ),
            (
                "video".to_owned(),
                workflow_node(
                    "video",
                    "video.atlas.text_to_video",
                    "Video",
                    220.0,
                    json!({
                        "prompt": "clean product shot",
                        "duration_sec": 5,
                        "resolution": "720P"
                    }),
                ),
            ),
            ("note".to_owned(), text_node("note")),
        ]),
        BTreeMap::from([
            (
                "input_to_video".to_owned(),
                edge(
                    "input_to_video",
                    ("input", "text"),
                    ("video", "prompt"),
                    CanvasEdgeKind::Data,
                    Some("text"),
                ),
            ),
            (
                "visual_note".to_owned(),
                edge(
                    "visual_note",
                    ("note", "text"),
                    ("video", "prompt"),
                    CanvasEdgeKind::Visual,
                    None,
                ),
            ),
        ]),
    );

    let graph = doc.project_workflow_graph().expect("project canvas");

    assert_eq!(graph.schema_version, 1);
    assert_eq!(graph.nodes.len(), 2);
    assert!(graph.nodes.contains_key("input"));
    assert!(graph.nodes.contains_key("video"));
    assert!(!graph.nodes.contains_key("note"));
    assert_eq!(graph.nodes["video"].pos, [220.0, 0.0]);
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.edges[0].edge_type, "text");
}

#[test]
fn projected_canvas_can_reuse_existing_graph_validation() {
    let doc = document(
        BTreeMap::from([
            (
                "video".to_owned(),
                workflow_node(
                    "video",
                    "video.atlas.text_to_video",
                    "Video",
                    220.0,
                    json!({
                        "prompt": "clean product shot",
                        "duration_sec": 5,
                        "resolution": "720P"
                    }),
                ),
            ),
            (
                "output".to_owned(),
                workflow_node("output", "output.save", "Save", 440.0, json!({})),
            ),
        ]),
        BTreeMap::from([(
            "video_to_output".to_owned(),
            edge(
                "video_to_output",
                ("video", "video"),
                ("output", "artifact"),
                CanvasEdgeKind::Artifact,
                None,
            ),
        )]),
    );

    let graph = doc.project_workflow_graph().expect("project canvas");
    GraphService::new(NodeRegistry::builtin())
        .validate_graph(&graph)
        .expect("projected graph remains executable");
}

#[test]
fn rejects_workflow_nodes_without_node_type() {
    let mut node = workflow_node(
        "video",
        "video.atlas.text_to_video",
        "Video",
        0.0,
        json!({}),
    );
    node.node_type = None;
    let doc = document(
        BTreeMap::from([("video".to_owned(), node)]),
        BTreeMap::new(),
    );

    assert!(matches!(
        doc.project_workflow_graph(),
        Err(CanvasError::MissingExecutableNodeType(node_id)) if node_id == "video"
    ));
}

#[test]
fn rejects_executable_edges_to_non_projected_nodes() {
    let doc = document(
        BTreeMap::from([
            (
                "video".to_owned(),
                workflow_node(
                    "video",
                    "video.atlas.text_to_video",
                    "Video",
                    0.0,
                    json!({
                        "prompt": "clean product shot",
                        "duration_sec": 5,
                        "resolution": "720P"
                    }),
                ),
            ),
            ("note".to_owned(), text_node("note")),
        ]),
        BTreeMap::from([(
            "note_to_video".to_owned(),
            edge(
                "note_to_video",
                ("note", "text"),
                ("video", "prompt"),
                CanvasEdgeKind::Data,
                Some("text"),
            ),
        )]),
    );

    assert!(matches!(
        doc.project_workflow_graph(),
        Err(CanvasError::MissingEndpoint { edge_id, node_id })
            if edge_id == "note_to_video" && node_id == "note"
    ));
}

#[test]
fn rejects_data_edges_without_explicit_edge_type() {
    let doc = document(
        BTreeMap::from([
            (
                "input".to_owned(),
                workflow_node(
                    "input",
                    "input.text",
                    "Text",
                    0.0,
                    json!({ "text": "hello" }),
                ),
            ),
            (
                "video".to_owned(),
                workflow_node(
                    "video",
                    "video.atlas.text_to_video",
                    "Video",
                    220.0,
                    json!({
                        "prompt": "clean product shot",
                        "duration_sec": 5,
                        "resolution": "720P"
                    }),
                ),
            ),
        ]),
        BTreeMap::from([(
            "input_to_video".to_owned(),
            edge(
                "input_to_video",
                ("input", "text"),
                ("video", "prompt"),
                CanvasEdgeKind::Data,
                None,
            ),
        )]),
    );

    assert!(matches!(
        doc.project_workflow_graph(),
        Err(CanvasError::MissingDataEdgeType)
    ));
}

#[test]
fn serializes_canvas_op_boundary_as_snake_case() {
    let op = CanvasOpEnvelope {
        op_id: "op_1".to_owned(),
        canvas_id: "canvas_1".to_owned(),
        seq: 8,
        base_seq: 7,
        actor: CanvasActor {
            id: "agent_1".to_owned(),
            kind: CanvasActorKind::Agent,
        },
        kind: CanvasOpKind::ArtifactAttach,
        payload: json!({
            "node_id": "video",
            "artifact_id": "artifact_1"
        }),
        idempotency_key: "client_op_1".to_owned(),
        created_at: NOW.to_owned(),
    };

    let encoded = serde_json::to_value(op).expect("serialize op");

    assert_eq!(encoded["actor"]["kind"], "agent");
    assert_eq!(encoded["kind"], "artifact_attach");
    assert_eq!(encoded["base_seq"], 7);
}

pub(crate) fn sample_document() -> CanvasDocument {
    document(
        BTreeMap::from([(
            "video".to_owned(),
            workflow_node(
                "video",
                "video.atlas.text_to_video",
                "Video",
                220.0,
                json!({
                    "prompt": "clean product shot",
                    "duration_sec": 5,
                    "resolution": "720P"
                }),
            ),
        )]),
        BTreeMap::new(),
    )
}

pub(crate) fn sample_text_node(id: &str) -> CanvasNode {
    text_node(id)
}

pub(crate) fn sample_comment(id: &str) -> CanvasComment {
    CanvasComment {
        id: id.to_owned(),
        anchor: crate::CanvasCommentAnchor {
            node_id: Some("video".to_owned()),
            edge_id: None,
            position: None,
        },
        body: "Looks good".to_owned(),
        resolved: false,
        author_id: "user_1".to_owned(),
        created_at: NOW.to_owned(),
        updated_at: NOW.to_owned(),
    }
}
