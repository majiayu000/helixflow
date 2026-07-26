use serde::{Deserialize, Serialize};

use super::{Store, StoreResult, VersionRecord};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct ProposalFileReference {
    pub proposal_id: String,
    pub workspace_id: String,
    pub ops_path: String,
    pub preview_graph_path: Option<String>,
}

impl Store {
    pub async fn version_file_references(
        &self,
        relative_path: &str,
    ) -> StoreResult<Vec<VersionRecord>> {
        Ok(sqlx::query_as::<_, VersionRecord>(
            r#"
            SELECT id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
                   semantics_json, created_at
            FROM versions
            WHERE graph_path = ?
            ORDER BY workspace_id, idx, id
            "#,
        )
        .bind(relative_path)
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn proposal_file_references(
        &self,
        relative_path: &str,
    ) -> StoreResult<Vec<ProposalFileReference>> {
        Ok(sqlx::query_as::<_, ProposalFileReference>(
            r#"
            SELECT id AS proposal_id, workspace_id, ops_path, preview_graph_path
            FROM proposals
            WHERE ops_path = ? OR preview_graph_path = ?
            ORDER BY workspace_id, id
            "#,
        )
        .bind(relative_path)
        .bind(relative_path)
        .fetch_all(self.pool())
        .await?)
    }
}
