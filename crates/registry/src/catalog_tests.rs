use super::*;
use crate::catalog_seed::builtin_catalog;

#[test]
fn revision_is_deterministic_and_content_addressed() {
    let a = builtin_catalog();
    let b = builtin_catalog();
    assert_eq!(a.catalog_revision, b.catalog_revision);
    assert!(a.catalog_revision.starts_with("sha256:"));
    assert_eq!(a.catalog_revision.len(), "sha256:".len() + 64);
    assert!(
        a.catalog_revision["sha256:".len()..]
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    );

    let mut models = a.models.clone();
    models[0].display_name = "Renamed".to_owned();
    let changed = CatalogSnapshot::build(
        a.capabilities.clone(),
        models,
        a.bindings.clone(),
        a.connectors.clone(),
        a.workflow_backends.clone(),
        a.default_bindings.clone(),
    );
    assert_ne!(a.catalog_revision, changed.catalog_revision);
}

#[test]
fn build_sorts_entries_regardless_of_input_order() {
    let seed = builtin_catalog();
    let mut reversed_models = seed.models.clone();
    reversed_models.reverse();
    let mut reversed_bindings = seed.bindings.clone();
    reversed_bindings.reverse();

    let rebuilt = CatalogSnapshot::build(
        seed.capabilities.clone(),
        reversed_models,
        reversed_bindings,
        seed.connectors.clone(),
        seed.workflow_backends.clone(),
        seed.default_bindings.clone(),
    );

    assert_eq!(seed, rebuilt);
}

#[test]
fn capability_and_model_are_many_to_many() {
    let catalog = builtin_catalog();

    // One model implements one capability through two connectors...
    let nano_bindings = catalog.bindings_for_pair("text_to_image", "google/nano-banana-2");
    assert_eq!(nano_bindings.len(), 2);
    let connectors: Vec<&str> = nano_bindings
        .iter()
        .map(|binding| match &binding.implementation {
            ImplementationTarget::ApiConnector { connector_id, .. } => connector_id.as_str(),
            ImplementationTarget::WorkflowTemplate { backend_id, .. } => backend_id.as_str(),
        })
        .collect();
    assert!(connectors.contains(&"atlas"));
    assert!(connectors.contains(&"fal"));

    // ...and projections are consistent in both directions.
    let models = catalog.models_for_capability("text_to_image");
    assert!(
        models
            .iter()
            .any(|model| model.model_id == "google/nano-banana-2")
    );
    let capabilities = catalog.capabilities_for_model("google/nano-banana-2");
    assert!(
        capabilities
            .iter()
            .any(|capability| capability.capability_id == "text_to_image")
    );
}

#[test]
fn seed_catalog_covers_all_v2_capabilities() {
    let catalog = builtin_catalog();
    for capability_id in [
        "prompt_writer",
        "text_to_image",
        "image_edit",
        "text_to_video",
        "image_to_video",
        "video_extend",
        "upscale_image",
        "upscale_video",
        "image_analyze",
    ] {
        assert!(
            catalog.capability(capability_id).is_some(),
            "missing capability {capability_id}"
        );
    }
    assert!(catalog.workflow_backends.is_empty());
}

#[test]
fn default_bindings_reference_existing_enabled_bindings() {
    let catalog = builtin_catalog();
    assert!(!catalog.default_bindings.is_empty());
    for (capability_id, binding_id) in &catalog.default_bindings {
        let binding = catalog
            .binding(binding_id)
            .unwrap_or_else(|| panic!("default binding {binding_id} missing"));
        assert_eq!(&binding.capability_id, capability_id);
        assert_eq!(binding.availability, BindingAvailability::Enabled);
    }
}

#[test]
fn api_boundary_serializes_camel_case() {
    let catalog = builtin_catalog();
    let encoded = serde_json::to_value(&catalog).expect("serialize catalog");

    assert!(encoded.get("catalogRevision").is_some());
    assert!(encoded.get("workflowBackends").is_some());
    assert!(encoded.get("defaultBindings").is_some());
    let binding = &encoded["bindings"][0];
    assert!(binding.get("bindingId").is_some());
    assert!(binding.get("capabilityId").is_some());
    assert!(binding.get("bindingRevision").is_some());
    let target = binding
        .get("implementation")
        .and_then(|value| value.get("apiConnector"))
        .expect("api connector target");
    assert!(target.get("connectorId").is_some());
    assert!(target.get("operationId").is_some());

    let roundtrip: CatalogSnapshot = serde_json::from_value(encoded).expect("deserialize catalog");
    assert_eq!(roundtrip, catalog);
}

#[test]
fn wired_ports_satisfy_required_schema_params() {
    let catalog = builtin_catalog();
    let binding = catalog
        .binding("google.nano-banana-2.text-to-image.atlas.v1")
        .expect("binding");
    let wired = std::collections::BTreeSet::from(["prompt".to_owned()]);

    binding
        .input_schema
        .validate_value_with_wired(
            "binding",
            &serde_json::json!({ "aspect_ratio": "1:1" }),
            &wired,
        )
        .expect("wired prompt satisfies required");

    binding
        .input_schema
        .validate_value("binding", &serde_json::json!({ "aspect_ratio": "1:1" }))
        .expect_err("without wiring the required param is enforced");
}
