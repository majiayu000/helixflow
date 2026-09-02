//! Deterministic stage → graph construction (tech.md §6 steps 2-8).

use std::collections::BTreeMap;

use helixflow_graph::semantics::NodeSemanticsEntry;
use helixflow_graph::{GraphEdge, GraphNode, GraphService, WorkflowGraph, port_type_label};
use helixflow_registry::PortType;
use helixflow_registry::catalog::{BindingAvailability, CatalogSnapshot, ImplementationSelection};
use helixflow_registry::resolver::{
    CapabilityResolver, ConnectorAvailability, ResolveError, ResolveRequest,
};
use serde_json::{Map, Value};

use crate::errors::{ClarifyFirst, CompileError};
use crate::intent::{IntentPlan, StageIntent, TopologyIntent};
use crate::{LayoutHint, ResolvedStage};

const COLUMN_WIDTH: f32 = 240.0;
const ROW_HEIGHT: f32 = 180.0;

pub(crate) struct BuiltGraph {
    pub target: WorkflowGraph,
    pub resolved_stages: Vec<ResolvedStage>,
    pub layout_hints: Vec<LayoutHint>,
    pub diagnostics: Vec<String>,
}

pub(crate) fn build(
    intent: &IntentPlan,
    service: &GraphService,
    catalog: &CatalogSnapshot,
    availability: &ConnectorAvailability,
    connector_preference: Option<&str>,
) -> Result<Result<BuiltGraph, ClarifyFirst>, CompileError> {
    let resolver = CapabilityResolver::new(catalog);
    let registry = service.registry();

    let mut nodes = BTreeMap::new();
    let mut edges = Vec::new();
    let mut resolved_stages = Vec::new();
    let mut layout_hints = Vec::new();
    let mut diagnostics = Vec::new();
    let mut synthesized_inputs: Vec<(String, String)> = Vec::new();

    for (index, stage) in intent.stages.iter().enumerate() {
        // Step 2: resolve capability/model/binding. Recoverable resolver
        // outcomes route to clarification. A missing explicit binding remains
        // fail-closed, but is a user-actionable catalog gap rather than an
        // internal server error: never substitute another capability/model.
        let resolved = match resolver.resolve(
            &ResolveRequest {
                capability_id: stage.capability_id.clone(),
                requested_model: stage.requested_model.clone(),
                connector_preference: connector_preference.map(str::to_owned),
            },
            availability,
        ) {
            Ok(resolved) => resolved,
            Err(ResolveError::BindingNotFound {
                capability_id,
                model_id,
            }) => {
                return Ok(Err(binding_not_found_clarification(
                    stage,
                    catalog,
                    &capability_id,
                    model_id.as_deref(),
                )));
            }
            Err(err) if err.recoverable() => {
                return Ok(Err(ClarifyFirst::new(
                    err.code(),
                    vec![format!("{}.model", stage.stage_id)],
                    serde_json::json!({ "stageId": stage.stage_id, "detail": err.to_string() }),
                    "ask the user to pick a specific model or enable a connector",
                )));
            }
            Err(err) => return Err(err.into()),
        };

        let node_type = catalog
            .capability(&resolved.capability_id)
            .map(|capability| capability.node_type.as_str())
            .ok_or_else(|| CompileError::UnmappedCapability {
                capability_id: resolved.capability_id.clone(),
            })?;
        let definition =
            registry
                .definition(node_type)
                .map_err(|err| CompileError::GraphInvalid {
                    code: "CAPABILITY_NOT_FOUND",
                    message: err.to_string(),
                })?;
        debug_assert_eq!(
            definition.capability.as_deref().unwrap_or_default(),
            resolved.capability_id
        );

        // Steps 3-4: wire typed inputs by port name and type.
        for input in &stage.input_from {
            let source_stage = intent
                .stages
                .iter()
                .find(|candidate| candidate.stage_id == input.stage_id)
                .expect("validated earlier");
            let source_type = node_type_for_stage(source_stage, catalog)?;
            let source_def =
                registry
                    .definition(source_type)
                    .map_err(|err| CompileError::GraphInvalid {
                        code: "CAPABILITY_NOT_FOUND",
                        message: err.to_string(),
                    })?;
            let Some(source_port) = source_def
                .outputs
                .iter()
                .find(|port| port.name == input.output)
            else {
                return Err(CompileError::PortTypeMismatch {
                    stage_id: stage.stage_id.clone(),
                    from_stage: input.stage_id.clone(),
                    output: input.output.clone(),
                    reason: "source stage has no such output".to_owned(),
                });
            };
            let Some(target_port) = definition
                .inputs
                .iter()
                .find(|port| port.name == input.output && port.port_type == source_port.port_type)
            else {
                return Err(CompileError::PortTypeMismatch {
                    stage_id: stage.stage_id.clone(),
                    from_stage: input.stage_id.clone(),
                    output: input.output.clone(),
                    reason: format!(
                        "no `{}` input of type {:?} on `{}`",
                        input.output, source_port.port_type, stage.capability_id
                    ),
                });
            };
            edges.push(GraphEdge {
                from: [input.stage_id.clone(), source_port.name.clone()],
                to: [stage.stage_id.clone(), target_port.name.clone()],
                edge_type: port_type_label(source_port.port_type).to_owned(),
            });
        }

        // Merge params: binding defaults < stage params (data-driven, no
        // provider defaults anywhere).
        let binding = catalog
            .binding(&resolved.binding_id)
            .expect("resolver returned an existing binding");
        let mut params = Map::new();
        if let Some(defaults) = binding.defaults.as_object() {
            for (key, value) in defaults {
                params.insert(key.clone(), value.clone());
            }
        }
        if let Some(stage_params) = stage.params.as_object() {
            for (key, value) in stage_params {
                params.insert(key.clone(), value.clone());
            }
        }

        // A required text input with a literal value but no upstream stage is
        // materialized as an `input.text` node: the Agent supplies the value,
        // the compiler owns the structure.
        for port in definition
            .inputs
            .iter()
            .filter(|port| port.required && port.port_type == PortType::Text)
        {
            let already_wired = edges
                .iter()
                .any(|edge| edge.to[0] == stage.stage_id && edge.to[1] == port.name);
            if already_wired || definition.params_schema.properties.contains_key(&port.name) {
                continue;
            }
            let Some(value) = params.remove(&port.name) else {
                continue;
            };
            let input_id = format!("{}-{}-input", stage.stage_id, port.name);
            nodes.insert(
                input_id.clone(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Text Input".to_owned(),
                    params: serde_json::json!({ "text": value }),
                    pos: [0.0, 0.0],
                    size: None,
                    semantics: None,
                },
            );
            edges.push(GraphEdge {
                from: [input_id.clone(), "text".to_owned()],
                to: [stage.stage_id.clone(), port.name.clone()],
                edge_type: "text".to_owned(),
            });
            synthesized_inputs.push((input_id, stage.stage_id.clone()));
            diagnostics.push(format!(
                "{}: materialized `input.text` node for required `{}` input",
                stage.stage_id, port.name
            ));
        }

        // Step 5: every required input must be satisfied by a wired edge, a
        // literal param, or a schema default — otherwise clarify (P7).
        let wired: Vec<&str> = edges
            .iter()
            .filter(|edge| edge.to[0] == stage.stage_id)
            .map(|edge| edge.to[1].as_str())
            .collect();
        let mut missing = Vec::new();
        for port in definition.inputs.iter().filter(|port| port.required) {
            let satisfied = wired.contains(&port.name.as_str())
                || params.get(&port.name).is_some_and(|value| !value.is_null());
            if !satisfied {
                missing.push(format!("{}.{}", stage.stage_id, port.name));
            }
        }
        for required in &definition.params_schema.required {
            if params.contains_key(required) {
                continue;
            }
            if wired.contains(&required.as_str()) {
                // The v1 structural contract still wants the param present;
                // executors prefer the wired value at run time.
                params.insert(required.clone(), Value::String(String::new()));
                diagnostics.push(format!(
                    "{}: filled placeholder param `{required}` (value arrives over the wire)",
                    stage.stage_id
                ));
                continue;
            }
            missing.push(format!("{}.{}", stage.stage_id, required));
        }
        if !missing.is_empty() {
            missing.sort_unstable();
            missing.dedup();
            return Ok(Err(ClarifyFirst::new(
                "REQUIRED_INPUT_MISSING",
                missing,
                serde_json::json!({ "stageId": stage.stage_id }),
                "ask the user to provide the missing input or connect an upstream stage",
            )));
        }

        // Step 6-7: stable ids and the embedded semantic layer (GH145).
        // Node id == stage id.
        let pos = [index as f32 * COLUMN_WIDTH, 0.0];
        nodes.insert(
            stage.stage_id.clone(),
            GraphNode {
                node_type: node_type.to_owned(),
                title: definition.title.clone(),
                params: Value::Object(params),
                pos,
                size: None,
                semantics: Some(NodeSemanticsEntry {
                    capability_id: resolved.capability_id.clone(),
                    mode: binding.mode.clone(),
                    implementation: match &stage.requested_model {
                        Some(_) => ImplementationSelection::Pinned {
                            requested_model_id: resolved.resolved_model_id.clone(),
                            binding_id: resolved.binding_id.clone(),
                        },
                        None => ImplementationSelection::Policy {
                            policy_id: "capability_default".to_owned(),
                            constraints: Value::Object(Map::new()),
                        },
                    },
                }),
            },
        );
        resolved_stages.push(ResolvedStage {
            stage_id: stage.stage_id.clone(),
            node_id: stage.stage_id.clone(),
            capability_id: resolved.capability_id.clone(),
            requested_model_id: resolved.requested_model_id.clone(),
            resolved_model_id: resolved.resolved_model_id.clone(),
            binding_id: resolved.binding_id.clone(),
            binding_revision: resolved.binding_revision.clone(),
        });
    }

    apply_layout(intent, &mut nodes, &mut layout_hints);
    for (input_id, consumer_id) in &synthesized_inputs {
        let consumer_pos = nodes
            .get(consumer_id)
            .map(|node| node.pos)
            .unwrap_or([0.0, 0.0]);
        let pos = [consumer_pos[0] - COLUMN_WIDTH, consumer_pos[1]];
        if let Some(node) = nodes.get_mut(input_id) {
            node.pos = pos;
        }
        layout_hints.push(LayoutHint {
            node_id: input_id.clone(),
            pos,
        });
    }

    let target = WorkflowGraph {
        schema_version: 1,
        catalog_revision: Some(catalog.catalog_revision.clone()),
        nodes,
        edges,
    };
    // Step 8: the assembled graph must pass full structural + semantic
    // validation before any proposal is derived from it.
    target
        .validate_semantics(service, catalog)
        .map_err(|err| CompileError::GraphInvalid {
            code: err.code(),
            message: err.to_string(),
        })?;

    Ok(Ok(BuiltGraph {
        target,
        resolved_stages,
        layout_hints,
        diagnostics,
    }))
}

