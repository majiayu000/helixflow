use std::collections::BTreeMap;

use helixflow_registry::NodeRegistry;
use helixflow_registry::catalog::{CatalogSnapshot, ImplementationSelection};
use helixflow_registry::catalog_seed::builtin_catalog;
use serde_json::json;

use super::*;
use crate::{GraphEdge, GraphNode, GraphService};

fn service() -> GraphService {
    GraphService::new(NodeRegistry::builtin())
}

fn image_node(params: serde_json::Value) -> GraphNode {
    GraphNode {
        node_type: "image.generate".to_owned(),
        title: "Image".to_owned(),
        params,
        pos: [0.0, 0.0],
        size: None,
        semantics: None,
    }
}

fn video_node() -> GraphNode {
    GraphNode {
        node_type: "video.text_to_video".to_owned(),
        title: "Video".to_owned(),
        params: json!({ "prompt": "clip", "duration_sec": 4, "aspect_ratio": "9:16" }),
        pos: [200.0, 0.0],
        size: None,
        semantics: None,
    }
}

fn v1_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        catalog_revision: None,
        nodes: BTreeMap::from([
            (
                "image".to_owned(),
                image_node(json!({ "prompt": "product", "aspect_ratio": "1:1" })),
            ),
            ("video".to_owned(), video_node()),
        ]),
        edges: Vec::new(),
    }
}

fn set_semantics(graph: &mut WorkflowGraph, node_id: &str, entry: NodeSemanticsEntry) {
    graph.nodes.get_mut(node_id).expect("node exists").semantics = Some(entry);
}

#[test]
fn migrates_nodes_without_model_to_explicit_policy() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let (migrated, report) = migrate_v1(&v1_graph(), &registry, &catalog);

    assert!(report.resolvable);
    let migrated = migrated.expect("migrated graph");
    assert_eq!(migrated.schema_version, 1);
    assert_eq!(
        migrated.catalog_revision.as_deref(),
        Some(catalog.catalog_revision.as_str())
    );
    // Topology, ids, titles, and positions are untouched (P14).
    let mut structural = migrated.clone();
    structural.catalog_revision = None;
    for node in structural.nodes.values_mut() {
        node.semantics = None;
    }
    assert_eq!(structural, v1_graph());
    let semantics = migrated.collected_semantics();
    assert!(matches!(
        semantics
            .get("image")
            .expect("image semantics")
            .implementation,
        ImplementationSelection::Policy { .. }
    ));
    assert_eq!(
        semantics
            .get("video")
            .expect("video semantics")
            .capability_id,
        "text_to_video"
    );
    assert!(
        report
            .nodes
            .iter()
            .all(|node| matches!(node.action, MigrationAction::MappedPolicy { .. }))
    );

    migrated
        .validate_semantics(&service(), &catalog)
        .expect("migrated graph validates");
}

#[test]
fn migration_is_deterministic() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let first = migrate_v1(&v1_graph(), &registry, &catalog);
    let second = migrate_v1(&v1_graph(), &registry, &catalog);
    assert_eq!(first, second);
}

#[test]
fn legacy_model_param_is_resolved_to_pinned_and_stripped() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let mut graph = v1_graph();
    graph
        .nodes
        .get_mut("image")
        .expect("image node")
        .params
        .as_object_mut()
        .expect("params object")
        .insert("model".to_owned(), json!("nano banana"));

    let (migrated, report) = migrate_v1(&graph, &registry, &catalog);

    let migrated = migrated.expect("migrated graph");
    assert!(
        migrated.nodes["image"].params.get("model").is_none(),
        "legacy params.model must move into the semantic layer"
    );
    match &migrated.nodes["image"]
        .semantics
        .as_ref()
        .expect("semantics")
        .implementation
    {
        ImplementationSelection::Pinned {
            requested_model_id,
            binding_id,
        } => {
            assert_eq!(requested_model_id, "google/nano-banana-2");
            assert_eq!(binding_id, "google.nano-banana-2.text-to-image.atlas.v1");
        }
        other => panic!("expected pinned selection, got {other:?}"),
    }
    assert!(report.nodes.iter().any(|node| matches!(
        &node.action,
        MigrationAction::MappedPinned { model_id, .. } if model_id == "google/nano-banana-2"
    )));
    migrated
        .validate_semantics(&service(), &catalog)
        .expect("pinned migration validates");
}

#[test]
fn model_capability_mismatch_needs_resolution_not_silent_swap() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let mut graph = v1_graph();
    graph
        .nodes
        .get_mut("image")
        .expect("image node")
        .params
        .as_object_mut()
        .expect("params object")
        .insert("model".to_owned(), json!("Seedance 2"));

    let (migrated, report) = migrate_v1(&graph, &registry, &catalog);

    assert!(migrated.is_none());
    assert!(!report.resolvable);
    assert!(report.nodes.iter().any(|node| matches!(
        &node.action,
        MigrationAction::NeedsResolution { reason } if reason.contains("no binding")
    )));
}

