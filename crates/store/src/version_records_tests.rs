use std::str::FromStr;
use std::sync::Arc;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tokio::sync::Barrier;

use super::{
    NewProposal, NewVersion, Store, StoreError, StoreResult, VersionRecord, VersionSource,
};

async fn open_temp_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("helixflow.sqlite");
    let database_url = format!("sqlite://{}", db_path.display());
    let store = Store::open(&database_url).await.expect("open store");
    (store, dir)
}

async fn open_single_connection_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let options = SqliteConnectOptions::from_str(&format!(
        "sqlite://{}",
        dir.path().join("helixflow.sqlite").display()
    ))
    .expect("sqlite options")
    .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("connect sqlite");
    let store = Store { pool };
    store.run_migrations().await.expect("run migrations");
    (store, dir)
}

#[tokio::test]
async fn creates_workspace_and_version() {
    let (store, _dir) = open_temp_store().await;

    let workspace = store
        .create_workspace("First workflow")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Initial graph",
            source: VersionSource::Manual,
            graph_path: "workspaces/ws_1/graphs/ver_1.json",
            graph_hash: "sha256:graph",
            parent_id: None,
        })
        .await
        .expect("create version");
    let updated_workspace = store.workspace(&workspace.id).await.expect("workspace");

    assert_eq!(version.workspace_id, workspace.id);
    assert_eq!(version.idx, 1);
    assert_eq!(version.source, "manual");
    assert_eq!(
        updated_workspace.cur_version_id.as_deref(),
        Some(version.id.as_str())
    );
    assert_eq!(updated_workspace.runtime_provider_id, None);
}

#[tokio::test]
async fn conditional_version_creation_rejects_stale_parent() {
    let (store, _dir) = open_temp_store().await;

    let workspace = store
        .create_workspace("First workflow")
        .await
        .expect("create workspace");
    let first = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Initial graph",
            source: VersionSource::Manual,
            graph_path: "workspaces/ws_1/graphs/ver_1.json",
            graph_hash: "sha256:graph-1",
            parent_id: None,
        })
        .await
        .expect("create first version");
    let second = store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace.id,
                label: "Second graph",
                source: VersionSource::Manual,
                graph_path: "workspaces/ws_1/graphs/ver_2.json",
                graph_hash: "sha256:graph-2",
                parent_id: Some(&first.id),
            },
            &first.id,
        )
        .await
        .expect("create second version");
    let err = store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace.id,
                label: "Stale graph",
                source: VersionSource::Proposal,
                graph_path: "workspaces/ws_1/graphs/ver_stale.json",
                graph_hash: "sha256:stale",
                parent_id: Some(&first.id),
            },
            &first.id,
        )
        .await
        .expect_err("stale current version should fail");

    assert!(matches!(
        err,
        StoreError::VersionConflict {
            actual_version_id: Some(actual),
            ..
        } if actual == second.id
    ));
}

#[tokio::test]
async fn concurrent_same_base_normal_creation_has_one_winner_and_no_orphan() {
    assert_repeated_concurrent_same_base(false).await;
}

#[tokio::test]
async fn concurrent_same_base_pending_guarded_creation_has_one_winner_and_no_orphan() {
    assert_repeated_concurrent_same_base(true).await;
}

#[tokio::test]
async fn pending_guard_rejects_existing_pending_proposal_and_rolls_back_insert() {
    let (store, _dir) = open_temp_store().await;
    let workspace = store
        .create_workspace("Pending guard")
        .await
        .expect("create workspace");
    let base = create_base_version(&store, &workspace.id, "pending").await;
    store
        .create_proposal(NewProposal {
            workspace_id: &workspace.id,
            base_version_id: &base.id,
            kind: "modify",
            title: "Pending",
            summary: "Must block guarded version creation",
            ops_path: "workspaces/pending/ops.json",
            preview_graph_path: None,
            message_id: None,
        })
        .await
        .expect("create pending proposal");

    let error = store
        .create_version_after_without_pending_proposal(
            NewVersion {
                workspace_id: &workspace.id,
                label: "Guarded",
                source: VersionSource::Manual,
                graph_path: "workspaces/pending/guarded.json",
                graph_hash: "sha256:guarded",
                parent_id: Some(&base.id),
            },
            &base.id,
        )
        .await
        .expect_err("pending proposal must block guarded creation");

    assert!(matches!(
        error,
        StoreError::PendingProposalConflict { workspace_id } if workspace_id == workspace.id
    ));
    let versions = store
        .versions_for_workspace(&workspace.id)
        .await
        .expect("versions after guarded failure");
    assert_eq!(versions, vec![base.clone()]);
    assert_eq!(
        store
            .workspace(&workspace.id)
            .await
            .expect("workspace after guarded failure")
            .cur_version_id,
        Some(base.id.clone())
    );

    let normal = store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace.id,
                label: "Normal",
                source: VersionSource::Manual,
                graph_path: "workspaces/pending/normal.json",
                graph_hash: "sha256:normal",
                parent_id: Some(&base.id),
            },
            &base.id,
        )
        .await
        .expect("normal creation does not apply the pending guard");
    assert_eq!(
        store
            .versions_for_workspace(&workspace.id)
            .await
            .expect("versions after normal creation"),
        vec![base, normal]
    );
}

