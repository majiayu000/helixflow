use std::sync::Arc;

use tokio::sync::Barrier;

use crate::{
    AutoApplyProposalVersionRecord, AutoApplyProposalVersionResult, NewProposal, NewVersion, Store,
    StoreError, StoreResult, VersionSource,
};

async fn proposal_test_store() -> (Store, tempfile::TempDir) {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let options = SqliteConnectOptions::from_str(&database_url)
        .expect("parse database URL")
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("connect test pool");
    let store = Store { pool };
    store.run_migrations().await.expect("run migrations");
    (store, dir)
}

async fn concurrent_proposal_test_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open WAL store");
    assert_eq!(store.journal_mode().await.expect("journal mode"), "wal");
    let first_connection = store.pool().acquire().await.expect("first connection");
    let second_connection = store.pool().acquire().await.expect("second connection");
    drop((first_connection, second_connection));
    (store, dir)
}

#[tokio::test]
async fn auto_apply_commits_proposal_version_workspace_and_message_together() {
    let (store, _dir) = proposal_test_store().await;
    let workspace = store
        .create_workspace("Atomic proposal")
        .await
        .expect("create workspace");
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: "graphs/base.json",
            graph_hash: "sha256:base",
            parent_id: None,
        })
        .await
        .expect("create base");

    let result = store
        .auto_apply_proposal_version(AutoApplyProposalVersionRecord {
            proposal: NewProposal {
                workspace_id: &workspace.id,
                base_version_id: &base.id,
                kind: "modify",
                title: "Atomic edit",
                summary: "Apply it",
                ops_path: "proposals/atomic/ops.json",
                preview_graph_path: Some("proposals/atomic/preview.json"),
                message_id: None,
            },
            version: NewVersion {
                workspace_id: &workspace.id,
                label: "Agent edit",
                source: VersionSource::Proposal,
                graph_path: "proposals/atomic/applied.json",
                graph_hash: "sha256:applied",
                parent_id: Some(&base.id),
            },
            message_text: "Applied",
        })
        .await
        .expect("auto apply");

    assert_eq!(result.proposal.state, "applied");
    assert_eq!(
        result.proposal.result_version_id.as_deref(),
        Some(result.version.id.as_str())
    );
    assert_eq!(
        result.proposal.message_id.as_deref(),
        Some(result.message.id.as_str())
    );
    assert_eq!(
        result.message.ref_id.as_deref(),
        Some(result.proposal.id.as_str())
    );
    let expected_attachment = format!(r#"{{"versionId":"{}"}}"#, result.version.id);
    assert_eq!(
        result.message.attachment_ids_json.as_deref(),
        Some(expected_attachment.as_str())
    );
    assert_eq!(
        store
            .workspace(&workspace.id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(result.version.id.as_str())
    );
    assert!(
        store
            .latest_pending_proposal(&workspace.id)
            .await
            .expect("pending lookup")
            .is_none()
    );
}

#[tokio::test]
async fn auto_apply_version_conflict_rolls_back_all_database_records() {
    let (store, _dir) = proposal_test_store().await;
    let workspace = store
        .create_workspace("Atomic rollback")
        .await
        .expect("create workspace");
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: "graphs/base.json",
            graph_hash: "sha256:base",
            parent_id: None,
        })
        .await
        .expect("create base");
    for (suffix, parent_id) in [("missing", None), ("unrelated", Some("ver_unrelated"))] {
        let mismatch = submit_auto_apply_with_parent(
            store.clone(),
            &workspace.id,
            &base.id,
            suffix,
            parent_id,
        )
        .await
        .expect_err("version parent must match proposal base");
        assert!(matches!(
            mismatch,
            StoreError::VersionParentMismatch {
                expected_parent_version_id,
                actual_parent_version_id,
                ..
            } if expected_parent_version_id == base.id
                && actual_parent_version_id.as_deref() == parent_id
        ));
    }
    assert!(
        store
            .workspace_proposals(&workspace.id)
            .await
            .expect("proposals after invalid parent")
            .is_empty()
    );
    let newer = store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace.id,
                label: "Newer",
                source: VersionSource::Manual,
                graph_path: "graphs/newer.json",
                graph_hash: "sha256:newer",
                parent_id: Some(&base.id),
            },
            &base.id,
        )
        .await
        .expect("advance workspace");

    let err = store
        .auto_apply_proposal_version(AutoApplyProposalVersionRecord {
            proposal: NewProposal {
                workspace_id: &workspace.id,
                base_version_id: &base.id,
                kind: "modify",
                title: "Stale edit",
                summary: "Must roll back",
                ops_path: "proposals/stale/ops.json",
                preview_graph_path: Some("proposals/stale/preview.json"),
                message_id: None,
            },
            version: NewVersion {
                workspace_id: &workspace.id,
                label: "Stale agent edit",
                source: VersionSource::Proposal,
                graph_path: "proposals/stale/applied.json",
                graph_hash: "sha256:stale",
                parent_id: Some(&base.id),
            },
            message_text: "Should not persist",
        })
        .await
        .expect_err("stale proposal must conflict");

    assert!(matches!(err, StoreError::VersionConflict { .. }));
    assert!(
        store
            .workspace_proposals(&workspace.id)
            .await
            .expect("proposals")
            .is_empty()
    );
    assert!(
        store
            .workspace_messages(&workspace.id)
            .await
            .expect("messages")
            .is_empty()
    );
    let versions = store
        .versions_for_workspace(&workspace.id)
        .await
        .expect("versions");
    assert_eq!(versions.len(), 2);
    assert_eq!(
        store
            .workspace(&workspace.id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(newer.id.as_str())
    );
}

