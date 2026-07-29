use super::*;
use crate::{NewVersion, VersionSource};

async fn open_temp_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("helixflow.sqlite");
    let database_url = format!("sqlite://{}", db_path.display());
    let store = Store::open(&database_url).await.expect("open store");
    (store, dir)
}

async fn seed_workspace_run(
    store: &Store,
    workspace_id: Option<&str>,
    group_id: Option<&str>,
    status: &str,
) -> (String, RunRecord) {
    let workspace_id = match workspace_id {
        Some(id) => id.to_owned(),
        None => store.create_workspace("Claim").await.expect("workspace").id,
    };
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace_id,
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
            workspace_id: &workspace_id,
            version_id: &version.id,
            group_id,
            label: "Run",
            trigger: "agent",
            plan_json: None,
            estimate_json: None,
            status,
        })
        .await
        .expect("create run");
    (workspace_id, run)
}

#[tokio::test]
async fn second_claim_for_same_workspace_is_rejected() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, run_a) =
        seed_workspace_run(&store, None, None, "waiting_confirmation").await;
    let (_, run_b) =
        seed_workspace_run(&store, Some(&workspace_id), None, "waiting_confirmation").await;

    let first = store
        .claim_run_if_workspace_idle(
            &run_a.id,
            "waiting_confirmation",
            "running",
            &workspace_id,
            None,
            false,
        )
        .await
        .expect("first claim");
    assert_eq!(first.expect("first claim wins").status, "running");

    let second = store
        .claim_run_if_workspace_idle(
            &run_b.id,
            "waiting_confirmation",
            "running",
            &workspace_id,
            None,
            false,
        )
        .await
        .expect("second claim");
    assert!(
        second.is_none(),
        "second claim must be rejected while the first run is active"
    );
    assert_eq!(
        store.run(&run_b.id).await.expect("run b").status,
        "waiting_confirmation"
    );
}

#[tokio::test]
async fn claims_sharing_a_sweep_group_do_not_block_each_other() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, run_a) =
        seed_workspace_run(&store, None, Some("sweep_1"), "waiting_confirmation").await;
    let (_, run_b) = seed_workspace_run(
        &store,
        Some(&workspace_id),
        Some("sweep_1"),
        "waiting_confirmation",
    )
    .await;

    for run in [&run_a, &run_b] {
        let claimed = store
            .claim_run_if_workspace_idle(
                &run.id,
                "waiting_confirmation",
                "running",
                &workspace_id,
                Some("sweep_1"),
                false,
            )
            .await
            .expect("claim");
        assert_eq!(claimed.expect("sweep member claims").status, "running");
    }
}

#[tokio::test]
async fn claim_requires_expected_status() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, run) = seed_workspace_run(&store, None, None, "failed").await;

    let claimed = store
        .claim_run_if_workspace_idle(
            &run.id,
            "waiting_confirmation",
            "running",
            &workspace_id,
            None,
            false,
        )
        .await
        .expect("claim");
    assert!(claimed.is_none());
}
