use super::*;
use crate::catalog::{CatalogSnapshot, ModelDefinition, ModelLifecycle};
use crate::catalog_seed::builtin_catalog;

fn avail(entries: &[(&str, bool)]) -> ConnectorAvailability {
    entries
        .iter()
        .map(|(id, up)| ((*id).to_owned(), *up))
        .collect()
}

fn pinned(capability_id: &str, model: &str) -> ResolveRequest {
    ResolveRequest {
        capability_id: capability_id.to_owned(),
        requested_model: Some(model.to_owned()),
        connector_preference: None,
    }
}

fn policy(capability_id: &str) -> ResolveRequest {
    ResolveRequest {
        capability_id: capability_id.to_owned(),
        requested_model: None,
        connector_preference: None,
    }
}

#[test]
fn pinned_alias_resolves_to_canonical_model_via_default_binding() {
    let catalog = builtin_catalog();
    let resolver = CapabilityResolver::new(&catalog);

    let resolved = resolver
        .resolve(
            &pinned("text_to_image", "Nano Banana"),
            &avail(&[("atlas", true), ("fal", true)]),
        )
        .expect("resolved");

    assert_eq!(
        resolved.requested_model_id.as_deref(),
        Some("google/nano-banana-2")
    );
    assert_eq!(resolved.resolved_model_id, "google/nano-banana-2");
    assert_eq!(
        resolved.binding_id,
        "google.nano-banana-2.text-to-image.atlas.v1"
    );

    let seedance = resolver
        .resolve(
            &pinned("text_to_video", "Seedance 2"),
            &avail(&[("atlas", true)]),
        )
        .expect("resolved");
    assert_eq!(seedance.resolved_model_id, "bytedance/seedance-v1.5-pro");
}

#[test]
fn pinned_requested_and_resolved_model_always_match() {
    let catalog = builtin_catalog();
    let resolver = CapabilityResolver::new(&catalog);

    for query in ["google/nano-banana-2", "nano-banana-2", "Nano Banana 2"] {
        let resolved = resolver
            .resolve(
                &pinned("text_to_image", query),
                &avail(&[("atlas", true), ("fal", true)]),
            )
            .expect("resolved");
        assert_eq!(
            resolved.requested_model_id.as_deref(),
            Some(resolved.resolved_model_id.as_str()),
            "P4 violated for query {query}"
        );
    }
}

#[test]
fn pinned_falls_to_unique_available_binding_when_default_is_down() {
    let catalog = builtin_catalog();
    let resolver = CapabilityResolver::new(&catalog);

    let resolved = resolver
        .resolve(
            &pinned("text_to_image", "Nano Banana"),
            &avail(&[("atlas", false), ("fal", true)]),
        )
        .expect("resolved");

    assert_eq!(
        resolved.binding_id,
        "google.nano-banana-2.text-to-image.fal.v1"
    );
    assert_eq!(resolved.resolved_model_id, "google/nano-banana-2");
}

#[test]
fn pinned_fails_closed_when_all_bindings_unavailable() {
    let catalog = builtin_catalog();
    let resolver = CapabilityResolver::new(&catalog);

    let err = resolver
        .resolve(
            &pinned("text_to_image", "Nano Banana"),
            &avail(&[("atlas", false), ("fal", false)]),
        )
        .expect_err("unavailable");

    assert_eq!(err.code(), "BINDING_UNAVAILABLE");
    assert!(err.recoverable());
}

#[test]
fn connector_missing_from_availability_map_counts_as_down() {
    let catalog = builtin_catalog();
    let resolver = CapabilityResolver::new(&catalog);

    let err = resolver
        .resolve(&pinned("text_to_video", "Seedance"), &avail(&[]))
        .expect_err("fail closed");

    assert_eq!(err.code(), "BINDING_UNAVAILABLE");
}

#[test]
fn unknown_model_is_not_found_never_substring_matched() {
    let catalog = builtin_catalog();
    let resolver = CapabilityResolver::new(&catalog);

    for query in ["sora", "banana", "nano", ""] {
        let err = resolver
            .resolve(
                &pinned("text_to_image", query),
                &avail(&[("atlas", true), ("fal", true)]),
            )
            .expect_err("unknown model");
        assert_eq!(err.code(), "MODEL_NOT_FOUND", "query `{query}`");
    }
}

