//! V1 seed catalog (GH130 T1).
//!
//! Seeded from the models the providers actually call today, per the
//! maintainer decision recorded on issue #130. New models are added here as
//! data — never as provider code branches.

use std::collections::BTreeMap;

use serde_json::json;

use crate::catalog::{
    BindingAvailability, CapabilityBinding, CapabilityDefinition, CatalogSnapshot,
    ConnectorDefinition, ConnectorKind, ConnectorStatus, ImplementationTarget, MediaCategory,
    ModelDefinition, ModelLifecycle,
};
use crate::{ParamSpec, ParamsSchema, PortDefinition, PortType, port, port_many, schema};

pub fn builtin_catalog() -> CatalogSnapshot {
    CatalogSnapshot::build(
        capabilities(),
        models(),
        bindings(),
        connectors(),
        Vec::new(),
        default_bindings(),
    )
}

fn capabilities() -> Vec<CapabilityDefinition> {
    vec![
        capability(
            "prompt_writer",
            "llm.prompt_writer",
            MediaCategory::Text,
            "Prompt Writer",
            "Drafts a generation prompt from source text.",
            vec![port("text", PortType::Text, true)],
            vec![port("prompt", PortType::Text, true)],
            schema(
                &[],
                [(
                    "style",
                    ParamSpec::string_enum(&["cinematic", "product", "plain"]),
                )],
            ),
        ),
        capability(
            "text_to_image",
            "image.generate",
            MediaCategory::Image,
            "Text To Image",
            "Generates an image artifact from a prompt.",
            vec![
                port("prompt", PortType::Text, true),
                port_many("in", PortType::Image, false),
            ],
            vec![port("image", PortType::Image, true)],
            schema(
                &["prompt", "aspect_ratio"],
                [
                    ("prompt", ParamSpec::string()),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&["1:1", "9:16", "16:9"]),
                    ),
                    ("seed", ParamSpec::integer()),
                ],
            ),
        ),
        capability(
            "image_edit",
            "image.edit",
            MediaCategory::Image,
            "Image Edit",
            "Edits an image artifact from a prompt.",
            vec![
                port("image", PortType::Image, true),
                port("prompt", PortType::Text, true),
            ],
            vec![port("image", PortType::Image, true)],
            schema(&["prompt"], [("prompt", ParamSpec::string())]),
        ),
        capability(
            "text_to_video",
            "video.text_to_video",
            MediaCategory::Video,
            "Text To Video",
            "Generates a video artifact from a prompt.",
            vec![port("prompt", PortType::Text, true)],
            vec![port("video", PortType::Video, true)],
            schema(
                &["prompt", "duration_sec", "aspect_ratio"],
                [
                    ("prompt", ParamSpec::string()),
                    ("duration_sec", ParamSpec::integer_range(1, 10)),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&["1:1", "9:16", "16:9"]),
                    ),
                    ("seed", ParamSpec::integer()),
                ],
            ),
        ),
        capability(
            "image_to_video",
            "video.image_to_video",
            MediaCategory::Video,
            "Image To Video",
            "Animates an input image into a video artifact.",
            vec![
                port_many("image", PortType::Image, true),
                port_many("video", PortType::Video, false),
                port_many("audio", PortType::Audio, false),
                port("prompt", PortType::Text, false),
            ],
            vec![port("video", PortType::Video, true)],
            schema(
                &["duration_sec"],
                [
                    ("prompt", ParamSpec::string()),
                    ("duration_sec", ParamSpec::integer_range(4, 12)),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&["21:9", "16:9", "4:3", "1:1", "3:4", "9:16"]),
                    ),
                    ("resolution", ParamSpec::string_enum(&["720p", "480p"])),
                    ("generate_audio", ParamSpec::boolean()),
                    ("camera_fixed", ParamSpec::boolean()),
                    ("seed", ParamSpec::integer()),
                ],
            ),
        ),
        capability(
            "video_extend",
            "video.extend",
            MediaCategory::Video,
            "Video Extend",
            "Extends an existing video artifact.",
            vec![port("video", PortType::Video, true)],
            vec![port("video", PortType::Video, true)],
            schema(
                &["duration_sec"],
                [("duration_sec", ParamSpec::integer_range(1, 10))],
            ),
        ),
        capability(
            "upscale_image",
            "image.upscale",
            MediaCategory::Image,
            "Upscale Image",
            "Upscales an image artifact.",
            vec![port("image", PortType::Image, true)],
            vec![port("image", PortType::Image, true)],
            schema(&[], [("scale", ParamSpec::integer_range(2, 4))]),
        ),
        capability(
            "upscale_video",
            "video.upscale",
            MediaCategory::Video,
            "Upscale Video",
            "Upscales a video artifact.",
            vec![port("video", PortType::Video, true)],
            vec![port("video", PortType::Video, true)],
            schema(&[], [("scale", ParamSpec::integer_range(2, 4))]),
        ),
        capability(
            "image_analyze",
            "image.analyze",
            MediaCategory::Text,
            "Image Analyze",
            "Analyzes an image and returns text.",
            vec![port("image", PortType::Image, true)],
            vec![port("text", PortType::Text, true)],
            schema(&[], [("prompt", ParamSpec::string())]),
        ),
    ]
}