#[test]
fn missing_default_binding_needs_resolution() {
    let seed = builtin_catalog();
    let mut defaults = seed.default_bindings.clone();
    defaults.remove("text_to_video");
    let catalog = CatalogSnapshot::build(
        seed.capabilities.clone(),
        seed.models.clone(),
        seed.bindings.clone(),
        seed.connectors.clone(),
        seed.workflow_backends.clone(),
        defaults,
    );
    let registry = NodeRegistry::builtin();

    let (migrated, report) = migrate_v1(&v1_graph(), &registry, &catalog);

    assert!(migrated.is_none());
    assert!(!report.resolvable);
    assert!(report.nodes.iter().any(|node| matches!(
        &node.action,
        MigrationAction::NeedsResolution { reason }
            if reason.contains("no configured default binding")
    )));
}

#[test]
fn ambiguous_legacy_model_needs_user_choice() {
    let seed = builtin_catalog();
    let mut models = seed.models.clone();
    let mut clone = models
        .iter()
        .find(|model| model.model_id == "google/nano-banana-2")
        .expect("nano banana")
        .clone();
    clone.model_id = "other/nano-banana-pro".to_owned();
    clone.display_name = "Nano Banana Pro".to_owned();
    clone.aliases = vec!["nano banana".to_owned()];
    models.push(clone);
    let catalog = CatalogSnapshot::build(
        seed.capabilities.clone(),
        models,
        seed.bindings.clone(),
        seed.connectors.clone(),
        seed.workflow_backends.clone(),
        seed.default_bindings.clone(),
    );
    let registry = NodeRegistry::builtin();
    let mut graph = v1_graph();
    graph
        .nodes
        .get_mut("image")
        .expect("image node")
        .params
        .as_object_mut()
        .expect("params object")
        .insert("model".to_owned(), json!("nano banana"));

    let (migrated, report) = migrate_v1(&graph, &registry, &catalog);

    assert!(migrated.is_none());
    assert!(report.nodes.iter().any(|node| matches!(
        &node.action,
        MigrationAction::NeedsUserChoice { candidates } if candidates.len() == 2
    )));
}

#[test]
fn title_and_position_never_affect_semantics() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let (migrated, _) = migrate_v1(&v1_graph(), &registry, &catalog);
    let mut migrated = migrated.expect("migrated graph");

    let semantics_before = migrated.collected_semantics();
    let image = migrated.nodes.get_mut("image").expect("image node");
    image.title = "Totally A Seedance Video Node".to_owned();
    image.pos = [999.0, -42.0];

    migrated
        .validate_semantics(&service(), &catalog)
        .expect("still valid after cosmetic changes");
    assert_eq!(migrated.collected_semantics(), semantics_before);
}

#[test]
fn pinned_model_mismatch_is_rejected() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let (migrated, _) = migrate_v1(&v1_graph(), &registry, &catalog);
    let mut migrated = migrated.expect("migrated graph");

    set_semantics(
        &mut migrated,
        "image",
        NodeSemanticsEntry {
            capability_id: "text_to_image".to_owned(),
            mode: "text_to_image".to_owned(),
            implementation: ImplementationSelection::Pinned {
                requested_model_id: "bytedance/seedance-v1.5-pro".to_owned(),
                binding_id: "google.nano-banana-2.text-to-image.atlas.v1".to_owned(),
            },
        },
    );

    let err = migrated
        .validate_semantics(&service(), &catalog)
        .expect_err("pinned mismatch");
    assert_eq!(err.code(), "PINNED_MODEL_MISMATCH");
}

#[test]
fn stale_catalog_revision_is_rejected() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let (migrated, _) = migrate_v1(&v1_graph(), &registry, &catalog);
    let mut migrated = migrated.expect("migrated graph");
    migrated.catalog_revision = Some("sha256:stale".to_owned());

    let err = migrated
        .validate_semantics(&service(), &catalog)
        .expect_err("stale revision");
    assert_eq!(err.code(), "CATALOG_REVISION_STALE");
}

#[test]
fn missing_catalog_revision_is_rejected() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let (migrated, _) = migrate_v1(&v1_graph(), &registry, &catalog);
    let mut migrated = migrated.expect("migrated graph");
    migrated.catalog_revision = None;

    let err = migrated
        .validate_semantics(&service(), &catalog)
        .expect_err("missing revision");
    assert_eq!(err.code(), "CATALOG_REVISION_MISSING");
}

#[test]
fn executable_node_without_semantics_is_rejected() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let (migrated, _) = migrate_v1(&v1_graph(), &registry, &catalog);
    let mut migrated = migrated.expect("migrated graph");
    migrated.nodes.get_mut("image").expect("image").semantics = None;

    let err = migrated
        .validate_semantics(&service(), &catalog)
        .expect_err("missing semantics");
    assert_eq!(err.code(), "MISSING_SEMANTICS");
}