#[tokio::test]
async fn concurrent_same_base_auto_apply_has_one_winner_and_one_explicit_version_conflict() {
    let (store, _dir) = concurrent_proposal_test_store().await;
    let workspace = store
        .create_workspace("Concurrent proposals")
        .await
        .expect("create workspace");
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: "graphs/base.json",
            graph_hash: "sha256:base",
            parent_id: None,
        })
        .await
        .expect("create base");

    let barrier = Arc::new(Barrier::new(3));
    let first = tokio::spawn(submit_auto_apply_after_barrier(
        store.clone(),
        workspace.id.clone(),
        base.id.clone(),
        "first",
        Arc::clone(&barrier),
    ));
    let second = tokio::spawn(submit_auto_apply_after_barrier(
        store.clone(),
        workspace.id.clone(),
        base.id.clone(),
        "second",
        Arc::clone(&barrier),
    ));
    barrier.wait().await;
    let results = [
        first.await.expect("first auto apply joined"),
        second.await.expect("second auto apply joined"),
    ];

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let winner = results
        .iter()
        .find_map(|result| result.as_ref().ok())
        .expect("one request must commit");
    let failure = results
        .iter()
        .find_map(|result| result.as_ref().err())
        .expect("one request must fail");
    assert!(matches!(
        failure,
        StoreError::VersionConflict {
            expected_version_id,
            actual_version_id: Some(actual_version_id),
            ..
        } if expected_version_id == &base.id && actual_version_id == &winner.version.id
    ));
    assert_eq!(
        store
            .workspace_proposals(&workspace.id)
            .await
            .expect("proposals")
            .as_slice(),
        [winner.proposal.clone()]
    );
    assert_eq!(
        store
            .versions_for_workspace(&workspace.id)
            .await
            .expect("versions")
            .as_slice(),
        [base.clone(), winner.version.clone()]
    );
    assert_eq!(
        store
            .workspace_messages(&workspace.id)
            .await
            .expect("messages")
            .as_slice(),
        [winner.message.clone()]
    );
    assert_eq!(
        store.workspace(&workspace.id).await.unwrap().cur_version_id,
        Some(winner.version.id.clone())
    );
}