fn models() -> Vec<ModelDefinition> {
    vec![
        ModelDefinition {
            model_id: "google/nano-banana-2".to_owned(),
            family_id: "nano-banana".to_owned(),
            display_name: "Nano Banana 2".to_owned(),
            vendor: "google".to_owned(),
            lifecycle: ModelLifecycle::Active,
            aliases: vec!["nano banana".to_owned(), "nano banana 2".to_owned()],
        },
        ModelDefinition {
            model_id: "deepseek-ai/DeepSeek-V3-0324".to_owned(),
            family_id: "deepseek-v3".to_owned(),
            display_name: "DeepSeek V3".to_owned(),
            vendor: "deepseek-ai".to_owned(),
            lifecycle: ModelLifecycle::Active,
            aliases: vec!["deepseek".to_owned(), "deepseek v3".to_owned()],
        },
        ModelDefinition {
            model_id: "bytedance/seedance-v1.5-pro".to_owned(),
            family_id: "seedance".to_owned(),
            display_name: "Seedance 1.5 Pro".to_owned(),
            vendor: "bytedance".to_owned(),
            lifecycle: ModelLifecycle::Active,
            aliases: vec![
                "seedance".to_owned(),
                "seedance 1.5".to_owned(),
                "seedance 1.5 pro".to_owned(),
            ],
        },
        ModelDefinition {
            model_id: "bytedance/seedance-2.0-fast".to_owned(),
            family_id: "seedance".to_owned(),
            display_name: "Seedance 2.0 Fast".to_owned(),
            vendor: "bytedance".to_owned(),
            lifecycle: ModelLifecycle::Active,
            aliases: vec![
                "seedance 2".to_owned(),
                "seedance 2.0".to_owned(),
                "seedance 2.0 fast".to_owned(),
                "seedance 2 fast".to_owned(),
            ],
        },
    ]
}

fn connectors() -> Vec<ConnectorDefinition> {
    vec![
        ConnectorDefinition {
            connector_id: "atlas".to_owned(),
            provider_id: "atlas".to_owned(),
            kind: ConnectorKind::Api,
            enabled: true,
            status: ConnectorStatus::Active,
        },
        ConnectorDefinition {
            connector_id: "fal".to_owned(),
            provider_id: "fal".to_owned(),
            kind: ConnectorKind::Api,
            enabled: true,
            status: ConnectorStatus::Active,
        },
    ]
}

