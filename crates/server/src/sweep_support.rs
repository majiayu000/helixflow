use helixflow_agent::AgentSessionRequest;
use helixflow_graph::WorkflowGraph;
use helixflow_registry::NodeRegistry;
use helixflow_run::{AgentRunRequest, CostSummary, SweepPlan, SweepVariant};
use helixflow_store::{MessageRecord, NewMessage, RunRecord};
use serde_json::{Number, Value};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::workbench_payload::{
    PendingConfirmationPayload, RunPayload, pending_confirmation_from_pending,
    pending_confirmation_from_sweep, run_payload_from_outcome, run_payload_from_pending,
};

pub(crate) struct RunRequestResult {
    pub(crate) message: MessageRecord,
    pub(crate) run: RunPayload,
    pub(crate) pending_confirmation: Option<PendingConfirmationPayload>,
}

pub(crate) async fn handle_run_request(
    state: &AppState,
    request: AgentSessionRequest,
) -> Result<RunRequestResult, ApiError> {
    let raw_threshold = run_confirmation_threshold_config()?;
    handle_run_request_with_threshold_config(state, request, raw_threshold.as_deref()).await
}

async fn handle_run_request_with_threshold_config(
    state: &AppState,
    request: AgentSessionRequest,
    raw_threshold: Option<&str>,
) -> Result<RunRequestResult, ApiError> {
    let confirmation_threshold_usd =
        parse_run_confirmation_threshold_usd(raw_threshold).map_err(ApiError::server_error)?;
    let label = run_label(&request.user_message);
    if is_seed_sweep_request(&request.user_message) {
        return request_seed_sweep(state, request, label, confirmation_threshold_usd).await;
    }
    let provider = selected_provider_for_workspace(state, &request.workspace_id).await?;

    let pending = state
        .runner
        .prepare_agent_run(AgentRunRequest {
            workspace_id: request.workspace_id,
            version_id: request.base_version_id,
            group_id: None,
            label,
            provider,
            graph: request.graph,
        })
        .await
        .map_err(ApiError::run)?;
    let requires_confirmation =
        requires_run_confirmation(&pending.estimate, confirmation_threshold_usd);
    let message_text = if requires_confirmation {
        format!(
            "Run {} is waiting for confirmation; estimated cost is {}.",
            pending.run.id,
            format_cost(&pending.estimate)
        )
    } else {
        format!(
            "Run {} passed automatic cost approval at {}; execution is starting.",
            pending.run.id,
            format_cost(&pending.estimate)
        )
    };
    let message = persist_run_request_message(
        state,
        &pending.run.workspace_id,
        &pending.run.id,
        &message_text,
    )
    .await?;
    state
        .runner
        .announce_run_requested(&pending, requires_confirmation)
        .await
        .map_err(ApiError::run)?;
    if !requires_confirmation {
        let outcome = state
            .runner
            .start_confirmed_run(&pending.run.id)
            .await
            .map_err(ApiError::run)?;
        let costs = state
            .store
            .cost_ledger_for_run(&outcome.run.id)
            .await
            .map_err(ApiError::store)?;

        return Ok(RunRequestResult {
            message,
            run: run_payload_from_outcome(&outcome, &costs),
            pending_confirmation: None,
        });
    }

    Ok(RunRequestResult {
        message,
        run: run_payload_from_pending(&pending),
        pending_confirmation: Some(pending_confirmation_from_pending(&pending)),
    })
}

pub(crate) async fn interrupt_target_run(
    state: &AppState,
    run: &RunRecord,
) -> Result<RunRecord, ApiError> {
    if run.trigger != "sweep" {
        return Ok(run.clone());
    }
    let Some(group_id) = run.group_id.as_deref() else {
        return Ok(run.clone());
    };
    let runs = state
        .store
        .runs_for_group(group_id)
        .await
        .map_err(ApiError::store)?;
    if let Some(running) = runs
        .iter()
        .find(|candidate| candidate.status == "running")
        .cloned()
    {
        return Ok(running);
    }
    for candidate in runs.iter().filter(|candidate| candidate.status == "queued") {
        if state.runner.has_interrupt(&candidate.id).await {
            return Ok(candidate.clone());
        }
    }
    Ok(run.clone())
}