fn binding_not_found_clarification(
    stage: &StageIntent,
    catalog: &CatalogSnapshot,
    capability_id: &str,
    model_id: Option<&str>,
) -> ClarifyFirst {
    let capability_name = catalog
        .capability(capability_id)
        .map(|capability| capability.display_name.clone())
        .unwrap_or_else(|| capability_id.to_owned());

    let mut available_models: Vec<serde_json::Value> = catalog
        .bindings_for_capability(capability_id)
        .into_iter()
        .filter(|binding| binding.availability == BindingAvailability::Enabled)
        .filter_map(|binding| catalog.model(&binding.model_id))
        .map(|model| {
            serde_json::json!({
                "modelId": model.model_id,
                "displayName": model.display_name,
            })
        })
        .collect();
    available_models
        .sort_by(|left, right| left["modelId"].as_str().cmp(&right["modelId"].as_str()));
    available_models.dedup_by(|left, right| left["modelId"] == right["modelId"]);

    let requested_model = model_id.and_then(|id| catalog.model(id));
    let requested_model_capabilities: Vec<serde_json::Value> = model_id
        .map(|id| {
            let mut values: Vec<serde_json::Value> = catalog
                .bindings
                .iter()
                .filter(|binding| {
                    binding.model_id == id && binding.availability == BindingAvailability::Enabled
                })
                .filter_map(|binding| catalog.capability(&binding.capability_id))
                .map(|capability| {
                    serde_json::json!({
                        "capabilityId": capability.capability_id,
                        "displayName": capability.display_name,
                    })
                })
                .collect();
            values.sort_by(|left, right| {
                left["capabilityId"]
                    .as_str()
                    .cmp(&right["capabilityId"].as_str())
            });
            values.dedup_by(|left, right| left["capabilityId"] == right["capabilityId"]);
            values
        })
        .unwrap_or_default();

    ClarifyFirst::new(
        "BINDING_NOT_FOUND",
        vec![format!("{}.model", stage.stage_id)],
        serde_json::json!({
            "stageId": stage.stage_id,
            "capabilityId": capability_id,
            "capabilityName": capability_name,
            "requestedModelId": model_id,
            "requestedModelName": requested_model.map(|model| model.display_name.as_str()),
            "availableModels": available_models,
            "requestedModelCapabilities": requested_model_capabilities,
        }),
        "请选择一个已验证支持该能力的模型，或先配置相应的 Provider binding",
    )
}