fn bindings() -> Vec<CapabilityBinding> {
    vec![
        CapabilityBinding {
            binding_id: "google.nano-banana-2.text-to-image.atlas.v1".to_owned(),
            capability_id: "text_to_image".to_owned(),
            model_id: "google/nano-banana-2".to_owned(),
            implementation: ImplementationTarget::ApiConnector {
                connector_id: "atlas".to_owned(),
                operation_id: "google/nano-banana-2/text-to-image".to_owned(),
            },
            mode: "text_to_image".to_owned(),
            input_schema: schema(
                &["prompt", "aspect_ratio"],
                [
                    ("prompt", ParamSpec::string()),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&["1:1", "9:16", "16:9"]),
                    ),
                    ("seed", ParamSpec::integer()),
                ],
            ),
            output_schema: schema(&[], [("image", ParamSpec::string())]),
            defaults: json!({ "aspect_ratio": "1:1" }),
            availability: BindingAvailability::Enabled,
            binding_revision: "v1".to_owned(),
        },
        CapabilityBinding {
            binding_id: "google.nano-banana-2.image-edit.atlas.v1".to_owned(),
            capability_id: "image_edit".to_owned(),
            model_id: "google/nano-banana-2".to_owned(),
            implementation: ImplementationTarget::ApiConnector {
                connector_id: "atlas".to_owned(),
                operation_id: "google/nano-banana-2/edit".to_owned(),
            },
            mode: "image_edit".to_owned(),
            input_schema: schema(
                &["image", "prompt"],
                [
                    ("image", ParamSpec::string()),
                    ("prompt", ParamSpec::string()),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&["1:1", "9:16", "16:9"]),
                    ),
                ],
            ),
            output_schema: schema(&[], [("image", ParamSpec::string())]),
            defaults: json!({}),
            availability: BindingAvailability::Enabled,
            binding_revision: "v1".to_owned(),
        },
        CapabilityBinding {
            binding_id: "google.nano-banana-2.text-to-image.fal.v1".to_owned(),
            capability_id: "text_to_image".to_owned(),
            model_id: "google/nano-banana-2".to_owned(),
            implementation: ImplementationTarget::ApiConnector {
                connector_id: "fal".to_owned(),
                operation_id: "fal-ai/nano-banana-2".to_owned(),
            },
            mode: "text_to_image".to_owned(),
            input_schema: schema(
                &["prompt"],
                [
                    ("prompt", ParamSpec::string()),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&["1:1", "9:16", "16:9"]),
                    ),
                ],
            ),
            output_schema: schema(&[], [("image", ParamSpec::string())]),
            defaults: json!({}),
            availability: BindingAvailability::Enabled,
            binding_revision: "v1".to_owned(),
        },
        CapabilityBinding {
            binding_id: "deepseek-ai.deepseek-v3.prompt-writer.atlas.v1".to_owned(),
            capability_id: "prompt_writer".to_owned(),
            model_id: "deepseek-ai/DeepSeek-V3-0324".to_owned(),
            implementation: ImplementationTarget::ApiConnector {
                connector_id: "atlas".to_owned(),
                operation_id: "deepseek-ai/DeepSeek-V3-0324".to_owned(),
            },
            mode: "prompt_writer".to_owned(),
            input_schema: schema(
                &[],
                [
                    ("prompt", ParamSpec::string()),
                    (
                        "style",
                        ParamSpec::string_enum(&["cinematic", "product", "plain"]),
                    ),
                ],
            ),
            output_schema: schema(&[], [("prompt", ParamSpec::string())]),
            defaults: json!({ "style": "plain" }),
            availability: BindingAvailability::Enabled,
            binding_revision: "v1".to_owned(),
        },
        CapabilityBinding {
            binding_id: "bytedance.seedance-v1-5-pro.text-to-video.atlas.v1".to_owned(),
            capability_id: "text_to_video".to_owned(),
            model_id: "bytedance/seedance-v1.5-pro".to_owned(),
            implementation: ImplementationTarget::ApiConnector {
                connector_id: "atlas".to_owned(),
                operation_id: "bytedance/seedance-v1.5-pro/text-to-video-fast".to_owned(),
            },
            mode: "text_to_video".to_owned(),
            input_schema: schema(
                &["prompt", "duration_sec"],
                [
                    ("prompt", ParamSpec::string()),
                    ("duration_sec", ParamSpec::integer_range(1, 10)),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&["1:1", "9:16", "16:9"]),
                    ),
                ],
            ),
            output_schema: schema(&[], [("video", ParamSpec::string())]),
            defaults: json!({ "duration_sec": 5 }),
            availability: BindingAvailability::Enabled,
            binding_revision: "v1".to_owned(),
        },
        CapabilityBinding {
            binding_id: "bytedance.seedance-v1-5-pro.image-to-video.atlas.v1".to_owned(),
            capability_id: "image_to_video".to_owned(),
            model_id: "bytedance/seedance-v1.5-pro".to_owned(),
            implementation: ImplementationTarget::ApiConnector {
                connector_id: "atlas".to_owned(),
                operation_id: "bytedance/seedance-v1.5-pro/image-to-video".to_owned(),
            },
            mode: "image_to_video".to_owned(),
            input_schema: schema(
                &["image", "duration_sec"],
                [
                    ("image", ParamSpec::string()),
                    ("prompt", ParamSpec::string()),
                    ("duration_sec", ParamSpec::integer_range(4, 12)),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&["21:9", "16:9", "4:3", "1:1", "3:4", "9:16"]),
                    ),
                    ("resolution", ParamSpec::string_enum(&["720p", "480p"])),
                    ("generate_audio", ParamSpec::boolean()),
                    ("camera_fixed", ParamSpec::boolean()),
                    ("seed", ParamSpec::integer()),
                ],
            ),
            output_schema: schema(&[], [("video", ParamSpec::string())]),
            defaults: json!({
                "duration_sec": 5,
                "resolution": "720p",
                "generate_audio": true,
                "camera_fixed": false,
                "seed": -1
            }),
            availability: BindingAvailability::Enabled,
            binding_revision: "v1".to_owned(),
        },
        CapabilityBinding {
            binding_id: "bytedance.seedance-2-0-fast.text-to-video.atlas.v1".to_owned(),
            capability_id: "text_to_video".to_owned(),
            model_id: "bytedance/seedance-2.0-fast".to_owned(),
            implementation: ImplementationTarget::ApiConnector {
                connector_id: "atlas".to_owned(),
                operation_id: "bytedance/seedance-2.0-fast/text-to-video".to_owned(),
            },
            mode: "text_to_video".to_owned(),
            input_schema: schema(
                &["prompt", "duration_sec"],
                [
                    ("prompt", ParamSpec::string()),
                    ("duration_sec", ParamSpec::integer_range(4, 15)),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&["1:1", "9:16", "16:9"]),
                    ),
                ],
            ),
            output_schema: schema(&[], [("video", ParamSpec::string())]),
            defaults: json!({ "duration_sec": 5 }),
            availability: BindingAvailability::Enabled,
            binding_revision: "v1".to_owned(),
        },
        CapabilityBinding {
            binding_id: "bytedance.seedance-2-0-fast.reference-to-video.atlas.v1".to_owned(),
            capability_id: "image_to_video".to_owned(),
            model_id: "bytedance/seedance-2.0-fast".to_owned(),
            implementation: ImplementationTarget::ApiConnector {
                connector_id: "atlas".to_owned(),
                operation_id: "bytedance/seedance-2.0-fast/reference-to-video".to_owned(),
            },
            mode: "image_to_video".to_owned(),
            input_schema: schema(
                &["image", "duration_sec"],
                [
                    ("image", ParamSpec::string()),
                    ("prompt", ParamSpec::string()),
                    ("duration_sec", ParamSpec::integer_range(4, 15)),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&[
                            "21:9", "16:9", "4:3", "1:1", "3:4", "9:16", "adaptive",
                        ]),
                    ),
                    ("resolution", ParamSpec::string_enum(&["480p", "720p"])),
                    ("generate_audio", ParamSpec::boolean()),
                    ("camera_fixed", ParamSpec::boolean()),
                    ("seed", ParamSpec::integer()),
                ],
            ),
            output_schema: schema(&[], [("video", ParamSpec::string())]),
            defaults: json!({
                "duration_sec": 5,
                "resolution": "720p",
                "generate_audio": true,
                "seed": -1
            }),
            availability: BindingAvailability::Enabled,
            binding_revision: "v1".to_owned(),
        },
    ]
}