async fn request_seed_sweep(
    state: &AppState,
    request: AgentSessionRequest,
    label: String,
    confirmation_threshold_usd: f64,
) -> Result<RunRequestResult, ApiError> {
    let count = seed_sweep_run_count(&request.user_message);
    let provider = selected_provider_for_workspace(state, &request.workspace_id).await?;
    let seed_plan = build_seed_sweep_plan(
        request.workspace_id,
        request.base_version_id,
        label,
        provider,
        request.graph,
        count,
    )?;
    let pending = state
        .runner
        .prepare_sweep_plan(seed_plan.plan)
        .await
        .map_err(ApiError::run)?;
    let recommended = pending
        .runs
        .last()
        .ok_or_else(|| ApiError::server_error("sweep plan did not produce a recommended run"))?;
    let requires_confirmation =
        requires_run_confirmation(&pending.estimate, confirmation_threshold_usd);
    let message_text = if requires_confirmation {
        format!(
            "Seed sweep {} is waiting for confirmation; estimated cost is {}.",
            pending.group_id,
            format_cost(&pending.estimate)
        )
    } else {
        format!(
            "Seed sweep {} passed automatic cost approval at {}; execution is starting.",
            pending.group_id,
            format_cost(&pending.estimate)
        )
    };
    let message = persist_run_request_message(
        state,
        &recommended.run.workspace_id,
        &recommended.run.id,
        &message_text,
    )
    .await?;
    for run in &pending.runs {
        state
            .runner
            .announce_run_requested(run, requires_confirmation)
            .await
            .map_err(ApiError::run)?;
    }
    if !requires_confirmation {
        let run_ids = pending
            .runs
            .iter()
            .map(|run| run.run.id.clone())
            .collect::<Vec<_>>();
        let outcome = state
            .runner
            .start_confirmed_sweep(&run_ids, &recommended.run.id)
            .await
            .map_err(ApiError::run)?;
        let response_run = outcome
            .runs
            .iter()
            .find(|candidate| candidate.run.id == recommended.run.id)
            .or_else(|| outcome.runs.last())
            .ok_or_else(|| ApiError::server_error("started sweep omitted recommended run"))?;
        let costs = state
            .store
            .cost_ledger_for_run(&response_run.run.id)
            .await
            .map_err(ApiError::store)?;

        return Ok(RunRequestResult {
            message,
            run: run_payload_from_outcome(response_run, &costs),
            pending_confirmation: None,
        });
    }

    Ok(RunRequestResult {
        message,
        run: run_payload_from_pending(recommended),
        pending_confirmation: Some(pending_confirmation_from_sweep(
            &pending,
            &recommended.run.id,
            seed_plan.pending_changes,
        )),
    })
}

async fn persist_run_request_message(
    state: &AppState,
    workspace_id: &str,
    run_id: &str,
    text: &str,
) -> Result<MessageRecord, ApiError> {
    state
        .store
        .create_message(NewMessage {
            workspace_id,
            role: "agent",
            kind: "run_requested",
            text: Some(text),
            ref_id: Some(run_id),
            attachment_ids_json: None,
        })
        .await
        .map_err(ApiError::store)
}

pub(crate) fn requires_run_confirmation(
    cost: &CostSummary,
    confirmation_threshold_usd: f64,
) -> bool {
    if !cost.amount.is_finite() || cost.amount < 0.0 {
        return true;
    }
    if cost.currency != "USD" {
        return cost.amount > 0.0;
    }
    cost.amount > confirmation_threshold_usd
}

pub(crate) fn run_confirmation_threshold_config() -> Result<Option<String>, ApiError> {
    match std::env::var("HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD") {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(ApiError::server_error(
            "HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD must be valid UTF-8",
        )),
    }
}

pub(crate) fn parse_run_confirmation_threshold_usd(raw: Option<&str>) -> Result<f64, String> {
    let Some(raw) = raw else {
        return Ok(0.0);
    };
    let value = raw.parse::<f64>().map_err(|_| {
        "HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD must be a finite non-negative number"
            .to_owned()
    })?;
    if !value.is_finite() || value < 0.0 {
        return Err(
            "HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD must be a finite non-negative number"
                .to_owned(),
        );
    }
    Ok(value)
}
fn format_cost(cost: &CostSummary) -> String {
    format!("{} {}", format_cost_amount(cost.amount), cost.currency)
}

