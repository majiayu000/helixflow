use helixflow_agent::AgentSessionRequest;
use helixflow_graph::WorkflowGraph;
use helixflow_registry::NodeRegistry;
use helixflow_run::{AgentRunRequest, SweepPlan, SweepVariant};
use helixflow_store::RunRecord;
use serde_json::{Number, Value};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::workbench_payload::{
    PendingConfirmationPayload, RunPayload, pending_confirmation_from_pending,
    pending_confirmation_from_sweep, run_payload_from_pending,
};

pub(crate) struct RunRequestResult {
    pub(crate) ref_id: String,
    pub(crate) message_text: String,
    pub(crate) run: RunPayload,
    pub(crate) pending_confirmation: PendingConfirmationPayload,
}

pub(crate) async fn handle_run_request(
    state: &AppState,
    request: AgentSessionRequest,
) -> Result<RunRequestResult, ApiError> {
    let label = run_label(&request.user_message);
    if is_seed_sweep_request(&request.user_message) {
        return request_seed_sweep(state, request, label).await;
    }
    let provider = selected_provider_for_workspace(state, &request.workspace_id).await?;

    let pending = state
        .runner
        .request_agent_run(AgentRunRequest {
            workspace_id: request.workspace_id,
            version_id: request.base_version_id,
            group_id: None,
            label,
            provider,
            graph: request.graph,
        })
        .await
        .map_err(ApiError::run)?;

    Ok(RunRequestResult {
        ref_id: pending.run.id.clone(),
        message_text: format!("Run {} is waiting for confirmation.", pending.run.id),
        run: run_payload_from_pending(&pending),
        pending_confirmation: pending_confirmation_from_pending(&pending),
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
    Ok(runs
        .iter()
        .find(|candidate| candidate.status == "running")
        .cloned()
        .unwrap_or_else(|| run.clone()))
}

async fn request_seed_sweep(
    state: &AppState,
    request: AgentSessionRequest,
    label: String,
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
        .request_sweep_plan(seed_plan.plan)
        .await
        .map_err(ApiError::run)?;
    let recommended = pending
        .runs
        .last()
        .ok_or_else(|| ApiError::server_error("sweep plan did not produce a recommended run"))?;

    Ok(RunRequestResult {
        ref_id: recommended.run.id.clone(),
        message_text: format!(
            "Seed sweep {} is waiting for confirmation.",
            pending.group_id
        ),
        run: run_payload_from_pending(recommended),
        pending_confirmation: pending_confirmation_from_sweep(
            &pending,
            &recommended.run.id,
            seed_plan.pending_changes,
        ),
    })
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

    #[tokio::test]
    async fn seed_sweep_request_creates_grouped_pending_confirmation_without_outputs() {
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

        assert_eq!(response.run.status, "waiting_confirmation");
        assert_eq!(response.pending_confirmation.run_count, Some(4));
        assert_eq!(response.pending_confirmation.interruptible, Some(true));
        assert_eq!(response.pending_confirmation.cost.currency, "USD");
        assert!(
            response
                .pending_confirmation
                .pending_changes
                .iter()
                .any(|change| change == "video.seed = 101")
        );

        let recommended = state
            .store
            .run(&response.pending_confirmation.id)
            .await
            .expect("recommended run");
        assert_eq!(recommended.trigger, "sweep");
        assert_eq!(recommended.status, "waiting_confirmation");
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
                .all(|run| run.status == "waiting_confirmation" && run.trigger == "sweep")
        );
        for run in group_runs {
            let ledger = state
                .store
                .cost_ledger_for_run(&run.id)
                .await
                .expect("cost ledger");
            assert!(ledger.iter().any(|entry| entry.estimated));
            let artifacts = state.store.run_artifacts(&run.id).await.expect("artifacts");
            assert!(artifacts.is_empty());
        }
    }

    async fn state_with_workspace() -> (AppState, String, String, tempfile::TempDir) {
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

    fn seed_graph() -> WorkflowGraph {
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
                    },
                ),
                (
                    "save".to_owned(),
                    GraphNode {
                        node_type: "output.save".to_owned(),
                        title: "Save".to_owned(),
                        params: json!({}),
                        pos: [240.0, 0.0],
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