fn default_bindings() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "prompt_writer".to_owned(),
            "deepseek-ai.deepseek-v3.prompt-writer.atlas.v1".to_owned(),
        ),
        (
            "text_to_image".to_owned(),
            "google.nano-banana-2.text-to-image.atlas.v1".to_owned(),
        ),
        (
            "image_edit".to_owned(),
            "google.nano-banana-2.image-edit.atlas.v1".to_owned(),
        ),
        (
            "text_to_video".to_owned(),
            "bytedance.seedance-v1-5-pro.text-to-video.atlas.v1".to_owned(),
        ),
        (
            "image_to_video".to_owned(),
            "bytedance.seedance-2-0-fast.reference-to-video.atlas.v1".to_owned(),
        ),
    ])
}

#[allow(clippy::too_many_arguments)]
fn capability(
    capability_id: &str,
    node_type: &str,
    category: MediaCategory,
    display_name: &str,
    description: &str,
    inputs: Vec<PortDefinition>,
    outputs: Vec<PortDefinition>,
    params_schema: ParamsSchema,
) -> CapabilityDefinition {
    CapabilityDefinition {
        capability_id: capability_id.to_owned(),
        node_type: node_type.to_owned(),
        category,
        display_name: display_name.to_owned(),
        description: description.to_owned(),
        inputs,
        outputs,
        params_schema,
    }
}