fn format_cost_amount(amount: f64) -> String {
    if amount == 0.0 || amount.abs() >= 0.01 {
        return format!("{amount:.2}");
    }
    if amount.abs() < 0.000001 {
        return format!("{amount:.2e}");
    }
    format!("{amount:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

struct SeedSweepPlan {
    plan: SweepPlan,
    pending_changes: Vec<String>,
}

fn build_seed_sweep_plan(
    workspace_id: String,
    version_id: String,
    label: String,
    provider: String,
    graph: WorkflowGraph,
    count: usize,
) -> Result<SeedSweepPlan, ApiError> {
    let seed_nodes = seed_capable_node_ids(&graph)?;
    if seed_nodes.is_empty() {
        return Err(ApiError::bad_request(
            "current graph has no seed-capable provider node",
        ));
    }

    let seeds = [101_i64, 202, 303, 404, 505, 606];
    let mut variants = Vec::with_capacity(count);
    let mut pending_changes = Vec::with_capacity(count * seed_nodes.len());

    for seed in seeds.into_iter().take(count) {
        let mut variant_graph = graph.clone();
        for node_id in &seed_nodes {
            let node = variant_graph.nodes.get_mut(node_id).ok_or_else(|| {
                ApiError::server_error(format!("seed node `{node_id}` disappeared"))
            })?;
            let params = node.params.as_object_mut().ok_or_else(|| {
                ApiError::bad_request(format!("node `{node_id}` params must be an object"))
            })?;
            params.insert("seed".to_owned(), Value::Number(Number::from(seed)));
            pending_changes.push(format!("{node_id}.seed = {seed}"));
        }
        variants.push(SweepVariant {
            label: format!("seed {seed}"),
            graph: variant_graph,
        });
    }

    Ok(SeedSweepPlan {
        plan: SweepPlan {
            workspace_id,
            version_id,
            label,
            provider,
            variants,
        },
        pending_changes,
    })
}

fn seed_capable_node_ids(graph: &WorkflowGraph) -> Result<Vec<String>, ApiError> {
    let registry = NodeRegistry::builtin();
    let mut node_ids = Vec::new();
    for (node_id, node) in &graph.nodes {
        let definition = registry
            .definition(&node.node_type)
            .map_err(|err| ApiError::bad_request(err.to_string()))?;
        if definition.params_schema.properties.contains_key("seed")
            && definition.capability.is_some()
        {
            node_ids.push(node_id.clone());
        }
    }
    Ok(node_ids)
}

async fn selected_provider_for_workspace(
    state: &AppState,
    workspace_id: &str,
) -> Result<String, ApiError> {
    let workspace = state
        .store
        .workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let provider = state.selected_provider_for_workspace(&workspace);
    if !state.provider_registry.provider_enabled(&provider) {
        return Err(ApiError::conflict(format!(
            "runtime provider `{provider}` is unavailable"
        )));
    }
    Ok(provider)
}

fn is_seed_sweep_request(user_message: &str) -> bool {
    let normalized = user_message.to_ascii_lowercase();
    normalized.contains("seed") || user_message.contains("种子")
}

fn seed_sweep_run_count(user_message: &str) -> usize {
    user_message
        .split(|ch: char| !ch.is_ascii_digit())
        .find_map(|part| {
            if part.is_empty() {
                None
            } else {
                part.parse::<usize>().ok()
            }
        })
        .map(|count| count.clamp(2, 6))
        .unwrap_or(4)
}

fn run_label(user_message: &str) -> String {
    let trimmed = user_message.trim();
    if trimmed.is_empty() {
        return "Agent requested run".to_owned();
    }
    trimmed.chars().take(80).collect()
}

#[cfg(test)]
mod gh102_tests;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use helixflow_agent::{
        AgentError, AgentLogEntry, AgentSessionRequest, TurnMode, ValidatedAgentProposal,
        ValidatedAgentReply,
    };
    use helixflow_gateway::RuntimeProvider;
    use helixflow_graph::{GraphEdge, GraphNode, PreparedProposal, WorkflowGraph};
    use helixflow_run::EventBus;
    use helixflow_store::{NewVersion, Store, VersionSource};
    use serde_json::json;

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};

    #[test]
    fn confirmation_threshold_parser_is_conservative_and_rejects_invalid_config() {
        assert_eq!(parse_run_confirmation_threshold_usd(None), Ok(0.0));
        assert_eq!(parse_run_confirmation_threshold_usd(Some("1.25")), Ok(1.25));
        for invalid in ["", "abc", "-0.01", "NaN", "inf"] {
            assert!(
                parse_run_confirmation_threshold_usd(Some(invalid)).is_err(),
                "invalid threshold `{invalid}` must fail"
            );
        }
    }

    #[test]
    fn confirmation_policy_covers_threshold_currency_and_invalid_amounts() {
        let usd = |amount| CostSummary {
            amount,
            currency: "USD".to_owned(),
            estimated: true,
        };
        assert!(!requires_run_confirmation(&usd(0.99), 1.0));
        assert!(!requires_run_confirmation(&usd(1.0), 1.0));
        assert!(requires_run_confirmation(&usd(1.01), 1.0));
        assert!(requires_run_confirmation(&usd(f64::NAN), 1.0));
        assert!(requires_run_confirmation(&usd(-0.01), 1.0));
        assert!(requires_run_confirmation(
            &CostSummary {
                amount: 0.01,
                currency: "EUR".to_owned(),
                estimated: true,
            },
            100.0
        ));
        assert_eq!(format_cost_amount(0.0), "0.00");
        assert_eq!(format_cost_amount(0.0012), "0.0012");
        assert_eq!(format_cost_amount(0.0000001), "1.00e-7");
        assert_eq!(format_cost_amount(1.2), "1.20");
    }

    #[tokio::test]
    async fn seed_sweep_request_auto_starts_free_group() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;

        let response = handle_run_request(
            &state,
            AgentSessionRequest {
                workspace_id,
                base_version_id: version_id,
                user_message: "为当前工作流设计 4 个不同 seed 的真实运行计划".to_owned(),
                graph: seed_graph(),
                provider_catalog: RuntimeProvider::mock().catalog_snapshot(),
                run_context: None,
                sessions_dir: state.agent_sessions_dir.clone(),
                mode: TurnMode::RunRequest,
                skill: TurnMode::RunRequest.agent_skill(),
                canvas_context: None,
            },
        )
        .await
        .expect("seed sweep response");

        assert_eq!(response.run.status, "queued");
        assert_eq!(response.pending_confirmation, None);
        assert!(
            response
                .message
                .text
                .as_deref()
                .is_some_and(|text| text.contains("automatic cost approval"))
        );

        let recommended = state
            .store
            .run(&response.run.id)
            .await
            .expect("recommended run");
        assert_eq!(recommended.trigger, "sweep");
        let group_id = recommended.group_id.clone().expect("group id");
        let group_runs = state
            .store
            .runs_for_group(&group_id)
            .await
            .expect("group runs");
        assert_eq!(group_runs.len(), 4);
        assert!(
            group_runs
                .iter()
                .all(|run| run.status != "waiting_confirmation" && run.trigger == "sweep")
        );
        for run in group_runs {
            let ledger = state
                .store
                .cost_ledger_for_run(&run.id)
                .await
                .expect("cost ledger");
            assert!(ledger.iter().any(|entry| entry.estimated));
            let events = state.store.run_events(&run.id).await.expect("run events");
            let requested = events
                .iter()
                .find(|event| event.ev == "run.requested")
                .expect("run.requested event");
            let data: Value =
                serde_json::from_str(&requested.data_json).expect("requested event data");
            assert_eq!(data["requires_confirmation"], false);
        }
    }

    pub(super) async fn state_with_workspace() -> (AppState, String, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Sweep workspace")
            .await
            .expect("create workspace");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Sweep graph",
                source: VersionSource::Manual,
                graph_path: "graphs/sweep.json",
                graph_hash: "sha256:sweep",
                parent_id: None,
            })
            .await
            .expect("create version");
        let state = AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(NoopWorkbenchAgent),
            data_dir.join("sessions"),
        );
        (state, workspace.id, version.id, dir)
    }

    pub(super) fn seed_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::from([
                (
                    "video".to_owned(),
                    GraphNode {
                        node_type: "video.text_to_video".to_owned(),
                        title: "Video render".to_owned(),
                        params: json!({
                            "prompt": "clean product shot",
                            "duration_sec": 4,
                            "aspect_ratio": "9:16"
                        }),
                        pos: [0.0, 0.0],
                        size: None,
                    },
                ),
                (
                    "save".to_owned(),
                    GraphNode {
                        node_type: "output.save".to_owned(),
                        title: "Save".to_owned(),
                        params: json!({}),
                        pos: [240.0, 0.0],
                        size: None,
                    },
                ),
            ]),
            edges: vec![GraphEdge {
                from: ["video".to_owned(), "video".to_owned()],
                to: ["save".to_owned(), "artifact".to_owned()],
                edge_type: "artifact".to_owned(),
            }],
        }
    }

    struct NoopWorkbenchAgent;

    #[async_trait]
    impl WorkbenchAgent for NoopWorkbenchAgent {
        async fn answer_chat(
            &self,
            _request: AgentSessionRequest,
        ) -> Result<ValidatedAgentReply, AgentError> {
            Err(AgentError::Runtime("noop agent".to_owned()))
        }

        async fn propose_graph_change(
            &self,
            request: AgentSessionRequest,
        ) -> Result<ValidatedAgentProposal, AgentError> {
            Ok(ValidatedAgentProposal {
                session_id: "noop".to_owned(),
                agent_logs: vec![AgentLogEntry {
                    kind: "agent_log:status".to_owned(),
                    text: "noop".to_owned(),
                }],
                proposal: PreparedProposal {
                    base_version_id: request.base_version_id,
                    kind: helixflow_graph::ProposalKind::Modify,
                    title: "noop".to_owned(),
                    summary: "noop".to_owned(),
                    ops: Vec::new(),
                    diff_summary: Vec::new(),
                    preview_graph: request.graph,
                    state: helixflow_graph::ProposalState::Pending,
                    message_id: None,
                },
            })
        }
    }
}
