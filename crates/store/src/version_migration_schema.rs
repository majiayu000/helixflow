use sqlx::{Connection, Executor, Row, SqlitePool};

use super::{StoreError, StoreResult};

pub(super) async fn ensure_migration_version_source(pool: &SqlitePool) -> StoreResult<()> {
    let schema_sql: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'versions'",
    )
    .fetch_one(pool)
    .await?;
    if schema_sql.contains("'migration'") {
        return Ok(());
    }

    let mut connection = pool.acquire().await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await?;

    let migration_result = rebuild_versions(&mut connection).await;
    let restore_result = sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *connection)
        .await
        .map(|_| ())
        .map_err(StoreError::from);

    match (migration_result, restore_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(migration_error), Err(cleanup_error)) => Err(StoreError::SchemaMigrationCleanup {
            migration_error: migration_error.to_string(),
            cleanup_error: cleanup_error.to_string(),
        }),
    }
}

async fn rebuild_versions(
    connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
) -> StoreResult<()> {
    let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
    tx.execute(
        r#"
        CREATE TABLE versions_v2 (
          id TEXT PRIMARY KEY,
          workspace_id TEXT NOT NULL,
          idx INTEGER NOT NULL,
          label TEXT NOT NULL,
          source TEXT NOT NULL CHECK (
            source IN ('manual', 'proposal', 'restore', 'migration')
          ),
          graph_path TEXT NOT NULL,
          graph_hash TEXT NOT NULL,
          parent_id TEXT,
          created_at TEXT NOT NULL,
          semantics_json TEXT,
          FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
          FOREIGN KEY (parent_id) REFERENCES versions_v2(id) ON DELETE SET NULL,
          UNIQUE (workspace_id, idx)
        )
        "#,
    )
    .await?;
    tx.execute(
        r#"
        INSERT INTO versions_v2 (
          id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
          created_at, semantics_json
        )
        SELECT
          id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
          created_at, semantics_json
        FROM versions
        "#,
    )
    .await?;
    tx.execute("DROP TABLE versions").await?;
    tx.execute("ALTER TABLE versions_v2 RENAME TO versions")
        .await?;
    tx.execute("CREATE INDEX idx_versions_workspace_id ON versions(workspace_id)")
        .await?;

    let violations: i64 = sqlx::query("SELECT COUNT(*) FROM pragma_foreign_key_check")
        .fetch_one(&mut *tx)
        .await?
        .try_get(0)?;
    if violations != 0 {
        return Err(StoreError::StatementInvariant {
            operation: "version_migration_foreign_key_check",
            expected_rows: 0,
            actual_rows: violations as u64,
        });
    }
    tx.commit().await?;
    Ok(())
}
