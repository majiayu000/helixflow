//! Built-in node definitions and construction helpers (#147 split;
//! definitions are byte-identical to the pre-split registry).

use crate::types::{
    EstimatedCostRef, NodeDefinition, ParamSpec, ParamsSchema, PortDefinition, PortType,
};

pub fn builtin_node_definitions() -> Vec<NodeDefinition> {
    vec![
        node(
            "input.text",
            "Text Input",
            "input",
            None,
            None,
            "A user-provided text value.",
            vec![],
            vec![port("text", PortType::Text, true)],
            schema(&["text"], [("text", ParamSpec::string())]),
        ),
        node(
            "input.image",
            "Image Input",
            "input",
            None,
            None,
            "A user-uploaded image artifact.",
            vec![],
            vec![port("image", PortType::Image, true)],
            schema(&["storage_uri"], [("storage_uri", ParamSpec::string())]),
        ),
        node(
            "llm.prompt_writer",
            "Prompt Writer",
            "text",
            None,
            Some("prompt_writer"),
            "Drafts a generation prompt from source text.",
            vec![port("text", PortType::Text, true)],
            vec![port("prompt", PortType::Text, true)],
            schema(
                &["style"],
                [(
                    "style",
                    ParamSpec::string_enum(&["cinematic", "product", "plain"]),
                )],
            ),
        ),
        node(
            "image.generate",
            "Generate Image",
            "image",
            None,
            Some("image_generate"),
            "Generates an image artifact from a prompt.",
            vec![port("prompt", PortType::Text, true)],
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
        node(
            "video.text_to_video",
            "Text To Video",
            "video",
            None,
            Some("text_to_video"),
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
        node(
            "video.image_to_video",
            "Image To Video",
            "video",
            None,
            Some("image_to_video"),
            "Animates an input image into a video artifact.",
            vec![
                port("image", PortType::Image, true),
                port("prompt", PortType::Text, false),
            ],
            vec![port("video", PortType::Video, true)],
            schema(
                &["duration_sec"],
                [
                    ("prompt", ParamSpec::string()),
                    ("duration_sec", ParamSpec::integer_range(1, 10)),
                ],
            ),
        ),
        node(
            "output.save",
            "Save Output",
            "output",
            None,
            None,
            "Marks an upstream artifact as a workflow output.",
            vec![port("artifact", PortType::Json, true)],
            vec![],
            schema::<0>(&[], []),
        ),
    ]
}

fn node(
    node_type: &str,
    title: &str,
    category: &str,
    provider: Option<&str>,
    capability: Option<&str>,
    description: &str,
    inputs: Vec<PortDefinition>,
    outputs: Vec<PortDefinition>,
    params_schema: ParamsSchema,
) -> NodeDefinition {
    NodeDefinition {
        node_type: node_type.to_owned(),
        title: title.to_owned(),
        category: category.to_owned(),
        provider: provider.map(str::to_owned),
        capability: capability.map(str::to_owned),
        description: description.to_owned(),
        inputs,
        outputs,
        params_schema,
        estimated_cost: capability.map(|capability| EstimatedCostRef {
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
