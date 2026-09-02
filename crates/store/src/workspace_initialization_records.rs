use super::{
    Store, StoreError, StoreResult, VersionRecord, VersionSource, WorkspaceRecord, new_id,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedWorkspaceIdentity {
    pub workspace_id: String,
    pub initial_version_id: String,
}

#[derive(Debug, Clone)]
pub struct CreateWorkspaceWithInitialVersion<'a> {
    pub identity: ReservedWorkspaceIdentity,
    pub name: &'a str,
    pub version_label: &'a str,
    pub source: VersionSource,
    pub graph_path: &'a str,
    pub graph_hash: &'a str,
}

#[derive(Debug, Clone)]
pub struct InitializedWorkspace {
    pub workspace: WorkspaceRecord,
    pub version: VersionRecord,
}

impl Store {
    pub fn reserve_workspace_identity(&self) -> ReservedWorkspaceIdentity {
        ReservedWorkspaceIdentity {
            workspace_id: new_id("ws"),
            initial_version_id: new_id("ver"),
        }
    }

    pub async fn create_workspace_with_initial_version(
        &self,
        input: CreateWorkspaceWithInitialVersion<'_>,
    ) -> StoreResult<InitializedWorkspace> {
        let mut tx = self.pool().begin().await?;
        let workspace_insert = sqlx::query(
            r#"
            INSERT INTO workspaces (id, name, created_at, updated_at)
            VALUES (?, ?, current_timestamp, current_timestamp)
            "#,
        )
        .bind(&input.identity.workspace_id)
        .bind(input.name)
        .execute(&mut *tx)
        .await?;
        require_one_row("insert_initial_workspace", workspace_insert.rows_affected())?;

        let canvas_insert = sqlx::query(
            r#"
            INSERT INTO canvas_snapshots (
                workspace_id, revision, nodes_json, viewport_json, updated_at
            ) VALUES (?, 0, '{}', NULL, current_timestamp)
            "#,
        )
        .bind(&input.identity.workspace_id)
        .execute(&mut *tx)
        .await?;
        require_one_row("insert_initial_canvas", canvas_insert.rows_affected())?;

        let version_insert = sqlx::query(
            r#"
            INSERT INTO versions (
                id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id, created_at
            )
            VALUES (?, ?, 1, ?, ?, ?, ?, NULL, current_timestamp)
            "#,
        )
        .bind(&input.identity.initial_version_id)
        .bind(&input.identity.workspace_id)
        .bind(input.version_label)
        .bind(input.source.as_str())
        .bind(input.graph_path)
        .bind(input.graph_hash)
        .execute(&mut *tx)
        .await?;
        require_one_row("insert_initial_version", version_insert.rows_affected())?;

        let current_update = sqlx::query(
            r#"
            UPDATE workspaces
            SET cur_version_id = ?, updated_at = current_timestamp
            WHERE id = ? AND cur_version_id IS NULL
            "#,
        )
        .bind(&input.identity.initial_version_id)
        .bind(&input.identity.workspace_id)
        .execute(&mut *tx)
        .await?;
        require_one_row(
            "set_initial_current_version",
            current_update.rows_affected(),
        )?;
        tx.commit().await?;

        let workspace = self.workspace(&input.identity.workspace_id).await?;
        let version = self.version(&input.identity.initial_version_id).await?;
        Ok(InitializedWorkspace { workspace, version })
    }
}

fn require_one_row(operation: &'static str, actual_rows: u64) -> StoreResult<()> {
    if actual_rows == 1 {
        return Ok(());
    }
    Err(StoreError::StatementInvariant {
        operation,
        expected_rows: 1,
        actual_rows,
    })
}