#[tokio::test]
async fn version_insert_fault_rolls_back_current_and_preserves_base_hash() {
    assert_version_statement_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_conditional_version_insert
        BEFORE INSERT ON versions
        WHEN NEW.label = 'Faulted version'
        BEGIN
          SELECT RAISE(ABORT, 'fail_conditional_version_insert');
        END
        "#,
    )
    .await;
}

#[tokio::test]
async fn current_cas_fault_rolls_back_version_insert_and_preserves_base_hash() {
    assert_version_statement_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_conditional_current_update
        BEFORE UPDATE OF cur_version_id ON workspaces
        WHEN NEW.cur_version_id != OLD.cur_version_id
        BEGIN
          SELECT RAISE(ABORT, 'fail_conditional_current_update');
        END
        "#,
    )
    .await;
}

#[tokio::test]
async fn normal_creation_rejects_missing_declared_parent_before_writes() {
    assert_parent_mismatch_is_write_free(false, None).await;
}

#[tokio::test]
async fn pending_guarded_creation_rejects_unrelated_declared_parent_before_writes() {
    assert_parent_mismatch_is_write_free(true, Some("ver_unrelated")).await;
}

async fn assert_parent_mismatch_is_write_free(
    reject_pending_proposal: bool,
    declared_parent_id: Option<&str>,
) {
    let (store, _dir) = open_temp_store().await;
    let workspace = store
        .create_workspace("Parent mismatch")
        .await
        .expect("create workspace");
    let base = create_base_version(&store, &workspace.id, "parent-mismatch").await;
    let input = NewVersion {
        workspace_id: &workspace.id,
        label: "Mismatched parent",
        source: VersionSource::Manual,
        graph_path: "workspaces/parent-mismatch/rejected.json",
        graph_hash: "sha256:rejected",
        parent_id: declared_parent_id,
    };

    let error = if reject_pending_proposal {
        store
            .create_version_after_without_pending_proposal(input, &base.id)
            .await
    } else {
        store.create_version_after(input, &base.id).await
    }
    .expect_err("declared parent must equal the expected current version");

    assert!(matches!(
        error,
        StoreError::VersionParentMismatch {
            workspace_id,
            expected_parent_version_id,
            actual_parent_version_id,
        } if workspace_id == workspace.id
            && expected_parent_version_id == base.id
            && actual_parent_version_id.as_deref() == declared_parent_id
    ));
    assert_eq!(
        store
            .versions_for_workspace(&workspace.id)
            .await
            .expect("versions after parent mismatch"),
        vec![base.clone()]
    );
    assert_eq!(
        store
            .workspace(&workspace.id)
            .await
            .expect("workspace after parent mismatch")
            .cur_version_id,
        Some(base.id)
    );
}

async fn assert_version_statement_fault_rolls_back(trigger: &str) {
    let (store, _dir) = open_single_connection_store().await;
    let workspace = store
        .create_workspace("Statement fault")
        .await
        .expect("create workspace");
    let base = create_base_version(&store, &workspace.id, "statement-fault").await;
    sqlx::query(trigger)
        .execute(store.pool())
        .await
        .expect("install TEMP trigger");

    let error = store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace.id,
                label: "Faulted version",
                source: VersionSource::Manual,
                graph_path: "workspaces/fault/rejected.json",
                graph_hash: "sha256:rejected",
                parent_id: Some(&base.id),
            },
            &base.id,
        )
        .await
        .expect_err("injected statement fault must fail");

    assert!(matches!(error, StoreError::Sqlx(_)));
    let versions = store
        .versions_for_workspace(&workspace.id)
        .await
        .expect("versions after statement fault");
    assert_eq!(versions, vec![base.clone()]);
    assert_eq!(versions[0].graph_hash, base.graph_hash);
    assert_eq!(
        store
            .workspace(&workspace.id)
            .await
            .expect("workspace after statement fault")
            .cur_version_id,
        Some(base.id)
    );
}