#[tokio::test]
async fn suppressed_current_update_is_classified_and_rolls_back() {
    let (store, _dir) = proposal_test_store().await;
    let workspace = store
        .create_workspace("Suppressed current update")
        .await
        .expect("create workspace");
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: "graphs/base-suppressed.json",
            graph_hash: "sha256:base-suppressed",
            parent_id: None,
        })
        .await
        .expect("create base");
    sqlx::query(
        r#"
        CREATE TEMP TRIGGER ignore_auto_current_update
        BEFORE UPDATE OF cur_version_id ON workspaces
        WHEN NEW.cur_version_id != OLD.cur_version_id
        BEGIN
          SELECT RAISE(IGNORE);
        END
        "#,
    )
    .execute(store.pool())
    .await
    .expect("install ignored-update trigger");

    let error = submit_auto_apply(
        store.clone(),
        workspace.id.clone(),
        base.id.clone(),
        "suppressed",
    )
    .await
    .expect_err("ignored current update must be classified");

    assert!(matches!(
        error,
        StoreError::StatementInvariant {
            operation: "advance_auto_applied_current",
            expected_rows: 1,
            actual_rows: 0,
        }
    ));
    assert_eq!(
        store
            .workspace(&workspace.id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(base.id.as_str())
    );
    assert_eq!(
        store
            .versions_for_workspace(&workspace.id)
            .await
            .expect("versions")
            .len(),
        1
    );
    assert!(
        store
            .workspace_messages(&workspace.id)
            .await
            .expect("messages")
            .is_empty()
    );
    assert!(
        store
            .workspace_proposals(&workspace.id)
            .await
            .expect("proposals")
            .is_empty()
    );
}

#[tokio::test]
async fn diverged_current_after_suppressed_update_is_version_conflict_and_rolls_back() {
    let (store, _dir) = proposal_test_store().await;
    let workspace = store
        .create_workspace("Diverged current update")
        .await
        .expect("create workspace");
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: "graphs/base-diverged.json",
            graph_hash: "sha256:base-diverged",
            parent_id: None,
        })
        .await
        .expect("create base");
    sqlx::query(
        r#"
        CREATE TEMP TRIGGER diverge_auto_current_update
        BEFORE UPDATE OF cur_version_id ON workspaces
        WHEN NEW.cur_version_id != OLD.cur_version_id
        BEGIN
          UPDATE workspaces SET cur_version_id = NULL WHERE id = OLD.id;
          SELECT RAISE(IGNORE);
        END
        "#,
    )
    .execute(store.pool())
    .await
    .expect("install diverging-update trigger");

    let error = submit_auto_apply(
        store.clone(),
        workspace.id.clone(),
        base.id.clone(),
        "diverged",
    )
    .await
    .expect_err("diverged current must conflict");

    assert!(matches!(
        error,
        StoreError::VersionConflict {
            expected_version_id,
            actual_version_id: None,
            ..
        } if expected_version_id == base.id
    ));
    assert_eq!(
        store
            .workspace(&workspace.id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(base.id.as_str())
    );
    assert_eq!(
        store
            .versions_for_workspace(&workspace.id)
            .await
            .expect("versions")
            .len(),
        1
    );
    assert!(
        store
            .workspace_messages(&workspace.id)
            .await
            .expect("messages")
            .is_empty()
    );
    assert!(
        store
            .workspace_proposals(&workspace.id)
            .await
            .expect("proposals")
            .is_empty()
    );
}

#[tokio::test]
async fn auto_apply_version_insert_fault_rolls_back_all_database_records() {
    assert_auto_apply_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_auto_version_insert
        BEFORE INSERT ON versions
        WHEN NEW.label = 'fault'
        BEGIN
          SELECT RAISE(ABORT, 'fail_auto_version_insert');
        END
        "#,
    )
    .await;
}

#[tokio::test]
async fn auto_apply_current_update_fault_rolls_back_all_database_records() {
    assert_auto_apply_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_auto_current_update
        BEFORE UPDATE OF cur_version_id ON workspaces
        WHEN NEW.cur_version_id != OLD.cur_version_id
        BEGIN
          SELECT RAISE(ABORT, 'fail_auto_current_update');
        END
        "#,
    )
    .await;
}

