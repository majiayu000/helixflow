use std::collections::BTreeMap;
use std::path::PathBuf;

use helixflow_graph::{GraphService, ProposalOp, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_registry::catalog::{
    BindingAvailability, CapabilityBinding, CatalogSnapshot, ImplementationTarget, ModelDefinition,
    ModelLifecycle,
};
use helixflow_registry::catalog_seed::builtin_catalog;
use helixflow_registry::resolver::ConnectorAvailability;
use serde_json::{Value, json};

use super::*;

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/fixtures/gh130")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(&path).expect("read fixture"))
        .expect("fixture json")
}

fn fixture_intent(name: &str) -> IntentPlan {
    serde_json::from_value(fixture(name)["intent"].clone()).expect("intent parses")
}

fn service() -> GraphService {
    GraphService::new(NodeRegistry::builtin())
}

fn all_available(catalog: &CatalogSnapshot) -> ConnectorAvailability {
    catalog
        .connectors
        .iter()
        .map(|connector| (connector.connector_id.clone(), true))
        .collect()
}

fn empty_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
        catalog_revision: None,
    }
}

/// The T3 golden catalog: the production seed plus two bindings added as
/// pure data — GPT Image 2 for text_to_image and Seedance image_to_video.
/// Proving the fixtures compile against this catalog without any code
/// branches is the "new model = data only" acceptance criterion.
fn golden_catalog() -> CatalogSnapshot {
    let seed = builtin_catalog();
    let mut models = seed.models.clone();
    models.push(ModelDefinition {
        model_id: "openai/gpt-image-2".to_owned(),
        family_id: "gpt-image".to_owned(),
        display_name: "GPT Image 2".to_owned(),
        vendor: "openai".to_owned(),
        lifecycle: ModelLifecycle::Active,
        aliases: vec!["gpt image".to_owned(), "gpt image 2".to_owned()],
    });
    let mut bindings = seed.bindings.clone();
    let nano_t2i = seed
        .binding("google.nano-banana-2.text-to-image.atlas.v1")
        .expect("seed binding")
        .clone();
    bindings.push(CapabilityBinding {
        binding_id: "openai.gpt-image-2.text-to-image.atlas.v1".to_owned(),
        model_id: "openai/gpt-image-2".to_owned(),
        implementation: ImplementationTarget::ApiConnector {
            connector_id: "atlas".to_owned(),
            operation_id: "openai/gpt-image-2/text-to-image".to_owned(),
        },
        ..nano_t2i.clone()
    });
    let seedance_t2v = seed
        .binding("bytedance.seedance-v1-5-pro.text-to-video.atlas.v1")
        .expect("seed binding")
        .clone();
    bindings.push(CapabilityBinding {
        binding_id: "bytedance.seedance-v1-5-pro.image-to-video.atlas.v1".to_owned(),
        capability_id: "image_to_video".to_owned(),
        mode: "image_to_video".to_owned(),
        implementation: ImplementationTarget::ApiConnector {
            connector_id: "atlas".to_owned(),
            operation_id: "bytedance/seedance-v1.5-pro/image-to-video".to_owned(),
        },
        input_schema: helixflow_registry::ParamsSchema {
            required: vec!["duration_sec".to_owned()],
            properties: BTreeMap::from([
                ("prompt".to_owned(), helixflow_registry::ParamSpec::string()),
                (
                    "duration_sec".to_owned(),
                    helixflow_registry::ParamSpec::integer_range(1, 10),
                ),
            ]),
            allow_unknown: false,
        },
        availability: BindingAvailability::Enabled,
        ..seedance_t2v
    });
    let mut defaults = seed.default_bindings.clone();
    defaults.insert(
        "image_to_video".to_owned(),
        "bytedance.seedance-v1-5-pro.image-to-video.atlas.v1".to_owned(),
    );
    CatalogSnapshot::build(
        seed.capabilities.clone(),
        models,
        bindings,
        seed.connectors.clone(),
        seed.workflow_backends.clone(),
        defaults,
    )
}