/// Resolves the node type of an upstream stage without re-running binding
/// selection side effects (pure lookup, deterministic).
fn node_type_for_stage<'a>(
    stage: &StageIntent,
    catalog: &'a CatalogSnapshot,
) -> Result<&'a str, CompileError> {
    catalog
        .capability(&stage.capability_id)
        .map(|capability| capability.node_type.as_str())
        .ok_or_else(|| CompileError::UnmappedCapability {
            capability_id: stage.capability_id.clone(),
        })
}

/// Layout is a pure projection of the semantic topology: the main chain runs
/// horizontally; explicit parallel branches stack vertically (tech.md §12).
fn apply_layout(
    intent: &IntentPlan,
    nodes: &mut BTreeMap<String, GraphNode>,
    layout_hints: &mut Vec<LayoutHint>,
) {
    let mut branch_of: BTreeMap<&str, usize> = BTreeMap::new();
    let mut depth_of: BTreeMap<&str, usize> = BTreeMap::new();
    let mut next_branch = 0usize;

    for stage in &intent.stages {
        let (branch, depth) = if stage.input_from.is_empty() {
            let branch = if intent.topology == TopologyIntent::Parallel {
                let assigned = next_branch;
                next_branch += 1;
                assigned
            } else {
                0
            };
            (branch, 0)
        } else {
            let mut branch = 0;
            let mut depth = 0;
            for input in &stage.input_from {
                if let Some(upstream_branch) = branch_of.get(input.stage_id.as_str()) {
                    branch = branch.max(*upstream_branch);
                }
                if let Some(upstream_depth) = depth_of.get(input.stage_id.as_str()) {
                    depth = depth.max(upstream_depth + 1);
                }
            }
            (branch, depth)
        };
        branch_of.insert(stage.stage_id.as_str(), branch);
        depth_of.insert(stage.stage_id.as_str(), depth);

        let pos = [depth as f32 * COLUMN_WIDTH, branch as f32 * ROW_HEIGHT];
        if let Some(node) = nodes.get_mut(&stage.stage_id) {
            node.pos = pos;
        }
        layout_hints.push(LayoutHint {
            node_id: stage.stage_id.clone(),
            pos,
        });
    }
}
