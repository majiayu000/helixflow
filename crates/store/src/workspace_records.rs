use serde::{Deserialize, Serialize};

use super::{Store, StoreResult, VersionRecord, new_id};
use crate::WorkspaceRecord;

#[derive(Debug, Clone)]
pub struct NewMessage<'a> {
    pub workspace_id: &'a str,
    pub role: &'a str,
    pub kind: &'a str,
    pub text: Option<&'a str>,
    pub ref_id: Option<&'a str>,
    pub attachment_ids_json: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct MessageRecord {
    pub id: String,
    pub workspace_id: String,
    pub role: String,
    pub text: Option<String>,
    pub kind: String,
    pub ref_id: Option<String>,
    pub attachment_ids_json: Option<String>,
    pub created_at: String,
}

impl Store {
    pub async fn create_workspace(&self, name: &str) -> StoreResult<WorkspaceRecord> {
        let id = new_id("ws");

        sqlx::query(
            r#"
            INSERT INTO workspaces (id, name, created_at, updated_at)
            VALUES (?, ?, current_timestamp, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(name)
        .execute(&self.pool)
        .await?;

        self.workspace(&id).await
    }

    pub async fn workspace(&self, workspace_id: &str) -> StoreResult<WorkspaceRecord> {
        let workspace = sqlx::query_as::<_, WorkspaceRecord>(
            r#"
            SELECT id, name, cur_version_id, runtime_provider_id, created_at, updated_at
            FROM workspaces
            WHERE id = ?
            "#,
        )
        .bind(workspace_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(workspace)
    }

    pub async fn set_workspace_runtime_provider(
        &self,
        workspace_id: &str,
        provider_id: Option<&str>,
    ) -> StoreResult<WorkspaceRecord> {
        sqlx::query(
            r#"
            UPDATE workspaces
            SET runtime_provider_id = ?, updated_at = current_timestamp
            WHERE id = ?
            "#,
        )
        .bind(provider_id)
        .bind(workspace_id)
        .execute(&self.pool)
        .await?;

        self.workspace(workspace_id).await
    }

    pub async fn workspaces(&self) -> StoreResult<Vec<WorkspaceRecord>> {
        Ok(sqlx::query_as::<_, WorkspaceRecord>(
            r#"
            SELECT id, name, cur_version_id, runtime_provider_id, created_at, updated_at
            FROM workspaces
            ORDER BY updated_at DESC, id DESC
            "#,
        )
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn create_message(&self, input: NewMessage<'_>) -> StoreResult<MessageRecord> {
        let id = new_id("msg");

        sqlx::query(
            r#"
            INSERT INTO messages (
                id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.role)
        .bind(input.text)
        .bind(input.kind)
        .bind(input.ref_id)
        .bind(input.attachment_ids_json)
        .execute(self.pool())
        .await?;

        self.message(&id).await
    }

    pub async fn message(&self, message_id: &str) -> StoreResult<MessageRecord> {
        Ok(sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
            FROM messages
            WHERE id = ?
            "#,
        )
        .bind(message_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn workspace_messages(&self, workspace_id: &str) -> StoreResult<Vec<MessageRecord>> {
        Ok(sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
            FROM messages
            WHERE workspace_id = ?
            ORDER BY created_at, id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(self.pool())
        .await?)
    }

    /// Real per-workspace message stats for list summaries (HF-026).
    pub async fn workspace_message_stats(
        &self,
        workspace_id: &str,
    ) -> StoreResult<(i64, Option<String>)> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM messages
            WHERE workspace_id = ? AND role = 'user'
            "#,
        )
        .bind(workspace_id)
        .fetch_one(self.pool())
        .await?;
        let first: Option<String> = sqlx::query_scalar(
            r#"
            SELECT text
            FROM messages
            WHERE workspace_id = ? AND role = 'user' AND text IS NOT NULL
            ORDER BY created_at, id
            LIMIT 1
            "#,
        )
        .bind(workspace_id)
        .fetch_optional(self.pool())
        .await?;
        Ok((count, first))
    }

    pub async fn versions_for_workspace(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Vec<VersionRecord>> {
        Ok(sqlx::query_as::<_, VersionRecord>(
            r#"
            SELECT id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
                   semantics_json, created_at
            FROM versions
            WHERE workspace_id = ?
            ORDER BY idx, created_at, id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(self.pool())
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewRun, NewVersion, Store, VersionSource};

    async fn open_temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let db_path = dir.path().join("helixflow.sqlite");
        let database_url = format!("sqlite://{}", db_path.display());
        let store = Store::open(&database_url).await.expect("open store");
        (store, dir)
    }

    #[tokio::test]
    async fn persists_workspace_messages_in_created_order() {
        let (store, _dir) = open_temp_store().await;
        let workspace = store
            .create_workspace("Messages")
            .await
            .expect("create workspace");

        let user = store
            .create_message(NewMessage {
                workspace_id: &workspace.id,
                role: "user",
                kind: "text",
                text: Some("Build a workflow"),
                ref_id: None,
                attachment_ids_json: None,
            })
            .await
            .expect("create user message");
        let agent = store
            .create_message(NewMessage {
                workspace_id: &workspace.id,
                role: "agent",
                kind: "proposal_pending",
                text: Some("Proposal ready"),
                ref_id: Some("proposal_1"),
                attachment_ids_json: Some("[]"),
            })
            .await
            .expect("create agent message");

        let messages = store
            .workspace_messages(&workspace.id)
            .await
            .expect("workspace messages");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id, user.id);
        assert_eq!(messages[1].id, agent.id);
        assert_eq!(messages[1].ref_id.as_deref(), Some("proposal_1"));
    }

    #[tokio::test]
    async fn lists_workspaces_by_recent_update() {
        let (store, _dir) = open_temp_store().await;
        let first = store
            .create_workspace("First")
            .await
            .expect("create first workspace");
        let second = store
            .create_workspace("Second")
            .await
            .expect("create second workspace");

        let workspaces = store.workspaces().await.expect("workspaces");

        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].id, second.id);
        assert_eq!(workspaces[1].id, first.id);
    }

    #[tokio::test]
    async fn lists_versions_and_latest_run_for_workspace() {
        let (store, _dir) = open_temp_store().await;
        let workspace = store
            .create_workspace("Versions")
            .await
            .expect("create workspace");
        let first = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "First",
                source: VersionSource::Manual,
                graph_path: "graphs/first.json",
                graph_hash: "sha256:first",
                parent_id: None,
                semantics_json: None,
            })
            .await
            .expect("create first version");
        let second = store
            .create_version_after(
                NewVersion {
                    workspace_id: &workspace.id,
                    label: "Second",
                    source: VersionSource::Manual,
                    graph_path: "graphs/second.json",
                    graph_hash: "sha256:second",
                    parent_id: Some(&first.id),
                    semantics_json: None,
                },
                &first.id,
            )
            .await
            .expect("create second version");
        let run = store
            .create_run(NewRun {
                workspace_id: &workspace.id,
                version_id: &second.id,
                group_id: None,
                label: "Latest run",
                trigger: "manual",
                plan_json: None,
                estimate_json: None,
                status: "queued",
            })
            .await
            .expect("create run");

        let versions = store
            .versions_for_workspace(&workspace.id)
            .await
            .expect("versions");
        let latest_run = store
            .latest_workspace_run(&workspace.id)
            .await
            .expect("latest run")
            .expect("run");

        assert_eq!(
            versions
                .iter()
                .map(|version| version.id.as_str())
                .collect::<Vec<_>>(),
            vec![first.id.as_str(), second.id.as_str()]
        );
        assert_eq!(latest_run.id, run.id);
    }
}
