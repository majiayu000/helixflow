use super::*;
use serde_json::json;

#[test]
fn reports_module_name() {
    assert_eq!(module_name(), "registry");
}

#[test]
fn serializes_node_definition_boundary() {
    let definition = NodeDefinition {
        node_type: "mock.image".to_string(),
        title: "Generate Image".to_string(),
        category: "mock".to_string(),
        provider: Some("mock".to_string()),
        capability: Some("image_generate".to_string()),
        description: "A mock image node.".to_string(),
        inputs: vec![PortDefinition {
            name: "prompt".to_string(),
            port_type: PortType::Text,
            required: true,
        }],
        outputs: vec![PortDefinition {
            name: "image".to_string(),
            port_type: PortType::Image,
            required: true,
        }],
        params_schema: schema(&["prompt"], [("prompt", ParamSpec::string())]),
        estimated_cost: Some(EstimatedCostRef {
            unit: "call".to_string(),
            catalog_key: "mock.image_generate".to_string(),
        }),
    };

    let encoded = serde_json::to_value(&definition).expect("serialize node definition");

    assert_eq!(encoded["type"], "mock.image");
    assert_eq!(encoded["title"], "Generate Image");
    assert_eq!(encoded["category"], "mock");
    assert_eq!(encoded["provider"], "mock");
}

#[test]
fn rejects_unknown_node_types() {
    let registry = NodeRegistry::builtin();

    let err = registry
        .validate_node_params("video.missing", &json!({}))
        .expect_err("unknown node should fail");

    assert_eq!(
        err,
        RegistryError::UnknownNodeType("video.missing".to_owned())
    );
}

#[test]
fn validates_required_params_and_schema() {
    let registry = NodeRegistry::builtin();

    registry
        .validate_node_params(
            "video.text_to_video",
            &json!({
                "prompt": "A clean product ad",
                "duration_sec": 5,
                "aspect_ratio": "9:16"
            }),
        )
        .expect("valid params");

    assert!(matches!(
        registry.validate_node_params(
            "video.text_to_video",
            &json!({ "prompt": "missing duration", "aspect_ratio": "9:16" })
        ),
        Err(RegistryError::MissingRequiredParam { .. })
    ));
    assert!(matches!(
        registry.validate_node_params(
            "video.text_to_video",
            &json!({
                "prompt": "bad duration",
                "duration_sec": 30,
                "aspect_ratio": "9:16"
            })
        ),
        Err(RegistryError::ParamOutOfRange { .. })
    ));
}

#[test]
fn exports_catalog_for_agent_context() {
    let registry = NodeRegistry::builtin();
    let catalog = registry.export_catalog();

    assert_eq!(catalog.schema_version, 1);
    assert!(
        catalog
            .nodes
            .iter()
            .any(|node| node.node_type == "video.text_to_video")
    );
}

#[test]
fn gh130_baseline_params_model_is_rejected_as_unknown() {
    // GH130 T0 baseline: the graph layer rejects `params.model` while the
    // provider layer silently falls back to an internal default model, so a
    // user-selected model cannot survive the graph contract. SP130-T2/T4
    // replace this with an explicit model binding.
    let registry = NodeRegistry::builtin();

    for (node_type, params) in [
        (
            "image.generate",
            json!({
                "prompt": "a product image",
                "aspect_ratio": "1:1",
                "model": "google/nano-banana-2"
            }),
        ),
        (
            "video.text_to_video",
            json!({
                "prompt": "a product video",
                "duration_sec": 5,
                "aspect_ratio": "9:16",
                "model": "bytedance/seedance-v1.5-pro"
            }),
        ),
    ] {
        let err = registry
            .validate_node_params(node_type, &params)
            .expect_err("params.model must be rejected");
        assert!(matches!(
            err,
            RegistryError::UnknownParam { ref param, .. } if param == "model"
        ));
    }
}

#[test]
fn executable_nodes_are_provider_neutral() {
    let registry = NodeRegistry::builtin();

    for node_type in ["llm.prompt_writer", "image.generate", "video.text_to_video"] {
        let definition = registry.definition(node_type).expect("definition");
        assert_eq!(definition.provider, None);
        assert!(!definition.title.contains("Mock"));
        assert_eq!(
            definition
                .estimated_cost
                .as_ref()
                .map(|cost| cost.catalog_key.as_str()),
            definition.capability.as_deref()
        );
    }
}
