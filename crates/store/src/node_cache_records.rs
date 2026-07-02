use serde::{Deserialize, Serialize};

use super::{Store, StoreError, StoreResult, new_id};

#[derive(Debug, Clone)]
pub struct NewNodeCacheEntry<'a> {
    pub workspace_id: &'a str,
    pub provider: &'a str,
    pub node_type: &'a str,
    pub node_id: &'a str,
    pub cache_key: &'a str,
    pub input_hash_json: &'a str,
    pub artifact_ids_json: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct NodeCacheEntryRecord {
    pub id: String,
    pub workspace_id: String,
    pub provider: String,
    pub node_type: String,
    pub node_id: String,
    pub cache_key: String,
    pub input_hash_json: String,
    pub artifact_ids_json: String,
    pub created_at: String,
    pub last_hit_at: Option<String>,
}

impl Store {
    pub async fn node_cache_entry(
        &self,
        workspace_id: &str,
        provider: &str,
        node_type: &str,
        node_id: &str,
        cache_key: &str,
    ) -> StoreResult<Option<NodeCacheEntryRecord>> {
        Ok(sqlx::query_as::<_, NodeCacheEntryRecord>(
            r#"
            SELECT id, workspace_id, provider, node_type, node_id, cache_key,
                   input_hash_json, artifact_ids_json, created_at, last_hit_at
            FROM node_cache_entries
            WHERE workspace_id = ?
              AND provider = ?
              AND node_type = ?
              AND node_id = ?
              AND cache_key = ?
            "#,
        )
        .bind(workspace_id)
        .bind(provider)
        .bind(node_type)
        .bind(node_id)
        .bind(cache_key)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn upsert_node_cache_entry(
        &self,
        input: NewNodeCacheEntry<'_>,
    ) -> StoreResult<NodeCacheEntryRecord> {
        let existing_id: Option<String> = sqlx::query_scalar(
            r#"
            SELECT id
            FROM node_cache_entries
            WHERE workspace_id = ?
              AND provider = ?
              AND node_type = ?
              AND node_id = ?
              AND cache_key = ?
            "#,
        )
        .bind(input.workspace_id)
        .bind(input.provider)
        .bind(input.node_type)
        .bind(input.node_id)
        .bind(input.cache_key)
        .fetch_optional(self.pool())
        .await?;
        let id = existing_id.unwrap_or_else(|| new_id("cache"));
        sqlx::query(
            r#"
            INSERT INTO node_cache_entries (
                id, workspace_id, provider, node_type, node_id, cache_key,
                input_hash_json, artifact_ids_json, created_at, last_hit_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, current_timestamp, NULL)
            ON CONFLICT(workspace_id, provider, node_type, node_id, cache_key)
            DO UPDATE SET
                input_hash_json = excluded.input_hash_json,
                artifact_ids_json = excluded.artifact_ids_json,
                last_hit_at = NULL
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.provider)
        .bind(input.node_type)
        .bind(input.node_id)
        .bind(input.cache_key)
        .bind(input.input_hash_json)
        .bind(input.artifact_ids_json)
        .execute(self.pool())
        .await?;

        self.node_cache_entry(
            input.workspace_id,
            input.provider,
            input.node_type,
            input.node_id,
            input.cache_key,
        )
        .await?
        .ok_or_else(|| StoreError::Sqlx(sqlx::Error::RowNotFound))
    }

    pub async fn touch_node_cache_entry(
        &self,
        entry_id: &str,
    ) -> StoreResult<NodeCacheEntryRecord> {
        sqlx::query(
            r#"
            UPDATE node_cache_entries
            SET last_hit_at = current_timestamp
            WHERE id = ?
            "#,
        )
        .bind(entry_id)
        .execute(self.pool())
        .await?;

        Ok(sqlx::query_as::<_, NodeCacheEntryRecord>(
            r#"
            SELECT id, workspace_id, provider, node_type, node_id, cache_key,
                   input_hash_json, artifact_ids_json, created_at, last_hit_at
            FROM node_cache_entries
            WHERE id = ?
            "#,
        )
        .bind(entry_id)
        .fetch_one(self.pool())
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn open_temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let db_path = dir.path().join("helixflow.sqlite");
        let database_url = format!("sqlite://{}", db_path.display());
        let store = Store::open(&database_url).await.expect("open store");
        (store, dir)
    }

    #[tokio::test]
    async fn node_cache_entries_are_scoped_by_workspace_and_upsertable() {
        let (store, _dir) = open_temp_store().await;
        let first = store
            .create_workspace("First cache workspace")
            .await
            .expect("first workspace");
        let second = store
            .create_workspace("Second cache workspace")
            .await
            .expect("second workspace");

        let entry = store
            .upsert_node_cache_entry(NewNodeCacheEntry {
                workspace_id: &first.id,
                provider: "mock",
                node_type: "image.generate",
                node_id: "image",
                cache_key: "sha256:first",
                input_hash_json: r#"{"prompt":"sha256:input"}"#,
                artifact_ids_json: r#"[{"port":"image","artifact_id":"art_1"}]"#,
            })
            .await
            .expect("insert cache entry");
        let updated = store
            .upsert_node_cache_entry(NewNodeCacheEntry {
                workspace_id: &first.id,
                provider: "mock",
                node_type: "image.generate",
                node_id: "image",
                cache_key: "sha256:first",
                input_hash_json: r#"{"prompt":"sha256:input"}"#,
                artifact_ids_json: r#"[{"port":"image","artifact_id":"art_2"}]"#,
            })
            .await
            .expect("update cache entry");
        let missing = store
            .node_cache_entry(
                &second.id,
                "mock",
                "image.generate",
                "image",
                "sha256:first",
            )
            .await
            .expect("second lookup");
        let touched = store
            .touch_node_cache_entry(&updated.id)
            .await
            .expect("touch cache entry");

        assert_eq!(updated.id, entry.id);
        assert_eq!(
            updated.artifact_ids_json,
            r#"[{"port":"image","artifact_id":"art_2"}]"#
        );
        assert!(missing.is_none());
        assert!(touched.last_hit_at.is_some());
    }
}