fn compile_fixture(name: &str, catalog: &CatalogSnapshot) -> Result<CompileOutcome, CompileError> {
    let intent = fixture_intent(name);
    compile(
        &intent,
        &empty_graph(),
        &service(),
        catalog,
        &all_available(catalog),
    )
}

fn expect_compiled(outcome: Result<CompileOutcome, CompileError>) -> CompiledProposal {
    match outcome.expect("compile succeeds") {
        CompileOutcome::Compiled(proposal) => proposal,
        CompileOutcome::Clarify(clarify) => panic!("unexpected clarify: {clarify:?}"),
    }
}

#[test]
fn golden_nano_banana_seedance_compiles_to_single_chain() {
    let catalog = golden_catalog();
    let proposal = expect_compiled(compile_fixture(
        "intent-nano-banana-seedance.json",
        &catalog,
    ));

    assert_eq!(proposal.graph_schema_version, 1);
    assert_eq!(proposal.target.nodes.len(), 2);
    assert_eq!(proposal.target.edges.len(), 1);
    let edge = &proposal.target.edges[0];
    assert_eq!(edge.from, ["s1".to_owned(), "image".to_owned()]);
    assert_eq!(edge.to, ["s2".to_owned(), "image".to_owned()]);

    let expected: BTreeMap<&str, &str> = BTreeMap::from([
        ("s1", "google/nano-banana-2"),
        ("s2", "bytedance/seedance-v1.5-pro"),
    ]);
    for stage in &proposal.resolved_stages {
        assert_eq!(
            expected.get(stage.stage_id.as_str()).copied(),
            Some(stage.resolved_model_id.as_str()),
            "stage {}",
            stage.stage_id
        );
        assert_eq!(
            stage.requested_model_id.as_deref(),
            Some(stage.resolved_model_id.as_str()),
            "P4: requested must equal resolved"
        );
    }
}

#[test]
fn golden_seed_catalog_without_i2v_binding_fails_closed() {
    // Fixture note: with no (image_to_video, seedance) binding the compiler
    // must fail with BINDING_NOT_FOUND — never fall back to text_to_video.
    let catalog = builtin_catalog();
    let err = compile_fixture("intent-nano-banana-seedance.json", &catalog)
        .expect_err("no i2v binding in seed");
    assert_eq!(err.code(), "BINDING_NOT_FOUND");
}

#[test]
fn golden_gpt_image_swap_changes_binding_only() {
    let catalog = golden_catalog();
    let nano = expect_compiled(compile_fixture(
        "intent-nano-banana-seedance.json",
        &catalog,
    ));
    let gpt = expect_compiled(compile_fixture("intent-gpt-image-seedance.json", &catalog));

    // Same edges, same node types — only the first stage's model/binding
    // differs. No code branch was added for the swap.
    assert_eq!(nano.target.edges, gpt.target.edges);
    assert_eq!(
        nano.target.nodes["s1"].node_type,
        gpt.target.nodes["s1"].node_type
    );
    let gpt_s1 = gpt
        .resolved_stages
        .iter()
        .find(|stage| stage.stage_id == "s1")
        .expect("s1");
    assert_eq!(gpt_s1.resolved_model_id, "openai/gpt-image-2");
    assert_eq!(
        gpt_s1.binding_id,
        "openai.gpt-image-2.text-to-image.atlas.v1"
    );
}

#[test]
fn golden_serial_chain_compiles_without_fanout() {
    let catalog = golden_catalog();
    let proposal = expect_compiled(compile_fixture("intent-serial-chain.json", &catalog));

    // 3 capability stages + 1 synthesized input.text feeding the writer.
    assert_eq!(proposal.target.nodes.len(), 4);
    assert_eq!(proposal.target.edges.len(), 3);
    assert!(proposal.target.nodes.contains_key("s1-text-input"));
    assert_eq!(proposal.target.collected_semantics().len(), 3);
    // A chain: each node has at most one inbound and one outbound edge.
    for node_id in proposal.target.nodes.keys() {
        let inbound = proposal
            .target
            .edges
            .iter()
            .filter(|edge| &edge.to[0] == node_id)
            .count();
        let outbound = proposal
            .target
            .edges
            .iter()
            .filter(|edge| &edge.from[0] == node_id)
            .count();
        assert!(inbound <= 1 && outbound <= 1, "fan detected at {node_id}");
    }
}

