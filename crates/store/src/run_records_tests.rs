use super::*;
use crate::{NewVersion, VersionSource};

async fn open_temp_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("helixflow.sqlite");
    let database_url = format!("sqlite://{}", db_path.display());
    let store = Store::open(&database_url).await.expect("open store");
    (store, dir)
}

#[tokio::test]
async fn select_run_artifact_clears_sibling_artifacts() {
    let (store, _dir) = open_temp_store().await;
    let workspace = store
        .create_workspace("Artifacts")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Graph",
            source: VersionSource::Manual,
            graph_path: "graphs/current.json",
            graph_hash: "sha256:graph",
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create version");
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace.id,
            version_id: &version.id,
            group_id: None,
            label: "Run",
            trigger: "manual",
            plan_json: None,
            estimate_json: None,
            status: "succeeded",
        })
        .await
        .expect("create run");
    let first = artifact(&store, &workspace.id, &run.id, "first", true).await;
    let second = artifact(&store, &workspace.id, &run.id, "second", false).await;

    let selected = store
        .select_run_artifact(&second.id)
        .await
        .expect("select artifact");
    let artifacts = store.run_artifacts(&run.id).await.expect("artifacts");

    assert_eq!(selected.id, second.id);
    assert_eq!(
        artifacts
            .iter()
            .filter(|artifact| artifact.selected)
            .map(|artifact| artifact.id.as_str())
            .collect::<Vec<_>>(),
        vec![second.id.as_str()]
    );
    assert!(!store.artifact(&first.id).await.expect("first").selected);
}

#[tokio::test]
async fn create_retry_run_derives_child_and_preserves_parent() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id, parent) = seed_run(&store, "running").await;
    store
        .update_run_status(&parent.id, "failed", Some(r#"{"error":"boom"}"#))
        .await
        .expect("fail parent");
    sqlx::query("UPDATE runs SET trigger = 'sweep', group_id = 'group_old' WHERE id = ?")
        .bind(&parent.id)
        .execute(store.pool())
        .await
        .expect("mark sweep parent");
    store
        .create_cost_ledger(NewCostLedger {
            workspace_id: &workspace_id,
            run_id: Some(&parent.id),
            run_step_id: None,
            provider: "mock",
            amount: 0.42,
            currency: "USD",
            estimated: true,
        })
        .await
        .expect("parent estimate");

    let retry = store
        .create_retry_run(&parent.id, false)
        .await
        .expect("retry");
    assert_eq!(retry.parent_run_id.as_deref(), Some(parent.id.as_str()));
    assert_eq!(retry.attempt, 1);
    assert_eq!(retry.status, "waiting_confirmation");
    assert_eq!(retry.version_id, version_id);
    assert_eq!(retry.workspace_id, workspace_id);
    assert_eq!(retry.trigger, "agent");
    assert!(retry.group_id.is_none());
    let retry_costs = store
        .cost_ledger_for_run(&retry.id)
        .await
        .expect("retry costs");
    assert_eq!(retry_costs.len(), 1);
    assert_eq!(retry_costs[0].amount, 0.42);

    let reloaded = store.run(&parent.id).await.expect("parent preserved");
    assert_eq!(reloaded.status, "failed");
    assert_eq!(reloaded.error_json.as_deref(), Some(r#"{"error":"boom"}"#));

    let retry2 = store
        .create_retry_run(&retry.id, false)
        .await
        .expect("retry2");
    assert_eq!(retry2.attempt, 2);
    assert_eq!(retry2.parent_run_id.as_deref(), Some(retry.id.as_str()));
}

#[tokio::test]
async fn create_reject_retry_run_once_is_durable_and_idempotent() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id, parent) = seed_run(&store, "succeeded").await;
    store
        .create_cost_ledger(NewCostLedger {
            workspace_id: &workspace_id,
            run_id: Some(&parent.id),
            run_step_id: None,
            provider: "mock",
            amount: 0.42,
            currency: "USD",
            estimated: true,
        })
        .await
        .expect("parent estimate");

    let (first, created) = store
        .create_reject_retry_run_once(&parent.id)
        .await
        .expect("create reject retry");
    assert!(created);
    assert_eq!(first.parent_run_id.as_deref(), Some(parent.id.as_str()));
    assert_eq!(first.attempt, 1);
    assert_eq!(first.status, "waiting_confirmation");
    assert_eq!(first.version_id, version_id);
    assert!(first.force_rerun);
    assert_eq!(
        store
            .cost_ledger_for_run(&first.id)
            .await
            .expect("copied estimates")
            .len(),
        1
    );

    let (replay, created_again) = store
        .create_reject_retry_run_once(&parent.id)
        .await
        .expect("replay reject retry");
    assert!(!created_again);
    assert_eq!(first.id, replay.id);

    let (left, right) = tokio::join!(
        store.create_reject_retry_run_once(&parent.id),
        store.create_reject_retry_run_once(&parent.id)
    );
    let left = left.expect("concurrent left");
    let right = right.expect("concurrent right");
    assert_eq!(left.0.id, first.id);
    assert_eq!(right.0.id, first.id);
}

#[tokio::test]
async fn artifact_review_state_defaults_pending_and_guards_transitions() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, _version_id, run) = seed_run(&store, "succeeded").await;
    let art = artifact(&store, &workspace_id, &run.id, "node", false).await;
    assert_eq!(art.review_state, "pending");

    let accepted = store
        .set_artifact_review_state(&art.id, &["pending"], "accepted")
        .await
        .expect("set")
        .expect("transition applied");
    assert_eq!(accepted.review_state, "accepted");

    // `accepted` is terminal: a pending-guarded transition no longer matches.
    let blocked = store
        .set_artifact_review_state(&art.id, &["pending"], "rejected")
        .await
        .expect("set");
    assert!(blocked.is_none());
    assert_eq!(
        store.artifact(&art.id).await.expect("art").review_state,
        "accepted"
    );
}

