use super::{NewVersion, Store, StoreError, StoreResult, VersionRecord, VersionSource, new_id};

#[derive(Debug, Clone)]
pub struct NewVersionMigrationAssessment<'a> {
    pub workspace_id: &'a str,
    pub source_version_id: &'a str,
    pub source_graph_hash: &'a str,
    pub migration_version: &'a str,
    pub catalog_revision: &'a str,
    pub workspace_connector_id: &'a str,
    pub status: &'a str,
    pub top_level_code: Option<&'a str>,
    pub reason_code_counts_json: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct VersionMigrationAssessmentRecord {
    pub id: String,
    pub workspace_id: String,
    pub source_version_id: String,
    pub source_graph_hash: String,
    pub migration_version: String,
    pub catalog_revision: String,
    pub workspace_connector_id: String,
    pub status: String,
    pub top_level_code: Option<String>,
    pub reason_code_counts_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ApplyVersionMigration<'a> {
    pub operation_id: &'a str,
    pub operation_fingerprint: &'a str,
    pub source_version_id: &'a str,
    pub source_graph_hash: &'a str,
    pub expected_runtime_provider_id: Option<&'a str>,
    pub target_label: &'a str,
    pub target_graph_path: &'a str,
    pub target_graph_hash: &'a str,
    pub target_semantics_json: &'a str,
    pub migration_version: &'a str,
    pub catalog_revision: &'a str,
    pub workspace_connector_id: &'a str,
    pub report_hash: &'a str,
    pub report_json: &'a str,
    pub workspace_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct VersionMigrationRecord {
    pub id: String,
    pub workspace_id: String,
    pub operation_id: String,
    pub operation_fingerprint: String,
    pub source_version_id: String,
    pub source_graph_hash: String,
    pub target_version_id: String,
    pub target_graph_hash: String,
    pub migration_version: String,
    pub catalog_revision: String,
    pub workspace_connector_id: String,
    pub report_hash: String,
    pub report_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedVersionMigration {
    pub migration: VersionMigrationRecord,
    pub target_version: VersionRecord,
    pub replayed: bool,
}

impl Store {
    pub async fn record_version_migration_assessment(
        &self,
        input: NewVersionMigrationAssessment<'_>,
    ) -> StoreResult<VersionMigrationAssessmentRecord> {
        let id = new_id("vma");
        sqlx::query(
            r#"
            INSERT INTO version_migration_assessments (
                id, workspace_id, source_version_id, source_graph_hash, migration_version,
                catalog_revision, workspace_connector_id, status, top_level_code,
                reason_code_counts_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.source_version_id)
        .bind(input.source_graph_hash)
        .bind(input.migration_version)
        .bind(input.catalog_revision)
        .bind(input.workspace_connector_id)
        .bind(input.status)
        .bind(input.top_level_code)
        .bind(input.reason_code_counts_json)
        .execute(self.pool())
        .await?;

        self.version_migration_assessment(&id).await
    }

    pub async fn version_migration_assessment(
        &self,
        id: &str,
    ) -> StoreResult<VersionMigrationAssessmentRecord> {
        Ok(sqlx::query_as(
            r#"
            SELECT id, workspace_id, source_version_id, source_graph_hash, migration_version,
                   catalog_revision, workspace_connector_id, status, top_level_code,
                   reason_code_counts_json, created_at
            FROM version_migration_assessments
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn version_migration_by_operation(
        &self,
        workspace_id: &str,
        operation_id: &str,
    ) -> StoreResult<Option<VersionMigrationRecord>> {
        Ok(sqlx::query_as(
            r#"
            SELECT id, workspace_id, operation_id, operation_fingerprint, source_version_id,
                   source_graph_hash, target_version_id, target_graph_hash, migration_version,
                   catalog_revision, workspace_connector_id, report_hash, report_json, created_at
            FROM version_migrations
            WHERE workspace_id = ? AND operation_id = ?
            "#,
        )
        .bind(workspace_id)
        .bind(operation_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn apply_version_migration(
        &self,
        input: ApplyVersionMigration<'_>,
    ) -> StoreResult<AppliedVersionMigration> {
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) =
            find_operation(&mut tx, input.workspace_id, input.operation_id).await?
        {
            if existing.operation_fingerprint != input.operation_fingerprint {
                return Err(StoreError::OperationIdConflict {
                    workspace_id: input.workspace_id.to_owned(),
                    operation_id: input.operation_id.to_owned(),
                });
            }
            let target_version = version_in_tx(&mut tx, &existing.target_version_id).await?;
            tx.commit().await?;
            return Ok(AppliedVersionMigration {
                migration: existing,
                target_version,
                replayed: true,
            });
        }

        let target_version_id = new_id("ver");
        let migration_id = new_id("vmg");
        let idx: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(idx), 0) + 1 FROM versions WHERE workspace_id = ?",
        )
        .bind(input.workspace_id)
        .fetch_one(&mut *tx)
        .await?;

        insert_target_version(&mut tx, &input, &target_version_id, idx).await?;
        let current_update = sqlx::query(
            r#"
            UPDATE workspaces
            SET cur_version_id = ?, updated_at = current_timestamp
            WHERE id = ? AND cur_version_id = ? AND runtime_provider_id IS ?
              AND NOT EXISTS (
                  SELECT 1
                  FROM proposals
                  WHERE workspace_id = workspaces.id AND state = 'pending'
              )
            "#,
        )
        .bind(&target_version_id)
        .bind(input.workspace_id)
        .bind(input.source_version_id)
        .bind(input.expected_runtime_provider_id)
        .execute(&mut *tx)
        .await?;
        if current_update.rows_affected() != 1 {
            let actual: Option<(Option<String>, Option<String>, bool)> = sqlx::query_as(
                r#"
                SELECT
                    cur_version_id,
                    runtime_provider_id,
                    EXISTS (
                        SELECT 1
                        FROM proposals
                        WHERE workspace_id = workspaces.id AND state = 'pending'
                    )
                FROM workspaces
                WHERE id = ?
                "#,
            )
            .bind(input.workspace_id)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some((actual_version_id, actual_connector_id, has_pending_proposal)) = actual {
                if actual_version_id.as_deref() != Some(input.source_version_id) {
                    return Err(StoreError::VersionConflict {
                        workspace_id: input.workspace_id.to_owned(),
                        expected_version_id: input.source_version_id.to_owned(),
                        actual_version_id,
                    });
                }
                if actual_connector_id.as_deref() != input.expected_runtime_provider_id {
                    return Err(StoreError::WorkspaceConnectorConflict {
                        workspace_id: input.workspace_id.to_owned(),
                        expected_connector_id: input
                            .expected_runtime_provider_id
                            .map(str::to_owned),
                        actual_connector_id,
                    });
                }
                if has_pending_proposal {
                    return Err(StoreError::PendingProposalConflict {
                        workspace_id: input.workspace_id.to_owned(),
                    });
                }
                return Err(StoreError::StatementInvariant {
                    operation: "apply_version_migration_atomic_predicate",
                    expected_rows: 1,
                    actual_rows: 0,
                });
            }
            return Err(StoreError::VersionConflict {
                workspace_id: input.workspace_id.to_owned(),
                expected_version_id: input.source_version_id.to_owned(),
                actual_version_id: None,
            });
        }

        sqlx::query(
            r#"
            INSERT INTO version_migrations (
                id, workspace_id, operation_id, operation_fingerprint, source_version_id,
                source_graph_hash, target_version_id, target_graph_hash, migration_version,
                catalog_revision, workspace_connector_id, report_hash, report_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&migration_id)
        .bind(input.workspace_id)
        .bind(input.operation_id)
        .bind(input.operation_fingerprint)
        .bind(input.source_version_id)
        .bind(input.source_graph_hash)
        .bind(&target_version_id)
        .bind(input.target_graph_hash)
        .bind(input.migration_version)
        .bind(input.catalog_revision)
        .bind(input.workspace_connector_id)
        .bind(input.report_hash)
        .bind(input.report_json)
        .execute(&mut *tx)
        .await?;

        let migration = find_operation(&mut tx, input.workspace_id, input.operation_id)
            .await?
            .ok_or(StoreError::StatementInvariant {
                operation: "read_inserted_version_migration",
                expected_rows: 1,
                actual_rows: 0,
            })?;
        let target_version = version_in_tx(&mut tx, &target_version_id).await?;
        tx.commit().await?;
        Ok(AppliedVersionMigration {
            migration,
            target_version,
            replayed: false,
        })
    }
}

async fn find_operation(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    workspace_id: &str,
    operation_id: &str,
) -> StoreResult<Option<VersionMigrationRecord>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, workspace_id, operation_id, operation_fingerprint, source_version_id,
               source_graph_hash, target_version_id, target_graph_hash, migration_version,
               catalog_revision, workspace_connector_id, report_hash, report_json, created_at
        FROM version_migrations
        WHERE workspace_id = ? AND operation_id = ?
        "#,
    )
    .bind(workspace_id)
    .bind(operation_id)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn insert_target_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    input: &ApplyVersionMigration<'_>,
    target_version_id: &str,
    idx: i64,
) -> StoreResult<()> {
    let version = NewVersion {
        workspace_id: input.workspace_id,
        label: input.target_label,
        source: VersionSource::Migration,
        graph_path: input.target_graph_path,
        graph_hash: input.target_graph_hash,
        parent_id: Some(input.source_version_id),
        semantics_json: Some(input.target_semantics_json),
    };
    sqlx::query(
        r#"
        INSERT INTO versions (
            id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
            semantics_json, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
        "#,
    )
    .bind(target_version_id)
    .bind(version.workspace_id)
    .bind(idx)
    .bind(version.label)
    .bind(version.source.as_str())
    .bind(version.graph_path)
    .bind(version.graph_hash)
    .bind(version.parent_id)
    .bind(version.semantics_json)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn version_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    version_id: &str,
) -> StoreResult<VersionRecord> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
               semantics_json, created_at
        FROM versions
        WHERE id = ?
        "#,
    )
    .bind(version_id)
    .fetch_one(&mut **tx)
    .await?)
}
