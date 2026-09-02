//! Built-in node definitions and construction helpers (#147 split;
//! definitions are byte-identical to the pre-split registry).

use crate::catalog::{BindingAvailability, CapabilityDefinition, MediaCategory};
use crate::catalog_seed::builtin_catalog;
use crate::types::{
    EstimatedCostRef, NodeDefinition, ParamSpec, ParamsSchema, PortDefinition, PortType,
};

pub fn builtin_node_definitions() -> Vec<NodeDefinition> {
    let mut definitions = vec![
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
    ];

    let catalog = builtin_catalog();
    definitions.extend(catalog.capabilities.iter().filter_map(|capability| {
        catalog
            .bindings
            .iter()
            .any(|binding| {
                binding.capability_id == capability.capability_id
                    && binding.availability == BindingAvailability::Enabled
            })
            .then(|| capability_node(capability))
    }));
    definitions
}

fn capability_node(capability: &CapabilityDefinition) -> NodeDefinition {
    NodeDefinition {
        node_type: capability.node_type.clone(),
        title: capability.display_name.clone(),
        category: match capability.category {
            MediaCategory::Text => "text",
            MediaCategory::Image => "image",
            MediaCategory::Video => "video",
        }
        .to_owned(),
        provider: None,
        capability: Some(capability.capability_id.clone()),
        description: capability.description.clone(),
        inputs: capability.inputs.clone(),
        outputs: capability.outputs.clone(),
        params_schema: capability.params_schema.clone(),
        estimated_cost: Some(EstimatedCostRef {
            unit: "call".to_owned(),
            catalog_key: capability.capability_id.clone(),
        }),
    }
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