#[tokio::test]
async fn artifact_identity_rejects_cross_workspace_run_and_cross_run_step() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_a, _version_a, run_a) = seed_run(&store, "running").await;
    let (workspace_b, _version_b, run_b) = seed_run(&store, "running").await;
    let step_b = store
        .create_run_step(NewRunStep {
            run_id: &run_b.id,
            node_id: "foreign",
            node_type: "image.generate",
            provider: Some("atlas"),
            state: "running",
        })
        .await
        .expect("create foreign step");

    let cross_workspace = store
        .create_artifact(NewArtifact {
            workspace_id: &workspace_a,
            run_id: Some(&run_b.id),
            run_step_id: None,
            node_id: Some("foreign"),
            kind: "image",
            storage_uri: "artifacts/cross-workspace.png",
            sha256: None,
            mime: Some("image/png"),
            width: None,
            height: None,
            duration_ms: None,
            selected: false,
            meta_json: None,
        })
        .await
        .expect_err("cross-workspace artifact must fail closed");
    assert!(matches!(
        cross_workspace,
        StoreError::RecoveryInvariant { .. }
    ));

    let cross_run_step = store
        .create_artifact(NewArtifact {
            workspace_id: &workspace_a,
            run_id: Some(&run_a.id),
            run_step_id: Some(&step_b.id),
            node_id: Some("foreign"),
            kind: "image",
            storage_uri: "artifacts/cross-run-step.png",
            sha256: None,
            mime: Some("image/png"),
            width: None,
            height: None,
            duration_ms: None,
            selected: false,
            meta_json: None,
        })
        .await
        .expect_err("cross-run step artifact must fail closed");
    assert!(matches!(
        cross_run_step,
        StoreError::RecoveryInvariant { .. }
    ));

    let artifact_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM artifacts")
        .fetch_one(store.pool())
        .await
        .expect("count artifacts");
    assert_eq!(artifact_count, 0);
    assert_ne!(workspace_a, workspace_b);
}

