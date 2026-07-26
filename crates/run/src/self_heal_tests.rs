use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use helixflow_gateway::{
    CostEstimate, MockProvider, Provider, ProviderCatalog, ProviderError, ProviderHealth,
    ProviderRequest, ProviderResult, ProviderResultValue, ProviderTaskHandle,
};
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_store::{NewRun, NewVersion, RunRecord, Store, VersionSource};
use serde_json::json;

use super::{AgentRunRequest, RunService, SweepPlan, SweepVariant};

const INVALID_RETRY_ENV_CHILD: &str = "HELIXFLOW_TEST_INVALID_RETRY_ENV_CHILD";

#[tokio::test]
async fn failed_paid_run_derives_retry_waiting_for_confirmation() {
    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let service = RunService::with_provider(store.clone(), CostedFailingProvider::paid());
    let mut events = service.events().subscribe();
    let pending = service
        .request_agent_run(AgentRunRequest {
            workspace_id: workspace_id.clone(),
            version_id,
            group_id: None,
            label: "Paid failure".to_owned(),
            provider: "mock".to_owned(),
            graph: self_heal_graph("writer-paid"),
        })
        .await
        .expect("prepare paid run");
    assert!(pending.estimate.amount > 0.0);

    service
        .start_confirmed_run(&pending.run.id)
        .await
        .expect("start confirmed parent");
    let retry = wait_for_retry(&store, &workspace_id).await;
    assert_eq!(retry.status, "waiting_confirmation");
    let ledger = store
        .cost_ledger_for_run(&retry.id)
        .await
        .expect("retry estimate ledger");
    assert!(
        ledger
            .iter()
            .any(|entry| entry.estimated && entry.amount > 0.0)
    );

    let saw_pending = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("retry event");
            if event.run_id == retry.id && event.ev == "run.retry_pending" {
                return true;
            }
        }
    })
    .await
    .expect("retry pending event timeout");
    assert!(saw_pending);
}

#[tokio::test]
async fn failed_recommended_sweep_run_derives_detached_retry() {
    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let service = RunService::with_provider(
        store.clone(),
        CostedFailingProvider::free_for("writer-recommended"),
    );
    let pending = service
        .request_sweep_plan(SweepPlan {
            workspace_id: workspace_id.clone(),
            version_id,
            label: "Failing sweep".to_owned(),
            provider: "mock".to_owned(),
            variants: vec![
                SweepVariant {
                    label: "baseline".to_owned(),
                    graph: self_heal_graph("writer-baseline"),
                },
                SweepVariant {
                    label: "recommended".to_owned(),
                    graph: self_heal_graph("writer-recommended"),
                },
            ],
        })
        .await
        .expect("prepare sweep");
    let parent_id = pending.runs[1].run.id.clone();
    let run_ids = pending
        .runs
        .iter()
        .map(|pending| pending.run.id.clone())
        .collect::<Vec<_>>();
    service
        .start_confirmed_sweep(&run_ids, &parent_id)
        .await
        .expect("start sweep");

    let retry = wait_for_retry(&store, &workspace_id).await;
    assert_eq!(retry.parent_run_id.as_deref(), Some(parent_id.as_str()));
    assert!(retry.group_id.is_none());
    assert_eq!(retry.trigger, "agent");
    wait_for_self_heal_status(&store, &retry.id, "failed").await;
}

#[tokio::test]
async fn retry_infrastructure_failure_emits_persisted_error_event() {
    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let service = RunService::new(store.clone());
    let mut events = service.events().subscribe();
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &version_id,
            group_id: None,
            label: "Broken retry metadata",
            trigger: "agent",
            plan_json: None,
            estimate_json: Some("{invalid-json"),
            status: "failed",
        })
        .await
        .expect("failed run");

    service
        .continue_self_heal_from_failed(&run)
        .await
        .expect_err("invalid retry estimate must fail visibly");
    let event = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("event");
            if event.run_id == run.id && event.ev == "run.retry_failed" {
                return event;
            }
        }
    })
    .await
    .expect("retry_failed event timeout");
    assert!(
        event.data["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    assert!(
        store
            .run_events(&run.id)
            .await
            .expect("persisted events")
            .iter()
            .any(|event| event.ev == "run.retry_failed")
    );
}

