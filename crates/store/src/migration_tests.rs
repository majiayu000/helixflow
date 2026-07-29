use std::borrow::Cow;

use sqlx::migrate::Migrator;

use super::{Store, connect_pool};

#[tokio::test]
async fn upgrades_pre_0004_database_and_backfills_historical_data() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("pre-0004.sqlite");
    let database_url = format!("sqlite://{}", db_path.display());
    let pool = connect_pool(&database_url)
        .await
        .expect("connect old store");
    let all = sqlx::migrate!("./migrations");
    let prior = Migrator {
        migrations: Cow::Owned(all.iter().take(3).cloned().collect()),
        ..Migrator::DEFAULT
    };
    prior
        .run(&pool)
        .await
        .expect("apply migrations through 0003");

    sqlx::query(
        r#"
        INSERT INTO workspaces (id, name, cur_version_id, created_at, updated_at)
        VALUES ('ws_old', 'Old', 'ver_old', current_timestamp, current_timestamp);
        INSERT INTO versions (
            id, workspace_id, idx, label, source, graph_path, graph_hash, created_at
        ) VALUES (
            'ver_old', 'ws_old', 1, 'Old graph', 'manual', 'old.json', 'sha256:old', current_timestamp
        );
        INSERT INTO runs (
            id, workspace_id, version_id, label, trigger, status, created_at
        ) VALUES (
            'run_old', 'ws_old', 'ver_old', 'Old run', 'manual', 'succeeded', current_timestamp
        );
        INSERT INTO artifacts (
            id, workspace_id, run_id, kind, storage_uri, selected, created_at
        ) VALUES (
            'art_old', 'ws_old', 'run_old', 'video', 'artifacts/old.mp4', 1, current_timestamp
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed historical data");
    pool.close().await;

    let store = Store::open(&database_url).await.expect("upgrade to 0004");
    let run = store.run("run_old").await.expect("historical run");
    let artifact = store
        .artifact("art_old")
        .await
        .expect("historical artifact");
    assert_eq!(run.attempt, 0);
    assert!(!run.force_rerun);
    assert_eq!(artifact.review_state, "accepted");
    drop(store);

    let reopened = Store::open(&database_url)
        .await
        .expect("reopen upgraded store");
    assert_eq!(
        reopened
            .artifact("art_old")
            .await
            .expect("artifact after restart")
            .review_state,
        "accepted"
    );
}

#[tokio::test]
async fn version_migration_rebuild_preserves_old_rows_semantics_and_foreign_keys() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("pre-0007.sqlite");
    let database_url = format!("sqlite://{}", db_path.display());
    let pool = connect_pool(&database_url)
        .await
        .expect("connect old store");
    let all = sqlx::migrate!("./migrations");
    let prior = Migrator {
        migrations: Cow::Owned(all.iter().take(6).cloned().collect()),
        ..Migrator::DEFAULT
    };
    prior.run(&pool).await.expect("apply through 0006");

    sqlx::query(
        r#"
        INSERT INTO workspaces (
            id, name, cur_version_id, runtime_provider_id, created_at, updated_at
        ) VALUES (
            'ws_old', 'Old', 'ver_child', 'atlas', current_timestamp, current_timestamp
        );
        INSERT INTO versions (
            id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
            created_at, semantics_json
        ) VALUES (
            'ver_parent', 'ws_old', 1, 'Parent', 'manual', 'parent.json', 'sha256:parent',
            NULL, current_timestamp, NULL
        ), (
            'ver_child', 'ws_old', 2, 'Child', 'proposal', 'child.json', 'sha256:child',
            'ver_parent', current_timestamp, '{"node":{"capabilityId":"text_to_image"}}'
        );
        INSERT INTO runs (
            id, workspace_id, version_id, label, trigger, status, attempt, force_rerun,
            created_at
        ) VALUES (
            'run_old', 'ws_old', 'ver_child', 'Old run', 'manual', 'succeeded', 0, 0,
            current_timestamp
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed old rows");
    pool.close().await;

    let store = Store::open(&database_url)
        .await
        .expect("upgrade through 0007");
    let child = store.version("ver_child").await.expect("child");
    let run = store.run("run_old").await.expect("run");
    let foreign_key_violations: Vec<(String, i64, String, i64)> =
        sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(store.pool())
            .await
            .expect("foreign key check");
    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'versions'",
    )
    .fetch_all(store.pool())
    .await
    .expect("indexes");

    assert_eq!(child.parent_id.as_deref(), Some("ver_parent"));
    assert!(
        child
            .semantics_json
            .as_deref()
            .is_some_and(|value| { value.contains("\"capabilityId\":\"text_to_image\"") })
    );
    assert_eq!(run.version_id, "ver_child");
    assert!(foreign_key_violations.is_empty());
    assert!(
        indexes
            .iter()
            .any(|name| name == "idx_versions_workspace_id")
    );

    let migrated = store
        .create_version(super::NewVersion {
            workspace_id: "ws_old",
            label: "Migration",
            source: super::VersionSource::Migration,
            graph_path: "migration.json",
            graph_hash: "sha256:migration",
            parent_id: Some("ver_child"),
            semantics_json: Some("{}"),
        })
        .await
        .expect("insert migration source");
    assert_eq!(migrated.source, "migration");
}

#[tokio::test]
async fn run_fix_migration_preserves_every_gh154_continuation_state() {
    let dir = tempfile::tempdir().expect("temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("pre-0009.sqlite").display());
    let pool = connect_pool(&database_url)
        .await
        .expect("connect old store");
    let all = sqlx::migrate!("./migrations");
    let prior = Migrator {
        migrations: Cow::Owned(all.iter().take(8).cloned().collect()),
        ..Migrator::DEFAULT
    };
    prior.run(&pool).await.expect("apply through 0008");
    sqlx::query(
        r#"
        INSERT INTO workspaces (id, name, cur_version_id, created_at, updated_at)
        VALUES ('ws_fix', 'Fix', 'ver_fix', current_timestamp, current_timestamp);
        INSERT INTO versions (
            id, workspace_id, idx, label, source, graph_path, graph_hash, created_at
        ) VALUES (
            'ver_fix', 'ws_fix', 1, 'Fix', 'manual', 'fix.json', 'sha256:fix',
            current_timestamp
        );
        INSERT INTO runs (
            id, workspace_id, version_id, label, trigger, status, created_at
        ) VALUES
            ('run_pending', 'ws_fix', 'ver_fix', 'Pending', 'agent', 'failed', current_timestamp),
            ('run_retry', 'ws_fix', 'ver_fix', 'Retry', 'agent', 'failed', current_timestamp),
            ('run_exhausted', 'ws_fix', 'ver_fix', 'Exhausted', 'agent', 'failed', current_timestamp),
            ('run_completed', 'ws_fix', 'ver_fix', 'Completed', 'agent', 'failed', current_timestamp),
            ('run_child', 'ws_fix', 'ver_fix', 'Child', 'agent', 'waiting_confirmation', current_timestamp);
        INSERT INTO run_failure_continuations (
            run_id, state, retry_key, child_run_id, created_at, updated_at
        ) VALUES
            ('run_pending', 'pending', 'retry:pending', NULL, current_timestamp, current_timestamp),
            ('run_retry', 'retry_created', 'retry:created', 'run_child', current_timestamp, current_timestamp),
            ('run_exhausted', 'exhausted', 'retry:exhausted', NULL, current_timestamp, current_timestamp),
            ('run_completed', 'completed', 'retry:completed', NULL, current_timestamp, current_timestamp)
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed GH154 continuations");
    pool.close().await;

    let store = Store::open(&database_url).await.expect("upgrade to 0009");
    for (run_id, state, child) in [
        ("run_pending", "pending", None),
        ("run_retry", "retry_created", Some("run_child")),
        ("run_exhausted", "exhausted", None),
        ("run_completed", "completed", None),
    ] {
        let record = store
            .failure_continuation(run_id)
            .await
            .expect("read continuation")
            .expect("preserved continuation");
        assert_eq!(record.state, state);
        assert_eq!(record.child_run_id.as_deref(), child);
        assert!(record.chain_id.is_none());
    }
    let foreign_key_violations: Vec<(String, i64, String, i64)> =
        sqlx::query_as("PRAGMA foreign_key_check")
            .fetch_all(store.pool())
            .await
            .expect("foreign key check");
    assert!(foreign_key_violations.is_empty());
}