#[tokio::test]
async fn terminal_run_and_step_states_reject_late_overwrites() {
    let (store, _dir) = open_temp_store().await;
    let (_workspace_id, _version_id, run) = seed_run(&store, "running").await;
    let step = store
        .create_run_step(NewRunStep {
            run_id: &run.id,
            node_id: "writer",
            node_type: "llm.prompt_writer",
            provider: Some("atlas"),
            state: "running",
        })
        .await
        .expect("create step");

    store
        .update_run_status(&run.id, "interrupted", None)
        .await
        .expect("interrupt run");
    store
        .update_run_step_state(&step.id, "skipped", None, None, None)
        .await
        .expect("skip step");

    store
        .update_run_status(&run.id, "succeeded", None)
        .await
        .expect_err("late run success must not overwrite interruption");
    store
        .update_run_estimate_and_status(&run.id, Some("{}"), "waiting_confirmation")
        .await
        .expect_err("late estimate must not reopen interrupted run");
    store
        .update_run_step_state(&step.id, "succeeded", Some(1.0), None, None)
        .await
        .expect_err("late step success must not overwrite skip");
    store
        .mark_run_step_cached_succeeded(&step.id, r#"{"cached":true}"#)
        .await
        .expect_err("cache metadata must not reopen a skipped step");

    assert_eq!(store.run(&run.id).await.expect("run").status, "interrupted");
    assert_eq!(
        store.run_step(&step.id).await.expect("step").state,
        "skipped"
    );
}

async fn seed_run(store: &Store, status: &str) -> (String, String, RunRecord) {
    let workspace = store.create_workspace("Seed").await.expect("workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Graph",
            source: VersionSource::Manual,
            graph_path: "graphs/current.json",
            graph_hash: "sha256:graph",
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create version");
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace.id,
            version_id: &version.id,
            group_id: None,
            label: "Run",
            trigger: "agent",
            plan_json: None,
            estimate_json: None,
            status,
        })
        .await
        .expect("create run");
    (workspace.id, version.id, run)
}

