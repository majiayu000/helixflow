use std::collections::BTreeMap;

use helixflow_graph::semantics::NodeSemanticsEntry;
use helixflow_registry::catalog::ImplementationSelection;
use serde_json::json;

use super::*;

#[test]
fn atlas_policy_resolves_configured_defaults() {
    let catalog = shared_catalog();

    let image = resolve_step_binding(catalog, "text_to_image", "atlas", None).expect("image");
    assert_eq!(image.capability_id, "text_to_image");
    assert_eq!(image.resolved_model_id, "google/nano-banana-2");
    assert_eq!(image.operation_id, "google/nano-banana-2/text-to-image");
    assert_eq!(image.connector_id, "atlas");
    assert_eq!(image.requested_model_id, None);

    let video = resolve_step_binding(catalog, "text_to_video", "atlas", None).expect("video");
    assert_eq!(
        video.operation_id,
        "bytedance/seedance-v1.5-pro/text-to-video-fast"
    );

    let chat = resolve_step_binding(catalog, "prompt_writer", "atlas", None).expect("chat");
    assert_eq!(chat.resolved_model_id, "deepseek-ai/DeepSeek-V3-0324");
}

#[test]
fn fal_preference_selects_the_fal_binding() {
    let catalog = shared_catalog();

    // The configured default lives on atlas; with the fal connector selected
    // the unique fal binding is the explicit choice.
    let image = resolve_step_binding(catalog, "text_to_image", "fal", None).expect("image");
    assert_eq!(image.connector_id, "fal");
    assert_eq!(image.operation_id, "fal-ai/nano-banana-2");
    assert_eq!(image.resolved_model_id, "google/nano-banana-2");
}

#[test]
fn capability_without_binding_on_connector_fails_closed() {
    let catalog = shared_catalog();

    // fal implements no text_to_video binding.
    let err = resolve_step_binding(catalog, "text_to_video", "fal", None)
        .expect_err("no fal video binding");
    assert_eq!(err.0, "BINDING_NOT_FOUND");
}

#[test]
fn pinned_semantics_mismatch_fails_run_creation() {
    let catalog = shared_catalog();
    let entry = NodeSemanticsEntry {
        capability_id: "text_to_image".to_owned(),
        mode: "text_to_image".to_owned(),
        implementation: ImplementationSelection::Pinned {
            requested_model_id: "google/nano-banana-2".to_owned(),
            // Pinned to the fal binding, but the run executes on atlas: the
            // resolved binding differs from the pinned one.
            binding_id: "google.nano-banana-2.text-to-image.fal.v1".to_owned(),
        },
    };

    let err = resolve_step_binding(catalog, "text_to_image", "atlas", Some(&entry))
        .expect_err("pinned mismatch");
    assert_eq!(err.0, "PINNED_MODEL_MISMATCH");
}

#[test]
fn pinned_semantics_matching_binding_resolves() {
    let catalog = shared_catalog();
    let entry = NodeSemanticsEntry {
        capability_id: "text_to_image".to_owned(),
        mode: "text_to_image".to_owned(),
        implementation: ImplementationSelection::Pinned {
            requested_model_id: "google/nano-banana-2".to_owned(),
            binding_id: "google.nano-banana-2.text-to-image.atlas.v1".to_owned(),
        },
    };

    let resolved = resolve_step_binding(catalog, "text_to_image", "atlas", Some(&entry))
        .expect("pinned resolves");
    assert_eq!(
        resolved.requested_model_id.as_deref(),
        Some("google/nano-banana-2")
    );
    assert_eq!(resolved.resolved_model_id, "google/nano-banana-2");
}

#[test]
fn mock_provider_skips_resolution_and_catalog_provider_freezes_revision() {
    let catalog = shared_catalog();
    let graph = helixflow_graph::WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([(
            "image".to_owned(),
            helixflow_graph::GraphNode {
                node_type: "image.generate".to_owned(),
                title: "Image".to_owned(),
                params: json!({ "prompt": "p", "aspect_ratio": "1:1" }),
                pos: [0.0, 0.0],
                size: None,
                semantics: None,
            },
        )]),
        edges: Vec::new(),
        catalog_revision: None,
    };
    let service = helixflow_graph::GraphService::new(helixflow_registry::NodeRegistry::builtin());

    let mut mock_plan = service.compile_plan(&graph, "ver_1", "mock").expect("plan");
    attach_resolved_bindings(&mut mock_plan, "mock", None).expect("mock skips");
    assert!(mock_plan.steps[0].resolved.is_none());
    assert_eq!(mock_plan.catalog_revision, None);

    let mut atlas_plan = service
        .compile_plan(&graph, "ver_1", "atlas")
        .expect("plan");
    attach_resolved_bindings(&mut atlas_plan, "atlas", None).expect("atlas resolves");
    let resolved = atlas_plan.steps[0].resolved.as_ref().expect("resolved");
    assert_eq!(resolved.resolved_model_id, "google/nano-banana-2");
    assert_eq!(
        atlas_plan.catalog_revision.as_deref(),
        Some(catalog.catalog_revision.as_str())
    );
}
