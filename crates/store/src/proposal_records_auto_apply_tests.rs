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
    let mismatched_parent = store
        .auto_apply_proposal_version(AutoApplyProposalVersionRecord {
            proposal: NewProposal {
                workspace_id: &workspace.id,
                base_version_id: &base.id,
                kind: "modify",
                title: "Invalid parent",
                summary: "Must not persist",
                ops_path: "proposals/invalid/ops.json",
                preview_graph_path: Some("proposals/invalid/preview.json"),
                message_id: None,
            },
            version: NewVersion {
                workspace_id: &workspace.id,
                label: "Invalid parent",
                source: VersionSource::Proposal,
                graph_path: "proposals/invalid/applied.json",
                graph_hash: "sha256:invalid",
                parent_id: None,
            },
            message_text: "Should not persist",
        })
        .await
        .expect_err("version parent must match proposal base");
    assert!(matches!(
        mismatched_parent,
        StoreError::VersionConflict { .. }
    ));
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
async fn concurrent_auto_apply_has_one_winner_and_one_explicit_version_conflict() {
    let (store, _dir) = proposal_test_store().await;
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

    let first = submit_auto_apply(
        store.clone(),
        workspace.id.clone(),
        base.id.clone(),
        "first",
    );
    let second = submit_auto_apply(
        store.clone(),
        workspace.id.clone(),
        base.id.clone(),
        "second",
    );
    let (first, second) = tokio::join!(first, second);
    let results = [first, second];

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let failure = results
        .iter()
        .find_map(|result| result.as_ref().err())
        .expect("one request must fail");
    assert!(matches!(failure, StoreError::VersionConflict { .. }));
    assert_eq!(
        store
            .workspace_proposals(&workspace.id)
            .await
            .expect("proposals")
            .len(),
        1
    );
    assert_eq!(
        store
            .versions_for_workspace(&workspace.id)
            .await
            .expect("versions")
            .len(),
        2
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
    let ops_path = format!("proposals/{suffix}/ops.json");
    let preview_path = format!("proposals/{suffix}/preview.json");
    let graph_path = format!("proposals/{suffix}/applied.json");
    let graph_hash = format!("sha256:{suffix}");
    store
        .auto_apply_proposal_version(AutoApplyProposalVersionRecord {
            proposal: NewProposal {
                workspace_id: &workspace_id,
                base_version_id: &base_version_id,
                kind: "modify",
                title: suffix,
                summary: suffix,
                ops_path: &ops_path,
                preview_graph_path: Some(&preview_path),
                message_id: None,
            },
            version: NewVersion {
                workspace_id: &workspace_id,
                label: suffix,
                source: VersionSource::Proposal,
                graph_path: &graph_path,
                graph_hash: &graph_hash,
                parent_id: Some(&base_version_id),
            },
            message_text: suffix,
        })
        .await
}
