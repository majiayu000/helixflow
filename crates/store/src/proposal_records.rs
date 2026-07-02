use crate::{Store, StoreResult, new_id};

#[derive(Debug, Clone)]
pub struct NewProposal<'a> {
    pub workspace_id: &'a str,
    pub base_version_id: &'a str,
    pub kind: &'a str,
    pub title: &'a str,
    pub summary: &'a str,
    pub ops_path: &'a str,
    pub preview_graph_path: Option<&'a str>,
    pub message_id: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ProposalRecord {
    pub id: String,
    pub workspace_id: String,
    pub base_version_id: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub ops_path: String,
    pub preview_graph_path: Option<String>,
    pub state: String,
    pub result_version_id: Option<String>,
    pub message_id: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

impl Store {
    pub async fn create_proposal(&self, input: NewProposal<'_>) -> StoreResult<ProposalRecord> {
        let id = new_id("prop");
        sqlx::query(
            r#"
            INSERT INTO proposals (
                id, workspace_id, base_version_id, kind, title, summary, ops_path,
                preview_graph_path, state, result_version_id, message_id, created_at, resolved_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending', NULL, ?, current_timestamp, NULL)
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.base_version_id)
        .bind(input.kind)
        .bind(input.title)
        .bind(input.summary)
        .bind(input.ops_path)
        .bind(input.preview_graph_path)
        .bind(input.message_id)
        .execute(&self.pool)
        .await?;

        self.proposal(&id).await
    }

    pub async fn proposal(&self, proposal_id: &str) -> StoreResult<ProposalRecord> {
        Ok(sqlx::query_as::<_, ProposalRecord>(
            r#"
            SELECT id, workspace_id, base_version_id, kind, title, summary, ops_path,
                   preview_graph_path, state, result_version_id, message_id,
                   created_at, resolved_at
            FROM proposals
            WHERE id = ?
            "#,
        )
        .bind(proposal_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn workspace_pending_proposal(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Option<ProposalRecord>> {
        Ok(sqlx::query_as::<_, ProposalRecord>(
            r#"
            SELECT id, workspace_id, base_version_id, kind, title, summary, ops_path,
                   preview_graph_path, state, result_version_id, message_id,
                   created_at, resolved_at
            FROM proposals
            WHERE workspace_id = ? AND state = 'pending'
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn resolve_proposal(
        &self,
        proposal_id: &str,
        state: &str,
        result_version_id: Option<&str>,
    ) -> StoreResult<ProposalRecord> {
        sqlx::query(
            r#"
            UPDATE proposals
            SET state = ?, result_version_id = ?, resolved_at = current_timestamp
            WHERE id = ?
            "#,
        )
        .bind(state)
        .bind(result_version_id)
        .bind(proposal_id)
        .execute(&self.pool)
        .await?;

        self.proposal(proposal_id).await
    }

    pub async fn workspace_proposal_history(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Vec<ProposalRecord>> {
        Ok(sqlx::query_as::<_, ProposalRecord>(
            r#"
            SELECT id, workspace_id, base_version_id, kind, title, summary, ops_path,
                   preview_graph_path, state, result_version_id, message_id,
                   created_at, resolved_at
            FROM proposals
            WHERE workspace_id = ?
            ORDER BY created_at
            "#,
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await?)
    }
}
