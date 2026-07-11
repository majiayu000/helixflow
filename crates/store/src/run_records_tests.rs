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