#[test]
fn golden_explicit_parallel_compiles_two_branches() {
    let catalog = golden_catalog();
    let proposal = expect_compiled(compile_fixture("intent-explicit-parallel.json", &catalog));

    assert_eq!(proposal.target.nodes.len(), 4);
    assert_eq!(proposal.target.edges.len(), 2);
    // Branches land on distinct rows.
    let y_a = proposal.target.nodes["a1"].pos[1];
    let y_b = proposal.target.nodes["b1"].pos[1];
    assert_ne!(y_a, y_b);
}

#[test]
fn golden_missing_image_input_clarifies() {
    let catalog = golden_catalog();
    match compile_fixture("intent-missing-image-input.json", &catalog).expect("outcome") {
        CompileOutcome::Clarify(clarify) => {
            assert_eq!(clarify.route, "clarify_first");
            assert_eq!(clarify.reason_code, "REQUIRED_INPUT_MISSING");
            assert!(clarify.missing_fields.contains(&"s1.image".to_owned()));
        }
        CompileOutcome::Compiled(_) => panic!("must clarify, not compile"),
    }
}

#[test]
fn golden_model_capability_mismatch_is_hard_error() {
    let catalog = golden_catalog();
    let err = compile_fixture("intent-model-mismatch.json", &catalog)
        .expect_err("seedance cannot do text_to_image");
    assert_eq!(err.code(), "BINDING_NOT_FOUND");
}

#[test]
fn identical_inputs_produce_identical_proposals() {
    let catalog = golden_catalog();
    let first = expect_compiled(compile_fixture(
        "intent-nano-banana-seedance.json",
        &catalog,
    ));
    let second = expect_compiled(compile_fixture(
        "intent-nano-banana-seedance.json",
        &catalog,
    ));
    assert_eq!(
        serde_json::to_string(&first).expect("serialize"),
        serde_json::to_string(&second).expect("serialize")
    );
}

#[test]
fn diff_against_matching_graph_is_empty_and_param_change_is_minimal() {
    let catalog = golden_catalog();
    let proposal = expect_compiled(compile_fixture(
        "intent-nano-banana-seedance.json",
        &catalog,
    ));

    // Converged: recompiling against the compiled base yields zero ops.
    let intent = fixture_intent("intent-nano-banana-seedance.json");
    let converged = expect_compiled(compile(
        &intent,
        &proposal.target,
        &service(),
        &catalog,
        &all_available(&catalog),
    ));
    assert!(converged.ops.is_empty(), "ops: {:?}", converged.ops);

    // A single param change produces exactly one SetParam with prev.
    let mut intent_changed = intent.clone();
    intent_changed.stages[0]
        .params
        .as_object_mut()
        .expect("params")
        .insert("prompt".to_owned(), json!("a different product"));
    let changed = expect_compiled(compile(
        &intent_changed,
        &proposal.target,
        &service(),
        &catalog,
        &all_available(&catalog),
    ));
    assert_eq!(changed.ops.len(), 1);
    match &changed.ops[0] {
        ProposalOp::SetParam {
            id,
            key,
            prev,
            value,
            ..
        } => {
            assert_eq!(id, "s1");
            assert_eq!(key, "prompt");
            assert!(prev.is_some());
            assert_eq!(value, &json!("a different product"));
        }
        other => panic!("expected SetParam, got {other:?}"),
    }
}

