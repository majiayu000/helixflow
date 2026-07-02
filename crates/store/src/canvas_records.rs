use serde::{Deserialize, Serialize};

use super::{Store, StoreError, StoreResult, new_id};

#[derive(Debug, Clone)]
pub struct NewCanvas<'a> {
    pub workspace_id: &'a str,
    pub title: &'a str,
    pub snapshot_path: &'a str,
    pub snapshot_hash: &'a str,
    pub current_version_id: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct UpdatedCanvasSnapshot<'a> {
    pub canvas_id: &'a str,
    pub seq: i64,
    pub snapshot_path: &'a str,
    pub snapshot_hash: &'a str,
    pub current_version_id: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct NewCanvasOp<'a> {
    pub canvas_id: &'a str,
    pub base_seq: i64,
    pub actor_json: &'a str,
    pub kind: &'a str,
    pub payload_json: &'a str,
    pub idempotency_key: &'a str,
}

#[derive(Debug, Clone)]
pub struct NewCanvasPresence<'a> {
    pub canvas_id: &'a str,
    pub actor_id: &'a str,
    pub cursor_json: Option<&'a str>,
    pub selection_json: Option<&'a str>,
    pub viewport_json: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct CanvasRecord {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub seq: i64,
    pub snapshot_path: String,
    pub snapshot_hash: String,
    pub current_version_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct CanvasOpRecord {
    pub id: String,
    pub canvas_id: String,
    pub seq: i64,
    pub base_seq: i64,
    pub actor_json: String,
    pub kind: String,
    pub payload_json: String,
    pub idempotency_key: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct CanvasPresenceRecord {
    pub canvas_id: String,
    pub actor_id: String,
    pub cursor_json: Option<String>,
    pub selection_json: Option<String>,
    pub viewport_json: Option<String>,
    pub updated_at: String,
}

impl Store {
    pub async fn create_canvas(&self, input: NewCanvas<'_>) -> StoreResult<CanvasRecord> {
        let id = new_id("canvas");
        sqlx::query(
            r#"
            INSERT INTO canvases (
                id, workspace_id, title, seq, snapshot_path, snapshot_hash,
                current_version_id, created_at, updated_at
            )
            VALUES (?, ?, ?, 0, ?, ?, ?, current_timestamp, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.title)
        .bind(input.snapshot_path)
        .bind(input.snapshot_hash)
        .bind(input.current_version_id)
        .execute(self.pool())
        .await?;

        self.canvas(&id).await
    }

    pub async fn canvas(&self, canvas_id: &str) -> StoreResult<CanvasRecord> {
        Ok(sqlx::query_as::<_, CanvasRecord>(
            r#"
            SELECT id, workspace_id, title, seq, snapshot_path, snapshot_hash,
                   current_version_id, created_at, updated_at
            FROM canvases
            WHERE id = ?
            "#,
        )
        .bind(canvas_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn workspace_canvas(&self, workspace_id: &str) -> StoreResult<Option<CanvasRecord>> {
        Ok(sqlx::query_as::<_, CanvasRecord>(
            r#"
            SELECT id, workspace_id, title, seq, snapshot_path, snapshot_hash,
                   current_version_id, created_at, updated_at
            FROM canvases
            WHERE workspace_id = ?
            ORDER BY created_at, id
            LIMIT 1
            "#,
        )
        .bind(workspace_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn update_canvas_snapshot(
        &self,
        input: UpdatedCanvasSnapshot<'_>,
    ) -> StoreResult<CanvasRecord> {
        sqlx::query(
            r#"
            UPDATE canvases
            SET seq = ?,
                snapshot_path = ?,
                snapshot_hash = ?,
                current_version_id = ?,
                updated_at = current_timestamp
            WHERE id = ?
            "#,
        )
        .bind(input.seq)
        .bind(input.snapshot_path)
        .bind(input.snapshot_hash)
        .bind(input.current_version_id)
        .bind(input.canvas_id)
        .execute(self.pool())
        .await?;

        self.canvas(input.canvas_id).await
    }

    pub async fn append_canvas_op(&self, input: NewCanvasOp<'_>) -> StoreResult<CanvasOpRecord> {
        if let Some(existing) = self
            .canvas_op_by_idempotency_key(input.canvas_id, input.idempotency_key)
            .await?
        {
            return Ok(existing);
        }

        let mut tx = self.pool().begin().await?;
        let current_seq: i64 = sqlx::query_scalar(
            r#"
            SELECT seq
            FROM canvases
            WHERE id = ?
            "#,
        )
        .bind(input.canvas_id)
        .fetch_one(&mut *tx)
        .await?;

        if input.base_seq > current_seq {
            return Err(StoreError::CanvasBaseSeqConflict {
                canvas_id: input.canvas_id.to_owned(),
                base_seq: input.base_seq,
                current_seq,
            });
        }

        let id = new_id("cop");
        let next_seq = current_seq + 1;
        sqlx::query(
            r#"
            INSERT INTO canvas_ops (
                id, canvas_id, seq, base_seq, actor_json, kind, payload_json,
                idempotency_key, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(input.canvas_id)
        .bind(next_seq)
        .bind(input.base_seq)
        .bind(input.actor_json)
        .bind(input.kind)
        .bind(input.payload_json)
        .bind(input.idempotency_key)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE canvases
            SET seq = ?, updated_at = current_timestamp
            WHERE id = ?
            "#,
        )
        .bind(next_seq)
        .bind(input.canvas_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        self.canvas_op(&id).await
    }

    pub async fn canvas_op(&self, op_id: &str) -> StoreResult<CanvasOpRecord> {
        Ok(sqlx::query_as::<_, CanvasOpRecord>(
            r#"
            SELECT id, canvas_id, seq, base_seq, actor_json, kind, payload_json,
                   idempotency_key, created_at
            FROM canvas_ops
            WHERE id = ?
            "#,
        )
        .bind(op_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn canvas_ops_after(
        &self,
        canvas_id: &str,
        after_seq: i64,
    ) -> StoreResult<Vec<CanvasOpRecord>> {
        Ok(sqlx::query_as::<_, CanvasOpRecord>(
            r#"
            SELECT id, canvas_id, seq, base_seq, actor_json, kind, payload_json,
                   idempotency_key, created_at
            FROM canvas_ops
            WHERE canvas_id = ? AND seq > ?
            ORDER BY seq
            "#,
        )
        .bind(canvas_id)
        .bind(after_seq)
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn upsert_canvas_presence(
        &self,
        input: NewCanvasPresence<'_>,
    ) -> StoreResult<CanvasPresenceRecord> {
        sqlx::query(
            r#"
            INSERT INTO canvas_presence (
                canvas_id, actor_id, cursor_json, selection_json, viewport_json, updated_at
            )
            VALUES (?, ?, ?, ?, ?, current_timestamp)
            ON CONFLICT(canvas_id, actor_id) DO UPDATE SET
                cursor_json = excluded.cursor_json,
                selection_json = excluded.selection_json,
                viewport_json = excluded.viewport_json,
                updated_at = current_timestamp
            "#,
        )
        .bind(input.canvas_id)
        .bind(input.actor_id)
        .bind(input.cursor_json)
        .bind(input.selection_json)
        .bind(input.viewport_json)
        .execute(self.pool())
        .await?;

        self.canvas_presence(input.canvas_id, input.actor_id).await
    }

    pub async fn canvas_presence(
        &self,
        canvas_id: &str,
        actor_id: &str,
    ) -> StoreResult<CanvasPresenceRecord> {
        Ok(sqlx::query_as::<_, CanvasPresenceRecord>(
            r#"
            SELECT canvas_id, actor_id, cursor_json, selection_json, viewport_json, updated_at
            FROM canvas_presence
            WHERE canvas_id = ? AND actor_id = ?
            "#,
        )
        .bind(canvas_id)
        .bind(actor_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn canvas_op_by_idempotency_key(
        &self,
        canvas_id: &str,
        idempotency_key: &str,
    ) -> StoreResult<Option<CanvasOpRecord>> {
        Ok(sqlx::query_as::<_, CanvasOpRecord>(
            r#"
            SELECT id, canvas_id, seq, base_seq, actor_json, kind, payload_json,
                   idempotency_key, created_at
            FROM canvas_ops
            WHERE canvas_id = ? AND idempotency_key = ?
            "#,
        )
        .bind(canvas_id)
        .bind(idempotency_key)
        .fetch_optional(self.pool())
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewVersion, VersionSource};

    async fn open_temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let db_path = dir.path().join("helixflow.sqlite");
        let database_url = format!("sqlite://{}", db_path.display());
        let store = Store::open(&database_url).await.expect("open store");
        (store, dir)
    }

    async fn workspace_with_version(store: &Store) -> (String, String) {
        let workspace = store
            .create_workspace("Canvas workspace")
            .await
            .expect("create workspace");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Initial graph",
                source: VersionSource::Manual,
                graph_path: "workspaces/ws_canvas/graphs/initial.json",
                graph_hash: "sha256:graph",
                parent_id: None,
            })
            .await
            .expect("create version");
        (workspace.id, version.id)
    }

    #[tokio::test]
    async fn creates_workspace_canvas_record() {
        let (store, _dir) = open_temp_store().await;
        let (workspace_id, version_id) = workspace_with_version(&store).await;

        let canvas = store
            .create_canvas(NewCanvas {
                workspace_id: &workspace_id,
                title: "Main canvas",
                snapshot_path: "workspaces/ws_canvas/canvas/main.json",
                snapshot_hash: "sha256:canvas",
                current_version_id: Some(&version_id),
            })
            .await
            .expect("create canvas");
        let loaded = store
            .workspace_canvas(&workspace_id)
            .await
            .expect("workspace canvas")
            .expect("canvas exists");

        assert_eq!(canvas.workspace_id, workspace_id);
        assert_eq!(canvas.seq, 0);
        assert_eq!(loaded.id, canvas.id);
        assert_eq!(
            loaded.current_version_id.as_deref(),
            Some(version_id.as_str())
        );
    }

    #[tokio::test]
    async fn appends_canvas_ops_with_server_sequence_and_idempotency() {
        let (store, _dir) = open_temp_store().await;
        let (workspace_id, version_id) = workspace_with_version(&store).await;
        let canvas = store
            .create_canvas(NewCanvas {
                workspace_id: &workspace_id,
                title: "Main canvas",
                snapshot_path: "workspaces/ws_canvas/canvas/main.json",
                snapshot_hash: "sha256:canvas",
                current_version_id: Some(&version_id),
            })
            .await
            .expect("create canvas");

        let op = store
            .append_canvas_op(NewCanvasOp {
                canvas_id: &canvas.id,
                base_seq: 0,
                actor_json: r#"{"id":"user_1","kind":"user"}"#,
                kind: "node_move",
                payload_json: r#"{"node_id":"node_1","position":{"x":1,"y":2}}"#,
                idempotency_key: "client_1",
            })
            .await
            .expect("append op");
        let repeated = store
            .append_canvas_op(NewCanvasOp {
                canvas_id: &canvas.id,
                base_seq: 0,
                actor_json: r#"{"id":"user_1","kind":"user"}"#,
                kind: "node_move",
                payload_json: r#"{"node_id":"node_1","position":{"x":1,"y":2}}"#,
                idempotency_key: "client_1",
            })
            .await
            .expect("idempotent append");
        let updated_canvas = store.canvas(&canvas.id).await.expect("canvas");

        assert_eq!(op.seq, 1);
        assert_eq!(repeated.id, op.id);
        assert_eq!(updated_canvas.seq, 1);
    }

    #[tokio::test]
    async fn rejects_canvas_ops_with_future_base_sequence() {
        let (store, _dir) = open_temp_store().await;
        let (workspace_id, version_id) = workspace_with_version(&store).await;
        let canvas = store
            .create_canvas(NewCanvas {
                workspace_id: &workspace_id,
                title: "Main canvas",
                snapshot_path: "workspaces/ws_canvas/canvas/main.json",
                snapshot_hash: "sha256:canvas",
                current_version_id: Some(&version_id),
            })
            .await
            .expect("create canvas");

        let err = store
            .append_canvas_op(NewCanvasOp {
                canvas_id: &canvas.id,
                base_seq: 2,
                actor_json: r#"{"id":"user_1","kind":"user"}"#,
                kind: "node_move",
                payload_json: r#"{"node_id":"node_1","position":{"x":1,"y":2}}"#,
                idempotency_key: "client_1",
            })
            .await
            .expect_err("future base seq should fail");

        assert!(matches!(
            err,
            StoreError::CanvasBaseSeqConflict {
                base_seq: 2,
                current_seq: 0,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn lists_canvas_ops_after_sequence_and_updates_presence() {
        let (store, _dir) = open_temp_store().await;
        let (workspace_id, version_id) = workspace_with_version(&store).await;
        let canvas = store
            .create_canvas(NewCanvas {
                workspace_id: &workspace_id,
                title: "Main canvas",
                snapshot_path: "workspaces/ws_canvas/canvas/main.json",
                snapshot_hash: "sha256:canvas",
                current_version_id: Some(&version_id),
            })
            .await
            .expect("create canvas");
        for index in 1..=2 {
            store
                .append_canvas_op(NewCanvasOp {
                    canvas_id: &canvas.id,
                    base_seq: index - 1,
                    actor_json: r#"{"id":"user_1","kind":"user"}"#,
                    kind: "run_request",
                    payload_json: r#"{"label":"Run"}"#,
                    idempotency_key: &format!("client_{index}"),
                })
                .await
                .expect("append op");
        }

        let ops = store
            .canvas_ops_after(&canvas.id, 1)
            .await
            .expect("ops after seq");
        let presence = store
            .upsert_canvas_presence(NewCanvasPresence {
                canvas_id: &canvas.id,
                actor_id: "user_1",
                cursor_json: Some(r#"{"x":1,"y":2}"#),
                selection_json: Some(r#"{"nodes":["node_1"]}"#),
                viewport_json: None,
            })
            .await
            .expect("upsert presence");

        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].seq, 2);
        assert_eq!(
            presence.selection_json.as_deref(),
            Some(r#"{"nodes":["node_1"]}"#)
        );
    }
}
