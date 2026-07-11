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
