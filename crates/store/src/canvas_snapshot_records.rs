use serde::{Deserialize, Serialize};

use super::{Store, StoreResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct CanvasSnapshotRecord {
    pub workspace_id: String,
    pub revision: i64,
    pub nodes_json: String,
    pub viewport_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommitCanvasSnapshot<'a> {
    pub workspace_id: &'a str,
    pub expected_revision: i64,
    pub nodes_json: &'a str,
    pub viewport_json: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasSnapshotCommitResult {
    Applied(CanvasSnapshotRecord),
    Stale { current_revision: i64 },
}

impl Store {
    pub async fn canvas_snapshot(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Option<CanvasSnapshotRecord>> {
        Ok(sqlx::query_as::<_, CanvasSnapshotRecord>(
            r#"
            SELECT workspace_id, revision, nodes_json, viewport_json
            FROM canvas_snapshots
            WHERE workspace_id = ?
            "#,
        )
        .bind(workspace_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn commit_canvas_snapshot(
        &self,
        input: CommitCanvasSnapshot<'_>,
    ) -> StoreResult<CanvasSnapshotCommitResult> {
        let next_revision = input.expected_revision + 1;
        let committed = sqlx::query(
            r#"
            INSERT INTO canvas_snapshots (
                workspace_id, revision, nodes_json, viewport_json, updated_at
            )
            SELECT id, ?, ?, ?, current_timestamp
            FROM workspaces
            WHERE id = ?
            ON CONFLICT(workspace_id) DO UPDATE SET
                revision = excluded.revision,
                nodes_json = excluded.nodes_json,
                viewport_json = excluded.viewport_json,
                updated_at = current_timestamp
            WHERE canvas_snapshots.revision = ?
            "#,
        )
        .bind(next_revision)
        .bind(input.nodes_json)
        .bind(input.viewport_json)
        .bind(input.workspace_id)
        .bind(input.expected_revision)
        .execute(self.pool())
        .await?;
        if committed.rows_affected() == 0 {
            let current_revision = sqlx::query_scalar::<_, i64>(
                "SELECT revision FROM canvas_snapshots WHERE workspace_id = ?",
            )
            .bind(input.workspace_id)
            .fetch_optional(self.pool())
            .await?
            .unwrap_or(0);
            return Ok(CanvasSnapshotCommitResult::Stale { current_revision });
        }
        let committed = sqlx::query_as::<_, CanvasSnapshotRecord>(
            r#"
            SELECT workspace_id, revision, nodes_json, viewport_json
            FROM canvas_snapshots
            WHERE workspace_id = ?
            "#,
        )
        .bind(input.workspace_id)
        .fetch_one(self.pool())
        .await?;
        Ok(CanvasSnapshotCommitResult::Applied(committed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn canvas_snapshot_commit_uses_revision_cas() {
        let dir = tempfile::tempdir().expect("temp dir");
        let database_url = format!("sqlite://{}", dir.path().join("store.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store.create_workspace("Canvas").await.expect("workspace");
        assert_eq!(
            store
                .canvas_snapshot(&workspace.id)
                .await
                .expect("initial snapshot")
                .expect("snapshot row")
                .revision,
            0
        );

        let first = store
            .commit_canvas_snapshot(CommitCanvasSnapshot {
                workspace_id: &workspace.id,
                expected_revision: 0,
                nodes_json: r#"{"node":{"position":[1.0,2.0]}}"#,
                viewport_json: Some(r#"{"x":0.0,"y":0.0,"zoom":1.0}"#),
            })
            .await
            .expect("first commit");
        assert!(
            matches!(first, CanvasSnapshotCommitResult::Applied(record) if record.revision == 1)
        );

        let stale = store
            .commit_canvas_snapshot(CommitCanvasSnapshot {
                workspace_id: &workspace.id,
                expected_revision: 0,
                nodes_json: "{}",
                viewport_json: None,
            })
            .await
            .expect("stale commit");
        assert_eq!(
            stale,
            CanvasSnapshotCommitResult::Stale {
                current_revision: 1
            }
        );

        let second = store
            .commit_canvas_snapshot(CommitCanvasSnapshot {
                workspace_id: &workspace.id,
                expected_revision: 1,
                nodes_json: r#"{"node":{"position":[3.0,4.0]}}"#,
                viewport_json: Some(r#"{"x":8.0,"y":9.0,"zoom":1.5}"#),
            })
            .await
            .expect("second commit");
        assert!(
            matches!(second, CanvasSnapshotCommitResult::Applied(record) if record.revision == 2)
        );
        assert_eq!(
            store
                .canvas_snapshot(&workspace.id)
                .await
                .expect("updated snapshot")
                .expect("snapshot row")
                .nodes_json,
            r#"{"node":{"position":[3.0,4.0]}}"#
        );
    }

    #[tokio::test]
    async fn concurrent_same_revision_canvas_commits_have_one_winner() {
        let dir = tempfile::tempdir().expect("temp dir");
        let database_url = format!("sqlite://{}", dir.path().join("store.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store.create_workspace("Canvas").await.expect("workspace");
        let mut tasks = tokio::task::JoinSet::new();
        for index in 0..16 {
            let store = store.clone();
            let workspace_id = workspace.id.clone();
            tasks.spawn(async move {
                let nodes_json = format!(r#"{{"node":{{"position":[{index},0]}}}}"#);
                store
                    .commit_canvas_snapshot(CommitCanvasSnapshot {
                        workspace_id: &workspace_id,
                        expected_revision: 0,
                        nodes_json: &nodes_json,
                        viewport_json: None,
                    })
                    .await
            });
        }
        let mut applied = 0;
        while let Some(result) = tasks.join_next().await {
            match result.expect("join canvas commit").expect("canvas commit") {
                CanvasSnapshotCommitResult::Applied(_) => applied += 1,
                CanvasSnapshotCommitResult::Stale {
                    current_revision: 1,
                } => {}
                other => panic!("unexpected canvas commit result: {other:?}"),
            }
        }
        assert_eq!(applied, 1);
    }
}
