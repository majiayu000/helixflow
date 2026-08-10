//! Built-in node definitions and construction helpers (#147 split;
//! definitions are byte-identical to the pre-split registry).

use crate::types::{
    EstimatedCostRef, NodeDefinition, ParamSpec, ParamsSchema, PortDefinition, PortType,
};

pub fn builtin_node_definitions() -> Vec<NodeDefinition> {
    vec![
        node(NodeSpec {
            node_type: "input.text",
            title: "Text Input",
            category: "input",
            provider: None,
            capability: None,
            description: "A user-provided text value.",
            inputs: vec![],
            outputs: vec![port("text", PortType::Text, true)],
            params_schema: schema(&["text"], [("text", ParamSpec::string())]),
        }),
        node(NodeSpec {
            node_type: "input.image",
            title: "Image Input",
            category: "input",
            provider: None,
            capability: None,
            description: "A user-uploaded image artifact.",
            inputs: vec![],
            outputs: vec![port("image", PortType::Image, true)],
            params_schema: schema(&["storage_uri"], [("storage_uri", ParamSpec::string())]),
        }),
        node(NodeSpec {
            node_type: "llm.prompt_writer",
            title: "Prompt Writer",
            category: "text",
            provider: None,
            capability: Some("prompt_writer"),
            description: "Drafts a generation prompt from source text.",
            inputs: vec![port("text", PortType::Text, true)],
            outputs: vec![port("prompt", PortType::Text, true)],
            params_schema: schema(
                &["style"],
                [(
                    "style",
                    ParamSpec::string_enum(&["cinematic", "product", "plain"]),
                )],
            ),
        }),
        node(NodeSpec {
            node_type: "image.generate",
            title: "Generate Image",
            category: "image",
            provider: None,
            capability: Some("text_to_image"),
            description: "Generates an image artifact from a prompt.",
            inputs: vec![port("prompt", PortType::Text, true)],
            outputs: vec![port("image", PortType::Image, true)],
            params_schema: schema(
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
        }),
        node(NodeSpec {
            node_type: "video.text_to_video",
            title: "Text To Video",
            category: "video",
            provider: None,
            capability: Some("text_to_video"),
            description: "Generates a video artifact from a prompt.",
            inputs: vec![port("prompt", PortType::Text, true)],
            outputs: vec![port("video", PortType::Video, true)],
            params_schema: schema(
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
        }),
        node(NodeSpec {
            node_type: "video.image_to_video",
            title: "Image To Video",
            category: "video",
            provider: None,
            capability: Some("image_to_video"),
            description: "Animates an input image into a video artifact.",
            inputs: vec![
                port("image", PortType::Image, true),
                port("prompt", PortType::Text, false),
            ],
            outputs: vec![port("video", PortType::Video, true)],
            params_schema: schema(
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
        }),
        node(NodeSpec {
            node_type: "output.save",
            title: "Save Output",
            category: "output",
            provider: None,
            capability: None,
            description: "Marks an upstream artifact as a workflow output.",
            inputs: vec![port("artifact", PortType::Json, true)],
            outputs: vec![],
            params_schema: schema::<0>(&[], []),
        }),
    ]
}

struct NodeSpec<'a> {
    node_type: &'a str,
    title: &'a str,
    category: &'a str,
    provider: Option<&'a str>,
    capability: Option<&'a str>,
    description: &'a str,
    inputs: Vec<PortDefinition>,
    outputs: Vec<PortDefinition>,
    params_schema: ParamsSchema,
}

fn node(spec: NodeSpec<'_>) -> NodeDefinition {
    NodeDefinition {
        node_type: spec.node_type.to_owned(),
        title: spec.title.to_owned(),
        category: spec.category.to_owned(),
        provider: spec.provider.map(str::to_owned),
        capability: spec.capability.map(str::to_owned),
        description: spec.description.to_owned(),
        inputs: spec.inputs,
        outputs: spec.outputs,
        params_schema: spec.params_schema,
        estimated_cost: spec.capability.map(|capability| EstimatedCostRef {
            unit: "call".to_owned(),
            catalog_key: capability.to_owned(),
        }),
    }
}

pub(crate) fn port(name: &str, port_type: PortType, required: bool) -> PortDefinition {
    PortDefinition {
        name: name.to_owned(),
        port_type,
        required,
    }
}

pub(crate) fn schema<const N: usize>(
    required: &[&str],
    properties: [(&str, ParamSpec); N],
) -> ParamsSchema {
    ParamsSchema {
        required: required.iter().map(|value| (*value).to_owned()).collect(),
        properties: properties
            .into_iter()
            .map(|(name, spec)| (name.to_owned(), spec))
            .collect(),
        allow_unknown: false,
    }
}