#[test]
fn semantics_on_non_executable_node_is_rejected() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let mut graph = v1_graph();
    graph.nodes.insert(
        "input".to_owned(),
        GraphNode {
            node_type: "input.text".to_owned(),
            title: "Input".to_owned(),
            params: json!({ "text": "hello" }),
            pos: [-200.0, 0.0],
            size: None,
            semantics: None,
        },
    );
    let (migrated, _) = migrate_v1(&graph, &registry, &catalog);
    let mut migrated = migrated.expect("migrated graph");
    let entry = migrated.nodes["image"]
        .semantics
        .clone()
        .expect("image semantics");
    migrated.nodes.get_mut("input").expect("input").semantics = Some(entry);

    let err = migrated
        .validate_semantics(&service(), &catalog)
        .expect_err("non-executable semantics");
    assert_eq!(err.code(), "UNEXPECTED_SEMANTICS");
}

#[test]
fn wired_required_input_satisfies_binding_schema() {
    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let graph = WorkflowGraph {
        schema_version: 1,
        catalog_revision: None,
        nodes: BTreeMap::from([
            (
                "input".to_owned(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Input".to_owned(),
                    params: json!({ "text": "an ad concept" }),
                    pos: [-200.0, 0.0],
                    size: None,
                    semantics: None,
                },
            ),
            (
                "writer".to_owned(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_owned(),
                    title: "Writer".to_owned(),
                    params: json!({ "style": "product" }),
                    pos: [0.0, 0.0],
                    size: None,
                    semantics: None,
                },
            ),
            (
                "image".to_owned(),
                // v1 structural rules still require `prompt` as a param even
                // when wired (providers prefer the wired text at run time);
                // the binding schema must accept the same shape.
                image_node(json!({ "prompt": "placeholder", "aspect_ratio": "1:1" })),
            ),
        ]),
        edges: vec![
            GraphEdge {
                from: ["input".to_owned(), "text".to_owned()],
                to: ["writer".to_owned(), "text".to_owned()],
                edge_type: "text".to_owned(),
            },
            GraphEdge {
                from: ["writer".to_owned(), "prompt".to_owned()],
                to: ["image".to_owned(), "prompt".to_owned()],
                edge_type: "text".to_owned(),
            },
        ],
    };

    let (migrated, report) = migrate_v1(&graph, &registry, &catalog);
    assert!(report.resolvable, "report: {report:?}");
    let mut migrated = migrated.expect("migrated graph");

    // Pin the image node explicitly: the wired prompt must satisfy the
    // binding schema's required `prompt`.
    set_semantics(
        &mut migrated,
        "image",
        NodeSemanticsEntry {
            capability_id: "text_to_image".to_owned(),
            mode: "text_to_image".to_owned(),
            implementation: ImplementationSelection::Pinned {
                requested_model_id: "google/nano-banana-2".to_owned(),
                binding_id: "google.nano-banana-2.text-to-image.atlas.v1".to_owned(),
            },
        },
    );

    migrated
        .validate_semantics(&service(), &catalog)
        .expect("wired prompt satisfies binding schema");
}

#[test]
fn legacy_graph_json_migrates_and_roundtrips_idempotently() {
    // Fixture mirrors a real durable v1 graph file: no catalog_revision, no
    // per-node semantics (GH145 product invariant 1 + serde acceptance).
    let legacy_json = json!({
        "schema_version": 1,
        "nodes": {
            "image": {
                "node_type": "image.generate",
                "title": "Image",
                "params": { "prompt": "product", "aspect_ratio": "1:1" },
                "pos": [0.0, 0.0]
            },
            "video": {
                "node_type": "video.text_to_video",
                "title": "Video",
                "params": { "prompt": "clip", "duration_sec": 4, "aspect_ratio": "9:16" },
                "pos": [200.0, 0.0]
            }
        },
        "edges": []
    });
    let legacy: WorkflowGraph = serde_json::from_value(legacy_json).expect("legacy graph reads");
    assert_eq!(legacy, v1_graph());

    let catalog = builtin_catalog();
    let registry = NodeRegistry::builtin();
    let (migrated, report) = migrate_v1(&legacy, &registry, &catalog);
    assert!(report.resolvable);
    let migrated = migrated.expect("migrated graph");

    let encoded = serde_json::to_value(&migrated).expect("serialize");
    assert!(encoded.get("catalog_revision").is_some());
    let entry = &encoded["nodes"]["image"]["semantics"];
    assert!(entry.get("capabilityId").is_some());
    assert!(entry.get("implementation").is_some());
    // Non-semantic fields keep the exact legacy encoding.
    assert_eq!(encoded["nodes"]["image"]["node_type"], "image.generate");

    let decoded: WorkflowGraph = serde_json::from_value(encoded).expect("roundtrip");
    assert_eq!(decoded, migrated);
    let re_encoded = serde_json::to_value(&decoded).expect("re-serialize");
    let re_decoded: WorkflowGraph = serde_json::from_value(re_encoded).expect("idempotent");
    assert_eq!(re_decoded, migrated);
}
