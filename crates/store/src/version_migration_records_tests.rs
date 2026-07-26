use super::{
    ApplyVersionMigration, NewVersion, NewVersionMigrationAssessment, Store, StoreError,
    VersionSource,
};

async fn fixture() -> (Store, tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().expect("temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("store.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Migration")
        .await
        .expect("workspace");
    let source = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Legacy",
            source: VersionSource::Manual,
            graph_path: "workspaces/ws/graphs/legacy.json",
            graph_hash: "sha256:legacy",
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("source");
    (store, dir, workspace.id, source.id)
}

fn migration_input<'a>(
    workspace_id: &'a str,
    source_version_id: &'a str,
    fingerprint: &'a str,
) -> ApplyVersionMigration<'a> {
    ApplyVersionMigration {
        workspace_id,
        operation_id: "op-stable",
        operation_fingerprint: fingerprint,
        source_version_id,
        source_graph_hash: "sha256:legacy",
        expected_runtime_provider_id: None,
        target_label: "Migrated v2",
        target_graph_path: "workspaces/ws/graphs/migrated.json",
        target_graph_hash: "sha256:migrated",
        target_semantics_json: r#"{"image":{"capabilityId":"text_to_image"}}"#,
        migration_version: "1",
        catalog_revision: "catalog-1",
        workspace_connector_id: "atlas",
        report_hash: "sha256:report",
        report_json: r#"{"status":"migratable"}"#,
    }
}

#[tokio::test]
async fn assessment_is_append_only_and_secret_free_shape() {
    let (store, _dir, workspace_id, source_version_id) = fixture().await;
    let input = || NewVersionMigrationAssessment {
        workspace_id: &workspace_id,
        source_version_id: &source_version_id,
        source_graph_hash: "sha256:legacy",
        migration_version: "1",
        catalog_revision: "catalog-1",
        workspace_connector_id: "atlas",
        status: "needs_resolution",
        top_level_code: None,
        reason_code_counts_json: r#"{"MODEL_AMBIGUOUS":1}"#,
    };

    let first = store
        .record_version_migration_assessment(input())
        .await
        .expect("first assessment");
    let second = store
        .record_version_migration_assessment(input())
        .await
        .expect("second assessment");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM version_migration_assessments WHERE workspace_id = ?",
    )
    .bind(&workspace_id)
    .fetch_one(store.pool())
    .await
    .expect("assessment count");

    assert_ne!(first.id, second.id);
    assert_eq!(count, 2);
    assert_eq!(first.reason_code_counts_json, r#"{"MODEL_AMBIGUOUS":1}"#);
}

#[tokio::test]
async fn apply_is_atomic_and_same_operation_replays() {
    let (store, _dir, workspace_id, source_version_id) = fixture().await;
    let first = store
        .apply_version_migration(migration_input(
            &workspace_id,
            &source_version_id,
            "sha256:fingerprint",
        ))
        .await
        .expect("apply");
    let replay = store
        .apply_version_migration(migration_input(
            &workspace_id,
            &source_version_id,
            "sha256:fingerprint",
        ))
        .await
        .expect("replay");

    assert!(!first.replayed);
    assert!(replay.replayed);
    assert_eq!(first.target_version.id, replay.target_version.id);
    assert_eq!(first.target_version.source, "migration");
    assert_eq!(
        store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id,
        Some(first.target_version.id.clone())
    );
    assert_eq!(
        store
            .version(&source_version_id)
            .await
            .expect("source")
            .semantics_json,
        None
    );

    let error = store
        .apply_version_migration(migration_input(
            &workspace_id,
            &source_version_id,
            "sha256:different",
        ))
        .await
        .expect_err("different fingerprint");
    assert!(matches!(error, StoreError::OperationIdConflict { .. }));
}

#[tokio::test]
async fn competing_operations_only_advance_current_once() {
    let (store, _dir, workspace_id, source_version_id) = fixture().await;
    let apply = |operation_id: &'static str, graph_hash: &'static str| {
        let store = store.clone();
        let workspace_id = workspace_id.clone();
        let source_version_id = source_version_id.clone();
        async move {
            store
                .apply_version_migration(ApplyVersionMigration {
                    workspace_id: &workspace_id,
                    operation_id,
                    operation_fingerprint: operation_id,
                    source_version_id: &source_version_id,
                    source_graph_hash: "sha256:legacy",
                    expected_runtime_provider_id: None,
                    target_label: "Migrated v2",
                    target_graph_path: graph_hash,
                    target_graph_hash: graph_hash,
                    target_semantics_json: "{}",
                    migration_version: "1",
                    catalog_revision: "catalog-1",
                    workspace_connector_id: "atlas",
                    report_hash: "sha256:report",
                    report_json: "{}",
                })
                .await
        }
    };

    let (left, right) = tokio::join!(
        apply("op-left", "sha256:left"),
        apply("op-right", "sha256:right")
    );

    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let loser = if left.is_err() { left } else { right };
    assert!(matches!(loser, Err(StoreError::VersionConflict { .. })));
}

#[tokio::test]
async fn apply_rejects_connector_switch_without_creating_a_target() {
    let (store, _dir, workspace_id, source_version_id) = fixture().await;
    store
        .set_workspace_runtime_provider(&workspace_id, Some("atlas"))
        .await
        .expect("switch connector");

    let error = store
        .apply_version_migration(migration_input(
            &workspace_id,
            &source_version_id,
            "sha256:connector-switch",
        ))
        .await
        .expect_err("stale connector must fail");
    assert!(matches!(
        error,
        StoreError::WorkspaceConnectorConflict { .. }
    ));
    assert_eq!(
        store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions")
            .len(),
        1
    );
}
