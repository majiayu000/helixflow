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
        )
        .await
        .expect("claim");
    assert!(claimed.is_none());
}

#[tokio::test]
async fn active_creates_preserve_sweep_group_admission() {
    let (store, _dir) = open_temp_store().await;
    for (first_group, next_group, allowed) in [
        (Some("sweep_1"), Some("sweep_1"), true),
        (Some("sweep_1"), Some("sweep_2"), false),
        (Some("sweep_1"), None, false),
        (None, Some("sweep_1"), false),
        (None, None, false),
    ] {
        for status in ["queued", "estimating", "running"] {
            let (workspace_id, first) = seed_workspace_run(&store, None, first_group, status).await;
            let next = store
                .create_run(NewRun {
                    workspace_id: &workspace_id,
                    version_id: &first.version_id,
                    group_id: next_group,
                    label: "Next run",
                    trigger: "sweep",
                    plan_json: None,
                    estimate_json: None,
                    status: "queued",
                })
                .await;
            if allowed {
                next.expect("same-group sweep member is admitted");
            } else {
                assert!(matches!(
                    next.expect_err("unrelated active run blocks creation"),
                    StoreError::WorkspaceBusy { active_run_id, .. } if active_run_id == first.id
                ));
            }
            assert_eq!(
                store
                    .active_workspace_runs(&workspace_id)
                    .await
                    .expect("active runs")
                    .len(),
                if allowed { 2 } else { 1 }
            );
        }
    }
}

#[tokio::test]
async fn create_and_claim_compete_for_the_same_workspace() {
    let (store, dir) = open_temp_store().await;
    let other = Store::open(&format!(
        "sqlite://{}",
        dir.path().join("helixflow.sqlite").display()
    ))
    .await
    .expect("independent store");
    let (workspace_id, pending) =
        seed_workspace_run(&store, None, None, "waiting_confirmation").await;
    let (created, claimed) = tokio::join!(
        store.create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &pending.version_id,
            group_id: None,
            label: "Concurrent create",
            trigger: "manual",
            plan_json: None,
            estimate_json: None,
            status: "queued",
        }),
        other.claim_run_if_workspace_idle(
            &pending.id,
            "waiting_confirmation",
            "running",
            &workspace_id,
            None
        )
    );
    let claimed = claimed.expect("claim query");
    assert_eq!(
        usize::from(created.is_ok()) + usize::from(claimed.is_some()),
        1
    );
    if let Err(err) = created {
        assert!(
            matches!(err, StoreError::WorkspaceBusy { active_run_id, .. } if active_run_id == pending.id)
        );
    } else {
        assert_eq!(
            store.run(&pending.id).await.expect("pending run").status,
            "waiting_confirmation"
        );
    }
    assert_eq!(
        store
            .active_workspace_runs(&workspace_id)
            .await
            .expect("active runs")
            .len(),
        1
    );
}

#[tokio::test]
async fn inactive_creates_and_other_workspaces_do_not_take_the_active_slot() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, active) = seed_workspace_run(&store, None, None, "queued").await;
    for status in ["waiting_confirmation", "succeeded", "failed", "interrupted"] {
        seed_workspace_run(&store, Some(&workspace_id), None, status).await;
    }
    // Retry creation remains inactive until the existing atomic claim admits it.
    let retry = store
        .create_retry_run(&active.id, false)
        .await
        .expect("pending retry");
    assert_eq!(retry.status, "waiting_confirmation");
    assert!(
        store
            .claim_run_if_workspace_idle(
                &retry.id,
                "waiting_confirmation",
                "running",
                &workspace_id,
                None
            )
            .await
            .expect("claim retry")
            .is_none()
    );
    seed_workspace_run(&store, None, None, "running").await;
    assert_eq!(
        store
            .active_workspace_runs(&workspace_id)
            .await
            .expect("active runs")
            .len(),
        1
    );

    store
        .update_run_status(&active.id, "succeeded", None)
        .await
        .expect("finish active run");
    seed_workspace_run(&store, Some(&workspace_id), None, "estimating").await;
}