#[tokio::test]
async fn auto_apply_message_insert_fault_rolls_back_all_database_records() {
    assert_auto_apply_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_auto_message_insert
        BEFORE INSERT ON messages
        WHEN NEW.kind = 'proposal_applied'
        BEGIN
          SELECT RAISE(ABORT, 'fail_auto_message_insert');
        END
        "#,
    )
    .await;
}

#[tokio::test]
async fn auto_apply_proposal_insert_fault_rolls_back_all_database_records() {
    assert_auto_apply_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_auto_proposal_insert
        BEFORE INSERT ON proposals
        WHEN NEW.state = 'applied'
        BEGIN
          SELECT RAISE(ABORT, 'fail_auto_proposal_insert');
        END
        "#,
    )
    .await;
}

async fn assert_auto_apply_fault_rolls_back(trigger_sql: &str) {
    let (store, _dir) = proposal_test_store().await;
    let workspace = store
        .create_workspace("Auto apply fault")
        .await
        .expect("create workspace");
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: "graphs/base-fault.json",
            graph_hash: "sha256:base-fault",
            parent_id: None,
        })
        .await
        .expect("create base");
    sqlx::query(trigger_sql)
        .execute(store.pool())
        .await
        .expect("install fault trigger");

    let error = submit_auto_apply(
        store.clone(),
        workspace.id.clone(),
        base.id.clone(),
        "fault",
    )
    .await
    .expect_err("fault must abort auto apply transaction");

    assert!(matches!(error, StoreError::Sqlx(_)));
    assert_eq!(
        store
            .workspace(&workspace.id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(base.id.as_str())
    );
    assert_eq!(
        store
            .versions_for_workspace(&workspace.id)
            .await
            .expect("versions")
            .len(),
        1
    );
    assert!(
        store
            .workspace_messages(&workspace.id)
            .await
            .expect("messages")
            .is_empty()
    );
    assert!(
        store
            .workspace_proposals(&workspace.id)
            .await
            .expect("proposals")
            .is_empty()
    );
}

async fn submit_auto_apply(
    store: Store,
    workspace_id: String,
    base_version_id: String,
    suffix: &'static str,
) -> StoreResult<AutoApplyProposalVersionResult> {
    submit_auto_apply_with_parent(
        store,
        &workspace_id,
        &base_version_id,
        suffix,
        Some(&base_version_id),
    )
    .await
}

async fn submit_auto_apply_after_barrier(
    store: Store,
    workspace_id: String,
    base_version_id: String,
    suffix: &'static str,
    barrier: Arc<Barrier>,
) -> StoreResult<AutoApplyProposalVersionResult> {
    barrier.wait().await;
    submit_auto_apply(store, workspace_id, base_version_id, suffix).await
}

async fn submit_auto_apply_with_parent(
    store: Store,
    workspace_id: &str,
    base_version_id: &str,
    suffix: &str,
    parent_id: Option<&str>,
) -> StoreResult<AutoApplyProposalVersionResult> {
    let ops_path = format!("proposals/{suffix}/ops.json");
    let preview_path = format!("proposals/{suffix}/preview.json");
    let graph_path = format!("proposals/{suffix}/applied.json");
    let graph_hash = format!("sha256:{suffix}");
    store
        .auto_apply_proposal_version(AutoApplyProposalVersionRecord {
            proposal: NewProposal {
                workspace_id,
                base_version_id,
                kind: "modify",
                title: suffix,
                summary: suffix,
                ops_path: &ops_path,
                preview_graph_path: Some(&preview_path),
                message_id: None,
            },
            version: NewVersion {
                workspace_id,
                label: suffix,
                source: VersionSource::Proposal,
                graph_path: &graph_path,
                graph_hash: &graph_hash,
                parent_id,
            },
            message_text: suffix,
        })
        .await
}