#[tokio::test]
async fn invalid_retry_environment_fails_before_creating_child_run() {
    if std::env::var_os(INVALID_RETRY_ENV_CHILD).is_none() {
        let mut command = std::process::Command::new(
            std::env::current_exe().expect("locate helixflow-run test executable"),
        );
        command
            .arg("--exact")
            .arg("self_heal_tests::invalid_retry_environment_fails_before_creating_child_run")
            .arg("--nocapture")
            .env(INVALID_RETRY_ENV_CHILD, "1");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            command.env(
                "HELIXFLOW_RUN_MAX_RETRIES",
                std::ffi::OsString::from_vec(vec![0xff]),
            );
        }
        #[cfg(not(unix))]
        command.env("HELIXFLOW_RUN_MAX_RETRIES", "not-a-number");

        let output = command.output().expect("run isolated retry config test");
        assert!(
            output.status.success(),
            "isolated retry config test failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let service = RunService::new(store.clone());
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &version_id,
            group_id: None,
            label: "Invalid retry config",
            trigger: "agent",
            plan_json: None,
            estimate_json: Some(r#"{"amount":0.0,"currency":"USD","lines":[]}"#),
            status: "failed",
        })
        .await
        .expect("failed parent run");

    let error = service
        .continue_self_heal_from_failed(&run)
        .await
        .expect_err("invalid retry configuration must fail closed");
    assert!(error.to_string().contains("HELIXFLOW_RUN_MAX_RETRIES"));

    let latest = store
        .latest_workspace_run(&workspace_id)
        .await
        .expect("latest run")
        .expect("parent run remains latest");
    assert_eq!(
        latest.id, run.id,
        "invalid config must not create a child run"
    );
    assert!(
        store
            .run_events(&run.id)
            .await
            .expect("persisted retry events")
            .iter()
            .any(|event| event.ev == "run.retry_failed"),
        "invalid config must be visible as a persisted retry failure"
    );
}

#[derive(Clone)]
struct CostedFailingProvider {
    estimate_amount: f64,
    fail_node_id: Option<String>,
}

impl CostedFailingProvider {
    fn paid() -> Self {
        Self {
            estimate_amount: 1.0,
            fail_node_id: None,
        }
    }

    fn free_for(node_id: &str) -> Self {
        Self {
            estimate_amount: 0.0,
            fail_node_id: Some(node_id.to_owned()),
        }
    }
}

#[async_trait]
impl Provider for CostedFailingProvider {
    fn id(&self) -> &str {
        "mock"
    }

    async fn health(&self) -> ProviderHealth {
        ProviderHealth {
            ok: true,
            message: None,
        }
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        MockProvider::new().catalog().await
    }

    async fn estimate(&self, _req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        Ok(CostEstimate {
            amount: self.estimate_amount,
            currency: "USD".to_owned(),
            estimated: true,
            unknown: false,
        })
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        if self
            .fail_node_id
            .as_deref()
            .is_none_or(|node_id| node_id == req.node_id)
        {
            return Err(ProviderError::UnsupportedCapability(req.capability));
        }
        MockProvider::new().invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        MockProvider::new().cancel(handle).await
    }
}

async fn open_self_heal_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("test.sqlite").display());
    let store = Store::open(&database_url).await.expect("store");
    (store, dir)
}

async fn self_heal_workspace_version(store: &Store) -> (String, String) {
    let workspace = store
        .create_workspace("Self heal")
        .await
        .expect("workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Graph",
            source: VersionSource::Manual,
            graph_path: "graph.json",
            graph_hash: "sha256:self-heal",
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("version");
    (workspace.id, version.id)
}

fn self_heal_graph(node_id: &str) -> WorkflowGraph {
    let input_id = format!("input-{node_id}");
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([
            (
                input_id.clone(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Input".to_owned(),
                    params: json!({ "text": "retry" }),
                    pos: [0.0, 0.0],
                    size: None,
                },
            ),
            (
                node_id.to_owned(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_owned(),
                    title: "Writer".to_owned(),
                    params: json!({ "style": "cinematic" }),
                    pos: [240.0, 0.0],
                    size: None,
                },
            ),
        ]),
        edges: vec![GraphEdge {
            from: [input_id, "text".to_owned()],
            to: [node_id.to_owned(), "text".to_owned()],
            edge_type: "text".to_owned(),
        }],
    }
}

async fn wait_for_retry(store: &Store, workspace_id: &str) -> RunRecord {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(run) = store
                .latest_workspace_run(workspace_id)
                .await
                .expect("latest run")
                && run.attempt > 0
            {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("retry run timeout")
}

async fn wait_for_self_heal_status(store: &Store, run_id: &str, expected: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.run(run_id).await.expect("run").status == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("run status timeout");
}