#[test]
fn linear_intent_with_fanout_reference_is_rejected() {
    let catalog = golden_catalog();
    let mut intent = fixture_intent("intent-serial-chain.json");
    // s3 now skips s2 and consumes s1 directly: no longer a single chain.
    intent.stages[2].input_from[0].stage_id = "s1".to_owned();
    intent.stages[2].input_from[0].output = "prompt".to_owned();

    let err = compile(
        &intent,
        &empty_graph(),
        &service(),
        &catalog,
        &all_available(&catalog),
    )
    .expect_err("fan-out under linear");
    assert_eq!(err.code(), "TOPOLOGY_CONFLICT");
}

#[test]
fn port_type_mismatch_is_rejected() {
    let catalog = golden_catalog();
    let mut intent = fixture_intent("intent-nano-banana-seedance.json");
    // Wire the image output into a video-typed consumer port name that the
    // target capability does not expose.
    intent.stages[1].input_from[0].output = "video".to_owned();

    let err = compile(
        &intent,
        &empty_graph(),
        &service(),
        &catalog,
        &all_available(&catalog),
    )
    .expect_err("bad port");
    assert_eq!(err.code(), "PORT_TYPE_MISMATCH");
}

#[test]
fn compiled_target_roundtrips_and_validates_semantics() {
    let catalog = golden_catalog();
    let proposal = expect_compiled(compile_fixture("intent-serial-chain.json", &catalog));
    let roundtrip: WorkflowGraph =
        serde_json::from_value(serde_json::to_value(&proposal.target).expect("serialize"))
            .expect("roundtrip");
    roundtrip
        .validate_semantics(&service(), &catalog)
        .expect("compiled graph passes semantic validation");
}

#[test]
fn ambiguous_connector_availability_routes_to_clarify() {
    let catalog = golden_catalog();
    let intent = fixture_intent("intent-nano-banana-seedance.json");
    // No connector is healthy: recoverable resolver failure → clarify.
    let availability = ConnectorAvailability::new();
    match compile(&intent, &empty_graph(), &service(), &catalog, &availability).expect("outcome") {
        CompileOutcome::Clarify(clarify) => {
            assert_eq!(clarify.reason_code, "BINDING_UNAVAILABLE");
        }
        CompileOutcome::Compiled(_) => panic!("must clarify when nothing is available"),
    }
}

#[test]
fn model_swap_on_kept_node_emits_set_semantics() {
    let catalog = golden_catalog();
    let nano = expect_compiled(compile_fixture(
        "intent-nano-banana-seedance.json",
        &catalog,
    ));

    // Recompile the GPT variant against the applied nano graph: s1 keeps its
    // node type, so the binding change must arrive as an explicit
    // SetSemantics op instead of leaving the stale entry behind (GH145).
    let gpt_intent = fixture_intent("intent-gpt-image-seedance.json");
    let gpt = expect_compiled(compile(
        &gpt_intent,
        &nano.target,
        &service(),
        &catalog,
        &all_available(&catalog),
    ));

    let semantics_ops: Vec<_> = gpt
        .ops
        .iter()
        .filter_map(|op| match op {
            ProposalOp::SetSemantics { id, semantics } => Some((id.as_str(), semantics)),
            _ => None,
        })
        .collect();
    assert_eq!(semantics_ops.len(), 1, "ops: {:?}", gpt.ops);
    let (id, semantics) = &semantics_ops[0];
    assert_eq!(*id, "s1");
    let entry = semantics.as_ref().expect("entry set, not cleared");
    match &entry.implementation {
        helixflow_registry::catalog::ImplementationSelection::Pinned {
            requested_model_id,
            binding_id,
        } => {
            assert_eq!(requested_model_id, "openai/gpt-image-2");
            assert_eq!(binding_id, "openai.gpt-image-2.text-to-image.atlas.v1");
        }
        other => panic!("expected pinned selection, got {other:?}"),
    }

    // Applying the ops converges the embedded semantics to the new target.
    let applied = service()
        .apply_ops(&nano.target, &gpt.ops)
        .expect("apply ops");
    assert_eq!(
        applied.collected_semantics(),
        gpt.target.collected_semantics()
    );
}