async fn assert_repeated_concurrent_same_base(reject_pending_proposal: bool) {
    let (store, _dir) = open_temp_store().await;
    for attempt in 0..8 {
        let workspace = store
            .create_workspace(&format!("Concurrent {reject_pending_proposal} {attempt}"))
            .await
            .expect("create workspace");
        let base = create_base_version(&store, &workspace.id, &attempt.to_string()).await;
        let barrier = Arc::new(Barrier::new(3));
        let first = tokio::spawn(concurrent_version_attempt(
            store.clone(),
            Arc::clone(&barrier),
            workspace.id.clone(),
            base.id.clone(),
            format!("{attempt}-first"),
            reject_pending_proposal,
        ));
        let second = tokio::spawn(concurrent_version_attempt(
            store.clone(),
            Arc::clone(&barrier),
            workspace.id.clone(),
            base.id.clone(),
            format!("{attempt}-second"),
            reject_pending_proposal,
        ));
        barrier.wait().await;
        let results = [
            first.await.expect("first task joined"),
            second.await.expect("second task joined"),
        ];
        let winners: Vec<&VersionRecord> = results
            .iter()
            .filter_map(|result| result.as_ref().ok())
            .collect();
        assert_eq!(winners.len(), 1, "attempt {attempt}: {results:?}");
        let winner = winners[0];
        let conflicts: Vec<&StoreError> = results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .collect();
        assert_eq!(conflicts.len(), 1, "attempt {attempt}: {results:?}");
        assert!(
            matches!(
                conflicts[0],
                StoreError::VersionConflict {
                    workspace_id,
                    expected_version_id,
                    actual_version_id: Some(actual_version_id),
                } if workspace_id.as_str() == workspace.id.as_str()
                    && expected_version_id.as_str() == base.id.as_str()
                    && actual_version_id.as_str() == winner.id.as_str()
            ),
            "attempt {attempt}: {results:?}"
        );

        let versions = store
            .versions_for_workspace(&workspace.id)
            .await
            .expect("versions after concurrent attempts");
        assert_eq!(versions.len(), 2, "attempt {attempt}: orphan version found");
        assert_eq!(versions[0], base);
        assert_eq!(versions[1], *winner);
        assert_eq!(
            store
                .workspace(&workspace.id)
                .await
                .expect("workspace after concurrent attempts")
                .cur_version_id
                .as_deref(),
            Some(winner.id.as_str())
        );
    }
}

async fn concurrent_version_attempt(
    store: Store,
    barrier: Arc<Barrier>,
    workspace_id: String,
    base_version_id: String,
    suffix: String,
    reject_pending_proposal: bool,
) -> StoreResult<VersionRecord> {
    barrier.wait().await;
    let label = format!("Concurrent {suffix}");
    let graph_path = format!("workspaces/{workspace_id}/{suffix}.json");
    let graph_hash = format!("sha256:{suffix}");
    let input = NewVersion {
        workspace_id: &workspace_id,
        label: &label,
        source: VersionSource::Manual,
        graph_path: &graph_path,
        graph_hash: &graph_hash,
        parent_id: Some(&base_version_id),
    };
    if reject_pending_proposal {
        store
            .create_version_after_without_pending_proposal(input, &base_version_id)
            .await
    } else {
        store.create_version_after(input, &base_version_id).await
    }
}

async fn create_base_version(store: &Store, workspace_id: &str, suffix: &str) -> VersionRecord {
    let graph_path = format!("workspaces/{workspace_id}/base-{suffix}.json");
    let graph_hash = format!("sha256:base-{suffix}");
    store
        .create_version(NewVersion {
            workspace_id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: &graph_path,
            graph_hash: &graph_hash,
            parent_id: None,
        })
        .await
        .expect("create base version")
}