#[test]
fn model_without_binding_for_capability_reports_binding_not_found() {
    let catalog = builtin_catalog();
    let resolver = CapabilityResolver::new(&catalog);

    // Seedance exists but has no text_to_image binding (fixture
    // intent-model-mismatch.json expects exactly this failure).
    let err = resolver
        .resolve(
            &pinned("text_to_image", "Seedance 2"),
            &avail(&[("atlas", true), ("fal", true)]),
        )
        .expect_err("mismatch");

    assert_eq!(err.code(), "BINDING_NOT_FOUND");
    assert!(!err.recoverable());
}

#[test]
fn ambiguous_model_names_require_clarification() {
    let mut catalog = builtin_catalog();
    let mut clone = catalog.models[0].clone();
    clone.model_id = "other/nano-banana-pro".to_owned();
    clone.display_name = "Nano Banana Pro".to_owned();
    clone.aliases = vec!["nano banana".to_owned()];
    let mut models = catalog.models.clone();
    models.push(clone);
    catalog = CatalogSnapshot::build(
        catalog.capabilities.clone(),
        models,
        catalog.bindings.clone(),
        catalog.connectors.clone(),
        catalog.workflow_backends.clone(),
        catalog.default_bindings.clone(),
    );
    let resolver = CapabilityResolver::new(&catalog);

    let err = resolver
        .resolve(
            &pinned("text_to_image", "nano banana"),
            &avail(&[("atlas", true), ("fal", true)]),
        )
        .expect_err("ambiguous");

    assert_eq!(err.code(), "MODEL_AMBIGUOUS");
    assert!(err.recoverable());
}

#[test]
fn policy_uses_only_the_configured_default_binding() {
    let catalog = builtin_catalog();
    let resolver = CapabilityResolver::new(&catalog);

    let resolved = resolver
        .resolve(
            &policy("text_to_image"),
            &avail(&[("atlas", true), ("fal", true)]),
        )
        .expect("resolved");

    assert_eq!(resolved.requested_model_id, None);
    assert_eq!(
        resolved.binding_id,
        "google.nano-banana-2.text-to-image.atlas.v1"
    );
}

#[test]
fn policy_without_configured_default_is_ambiguous_even_for_one_candidate() {
    let catalog = builtin_catalog();
    let mut defaults = catalog.default_bindings.clone();
    defaults.remove("text_to_video");
    let catalog = CatalogSnapshot::build(
        catalog.capabilities.clone(),
        catalog.models.clone(),
        catalog.bindings.clone(),
        catalog.connectors.clone(),
        catalog.workflow_backends.clone(),
        defaults,
    );
    let resolver = CapabilityResolver::new(&catalog);

    // text_to_video has exactly one binding, but P5 forbids implicit policy
    // selection without an explicit default.
    let err = resolver
        .resolve(&policy("text_to_video"), &avail(&[("atlas", true)]))
        .expect_err("no default");

    assert_eq!(err.code(), "BINDING_AMBIGUOUS");
    assert!(err.recoverable());
}

#[test]
fn capability_without_any_binding_reports_binding_not_found() {
    let catalog = builtin_catalog();
    let resolver = CapabilityResolver::new(&catalog);

    let err = resolver
        .resolve(&policy("image_to_video"), &avail(&[("atlas", true)]))
        .expect_err("no bindings in V1 seed");

    assert_eq!(err.code(), "BINDING_NOT_FOUND");
}

#[test]
fn unknown_capability_is_rejected_first() {
    let catalog = builtin_catalog();
    let resolver = CapabilityResolver::new(&catalog);

    let err = resolver
        .resolve(&pinned("style_transfer", "Nano Banana"), &avail(&[]))
        .expect_err("unknown capability");

    assert_eq!(err.code(), "CAPABILITY_NOT_FOUND");
}

#[test]
fn pinned_model_lifecycle_is_preserved_in_catalog_data() {
    // Guard against seed data drift: both V1 models stay active.
    let catalog = builtin_catalog();
    assert!(
        catalog
            .models
            .iter()
            .all(|model: &ModelDefinition| model.lifecycle == ModelLifecycle::Active)
    );
}