async fn artifact(
    store: &Store,
    workspace_id: &str,
    run_id: &str,
    node_id: &str,
    selected: bool,
) -> ArtifactRecord {
    store
        .create_artifact(NewArtifact {
            workspace_id,
            run_id: Some(run_id),
            run_step_id: None,
            node_id: Some(node_id),
            kind: "video",
            storage_uri: "workspace://outputs/run/video.mp4",
            sha256: None,
            mime: Some("video/mp4"),
            width: Some(1080),
            height: Some(1920),
            duration_ms: Some(5000),
            selected,
            meta_json: Some(r#"{"provider":"mock","capability":"text_to_video"}"#),
        })
        .await
        .expect("create artifact")
}

#[tokio::test]
async fn terminalization_waits_for_every_provider_task() {
    let (store, _dir) = open_temp_store().await;
    let (_workspace_id, _version_id, run) = seed_run(&store, "running").await;
    let step = store
        .create_run_step(NewRunStep {
            run_id: &run.id,
            node_id: "video",
            node_type: "video.generate",
            provider: Some("atlas"),
            state: "running",
        })
        .await
        .expect("create step");
    let dispatching = store
        .insert_or_read_provider_task(NewProviderTask {
            run_id: &run.id,
            run_step_id: &step.id,
            provider: "atlas",
            dispatch_origin: "https://api.atlascloud.ai",
            recovery_scope_fingerprint: "sha256:scope",
            operation_key: "dispatch:step",
            dispatch_owner_id: "dispatch-owner",
            dispatch_lease_seconds: 30,
            dispatch_deadline_seconds: 60,
        })
        .await
        .expect("dispatch task");
    let task = store
        .activate_provider_task(ProviderTaskHandleUpdate {
            task_id: &dispatching.id,
            dispatch_owner_id: "dispatch-owner",
            provider_task_id: "remote-task",
            status_url: None,
            result_url: None,
            recovery_deadline_seconds: 600,
        })
        .await
        .expect("activate")
        .expect("active task");

    store
        .request_run_terminalization(&run.id, "failed", Some(r#"{"code":"PROVIDER_FAILED"}"#))
        .await
        .expect("request terminalization");
    assert!(
        store
            .claim_run_terminalization(&run.id, "settler", 30)
            .await
            .expect("claim settler")
    );
    assert!(
        store
            .complete_run_terminalization(&run.id, "settler")
            .await
            .expect("try complete")
            .is_none()
    );
    assert_eq!(store.run(&run.id).await.expect("run").status, "running");

    store
        .abandon_provider_task(&task.id, "active", "CANCEL_UNSUPPORTED")
        .await
        .expect("abandon")
        .expect("task terminal");
    let failed = store
        .complete_run_terminalization(&run.id, "settler")
        .await
        .expect("complete")
        .expect("failed run");
    assert_eq!(failed.status, "failed");
    assert_eq!(
        store.run_steps(&run.id).await.expect("steps")[0].state,
        "skipped"
    );
    assert_eq!(
        store
            .failure_continuation(&run.id)
            .await
            .expect("continuation")
            .expect("durable continuation")
            .state,
        "pending"
    );
}

#[tokio::test]
async fn interrupted_terminalization_has_no_failure_continuation() {
    let (store, _dir) = open_temp_store().await;
    let (_workspace_id, _version_id, run) = seed_run(&store, "running").await;
    store
        .request_run_terminalization(&run.id, "interrupted", None)
        .await
        .expect("request terminalization");
    assert!(
        store
            .claim_run_terminalization(&run.id, "settler", 30)
            .await
            .expect("claim settler")
    );
    let interrupted = store
        .complete_run_terminalization(&run.id, "settler")
        .await
        .expect("complete")
        .expect("interrupted run");
    assert_eq!(interrupted.status, "interrupted");
    assert!(
        store
            .failure_continuation(&run.id)
            .await
            .expect("continuation query")
            .is_none()
    );
}

#[tokio::test]
async fn retry_child_creation_is_durable_and_idempotent() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, _version_id, run) = seed_run(&store, "running").await;
    store
        .create_cost_ledger(NewCostLedger {
            workspace_id: &workspace_id,
            run_id: Some(&run.id),
            run_step_id: None,
            provider: "atlas",
            amount: 2.0,
            currency: "USD",
            estimated: true,
        })
        .await
        .expect("create estimate");
    store
        .request_run_terminalization(&run.id, "failed", Some(r#"{"code":"PROVIDER_FAILED"}"#))
        .await
        .expect("request terminalization");
    assert!(
        store
            .claim_run_terminalization(&run.id, "settler", 30)
            .await
            .expect("claim")
    );
    store
        .complete_run_terminalization(&run.id, "settler")
        .await
        .expect("complete")
        .expect("failed run");

    let first = store
        .create_retry_run_once(&run.id, true)
        .await
        .expect("create retry");
    let replay = store
        .create_retry_run_once(&run.id, true)
        .await
        .expect("replay retry");
    assert_eq!(first.id, replay.id);
    assert_eq!(first.parent_run_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(
        store
            .cost_ledger_for_run(&first.id)
            .await
            .expect("copied estimates")
            .len(),
        1
    );
    let continuation = store
        .failure_continuation(&run.id)
        .await
        .expect("continuation")
        .expect("record");
    assert_eq!(continuation.state, "retry_created");
    assert_eq!(
        continuation.child_run_id.as_deref(),
        Some(first.id.as_str())
    );
}

#[tokio::test]
async fn retry_creation_requires_a_durable_failure_handoff() {
    let (store, _dir) = open_temp_store().await;
    let (_workspace_id, _version_id, run) = seed_run(&store, "failed").await;
    let err = store
        .create_retry_run_once(&run.id, true)
        .await
        .expect_err("missing continuation must fail");
    assert!(matches!(
        err,
        StoreError::RecoveryInvariant {
            operation: "create_retry_run_once",
            ..
        }
    ));
}
