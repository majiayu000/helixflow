use serde::{Deserialize, Serialize};

use super::{NewVersion, Store, StoreError, StoreResult, VersionRecord, new_id};

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

#[derive(Debug, Clone)]
pub struct ResolveProposal<'a> {
    pub proposal_id: &'a str,
    pub workspace_id: &'a str,
    pub state: ProposalResolutionState,
    pub result_version_id: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalResolutionState {
    Applied,
    Dismissed,
    Superseded,
    Failed,
}

impl ProposalResolutionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Dismissed => "dismissed",
            Self::Superseded => "superseded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApplyProposalVersionRecord<'a> {
    pub proposal_id: &'a str,
    pub expected_current_version_id: &'a str,
    pub version: NewVersion<'a>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
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
        let id = new_id("proposal");

        sqlx::query(
            r#"
            INSERT INTO proposals (
                id, workspace_id, base_version_id, kind, title, summary,
                ops_path, preview_graph_path, state, result_version_id, message_id,
                created_at, resolved_at
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
        .execute(self.pool())
        .await?;

        self.proposal(&id).await
    }

    pub async fn proposal(&self, proposal_id: &str) -> StoreResult<ProposalRecord> {
        Ok(sqlx::query_as::<_, ProposalRecord>(
            r#"
            SELECT id, workspace_id, base_version_id, kind, title, summary,
                   ops_path, preview_graph_path, state, result_version_id, message_id,
                   created_at, resolved_at
            FROM proposals
            WHERE id = ?
            "#,
        )
        .bind(proposal_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn workspace_proposals(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Vec<ProposalRecord>> {
        Ok(sqlx::query_as::<_, ProposalRecord>(
            r#"
            SELECT id, workspace_id, base_version_id, kind, title, summary,
                   ops_path, preview_graph_path, state, result_version_id, message_id,
                   created_at, resolved_at
            FROM proposals
            WHERE workspace_id = ?
            ORDER BY created_at, id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn latest_pending_proposal(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Option<ProposalRecord>> {
        Ok(sqlx::query_as::<_, ProposalRecord>(
            r#"
            SELECT id, workspace_id, base_version_id, kind, title, summary,
                   ops_path, preview_graph_path, state, result_version_id, message_id,
                   created_at, resolved_at
            FROM proposals
            WHERE workspace_id = ? AND state = 'pending'
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(workspace_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn attach_proposal_message(
        &self,
        proposal_id: &str,
        message_id: &str,
    ) -> StoreResult<ProposalRecord> {
        sqlx::query(
            r#"
            UPDATE proposals
            SET message_id = ?
            WHERE id = ?
            "#,
        )
        .bind(message_id)
        .bind(proposal_id)
        .execute(self.pool())
        .await?;

        self.proposal(proposal_id).await
    }

    pub async fn resolve_proposal(
        &self,
        input: ResolveProposal<'_>,
    ) -> StoreResult<ProposalRecord> {
        let current = self.proposal(input.proposal_id).await?;
        ensure_proposal_workspace(&current, input.workspace_id)?;
        ensure_pending(&current)?;

        let result = sqlx::query(
            r#"
            UPDATE proposals
            SET state = ?, result_version_id = ?, resolved_at = current_timestamp
            WHERE id = ? AND workspace_id = ? AND state = 'pending'
            "#,
        )
        .bind(input.state.as_str())
        .bind(input.result_version_id)
        .bind(input.proposal_id)
        .bind(input.workspace_id)
        .execute(self.pool())
        .await?;

        if result.rows_affected() != 1 {
            let actual = self
                .proposal(input.proposal_id)
                .await
                .ok()
                .map(|item| item.state);
            return Err(StoreError::ProposalStateConflict {
                proposal_id: input.proposal_id.to_owned(),
                expected_state: "pending".to_owned(),
                actual_state: actual,
            });
        }

        self.proposal(input.proposal_id).await
    }

    pub async fn create_version_after_applying_proposal(
        &self,
        input: ApplyProposalVersionRecord<'_>,
    ) -> StoreResult<VersionRecord> {
        let version_id = new_id("ver");
        let mut tx = self.pool().begin().await?;

        let proposal = sqlx::query_as::<_, ProposalRecord>(
            r#"
            SELECT id, workspace_id, base_version_id, kind, title, summary,
                   ops_path, preview_graph_path, state, result_version_id, message_id,
                   created_at, resolved_at
            FROM proposals
            WHERE id = ?
            "#,
        )
        .bind(input.proposal_id)
        .fetch_one(&mut *tx)
        .await?;
        ensure_proposal_workspace(&proposal, input.version.workspace_id)?;
        ensure_pending(&proposal)?;

        let actual_version_id: Option<String> = sqlx::query_scalar(
            r#"
            SELECT cur_version_id
            FROM workspaces
            WHERE id = ?
            "#,
        )
        .bind(input.version.workspace_id)
        .fetch_one(&mut *tx)
        .await?;

        if actual_version_id.as_deref() != Some(input.expected_current_version_id) {
            return Err(StoreError::VersionConflict {
                workspace_id: input.version.workspace_id.to_owned(),
                expected_version_id: input.expected_current_version_id.to_owned(),
                actual_version_id,
            });
        }

        let idx: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(MAX(idx), 0) + 1
            FROM versions
            WHERE workspace_id = ?
            "#,
        )
        .bind(input.version.workspace_id)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO versions (
                id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&version_id)
        .bind(input.version.workspace_id)
        .bind(idx)
        .bind(input.version.label)
        .bind(input.version.source.as_str())
        .bind(input.version.graph_path)
        .bind(input.version.graph_hash)
        .bind(input.version.parent_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE workspaces
            SET cur_version_id = ?, updated_at = current_timestamp
            WHERE id = ?
            "#,
        )
        .bind(&version_id)
        .bind(input.version.workspace_id)
        .execute(&mut *tx)
        .await?;

        let result = sqlx::query(
            r#"
            UPDATE proposals
            SET state = 'applied', result_version_id = ?, resolved_at = current_timestamp
            WHERE id = ? AND workspace_id = ? AND state = 'pending'
            "#,
        )
        .bind(&version_id)
        .bind(input.proposal_id)
        .bind(input.version.workspace_id)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() != 1 {
            return Err(StoreError::ProposalStateConflict {
                proposal_id: input.proposal_id.to_owned(),
                expected_state: "pending".to_owned(),
                actual_state: None,
            });
        }

        tx.commit().await?;
        self.version(&version_id).await
    }
}

fn ensure_proposal_workspace(proposal: &ProposalRecord, workspace_id: &str) -> StoreResult<()> {
    if proposal.workspace_id != workspace_id {
        return Err(StoreError::ProposalWorkspaceMismatch {
            proposal_id: proposal.id.clone(),
            workspace_id: workspace_id.to_owned(),
        });
    }
    Ok(())
}

fn ensure_pending(proposal: &ProposalRecord) -> StoreResult<()> {
    if proposal.state != "pending" {
        return Err(StoreError::ProposalStateConflict {
            proposal_id: proposal.id.clone(),
            expected_state: "pending".to_owned(),
            actual_state: Some(proposal.state.clone()),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Store, VersionSource};

    async fn open_temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let db_path = dir.path().join("helixflow.sqlite");
        let database_url = format!("sqlite://{}", db_path.display());
        let store = Store::open(&database_url).await.expect("open store");
        (store, dir)
    }

    #[tokio::test]
    async fn creates_and_lists_pending_proposals() {
        let (store, _dir) = open_temp_store().await;
        let workspace = store
            .create_workspace("Proposal workspace")
            .await
            .expect("create workspace");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Base graph",
                source: VersionSource::Manual,
                graph_path: "graphs/base.json",
                graph_hash: "sha256:base",
                parent_id: None,
            })
            .await
            .expect("create version");

        let proposal = store
            .create_proposal(NewProposal {
                workspace_id: &workspace.id,
                base_version_id: &version.id,
                kind: "modify",
                title: "Resize",
                summary: "Change resolution",
                ops_path: "proposals/resize/ops.json",
                preview_graph_path: Some("proposals/resize/preview.json"),
                message_id: None,
            })
            .await
            .expect("create proposal");

        let pending = store
            .latest_pending_proposal(&workspace.id)
            .await
            .expect("pending proposal")
            .expect("pending");
        assert_eq!(pending.id, proposal.id);
        assert_eq!(pending.state, "pending");
    }

    #[tokio::test]
    async fn resolving_proposal_rejects_repeated_resolution() {
        let (store, _dir) = open_temp_store().await;
        let workspace = store
            .create_workspace("Proposal workspace")
            .await
            .expect("create workspace");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Base graph",
                source: VersionSource::Manual,
                graph_path: "graphs/base.json",
                graph_hash: "sha256:base",
                parent_id: None,
            })
            .await
            .expect("create version");
        let proposal = store
            .create_proposal(NewProposal {
                workspace_id: &workspace.id,
                base_version_id: &version.id,
                kind: "modify",
                title: "Resize",
                summary: "Change resolution",
                ops_path: "proposals/resize/ops.json",
                preview_graph_path: None,
                message_id: None,
            })
            .await
            .expect("create proposal");

        let dismissed = store
            .resolve_proposal(ResolveProposal {
                proposal_id: &proposal.id,
                workspace_id: &workspace.id,
                state: ProposalResolutionState::Dismissed,
                result_version_id: None,
            })
            .await
            .expect("dismiss proposal");
        assert_eq!(dismissed.state, "dismissed");

        assert!(matches!(
            store
                .resolve_proposal(ResolveProposal {
                    proposal_id: &proposal.id,
                    workspace_id: &workspace.id,
                    state: ProposalResolutionState::Dismissed,
                    result_version_id: None,
                })
                .await,
            Err(StoreError::ProposalStateConflict { .. })
        ));
    }

    #[tokio::test]
    async fn applying_proposal_creates_version_and_marks_applied() {
        let (store, _dir) = open_temp_store().await;
        let workspace = store
            .create_workspace("Proposal workspace")
            .await
            .expect("create workspace");
        let base = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Base graph",
                source: VersionSource::Manual,
                graph_path: "graphs/base.json",
                graph_hash: "sha256:base",
                parent_id: None,
            })
            .await
            .expect("create version");
        let proposal = store
            .create_proposal(NewProposal {
                workspace_id: &workspace.id,
                base_version_id: &base.id,
                kind: "modify",
                title: "Resize",
                summary: "Change resolution",
                ops_path: "proposals/resize/ops.json",
                preview_graph_path: None,
                message_id: None,
            })
            .await
            .expect("create proposal");

        let child = store
            .create_version_after_applying_proposal(ApplyProposalVersionRecord {
                proposal_id: &proposal.id,
                expected_current_version_id: &base.id,
                version: NewVersion {
                    workspace_id: &workspace.id,
                    label: "Applied proposal",
                    source: VersionSource::Proposal,
                    graph_path: "graphs/applied.json",
                    graph_hash: "sha256:applied",
                    parent_id: Some(&base.id),
                },
            })
            .await
            .expect("apply proposal");
        let updated = store.proposal(&proposal.id).await.expect("proposal");

        assert_eq!(child.parent_id.as_deref(), Some(base.id.as_str()));
        assert_eq!(updated.state, "applied");
        assert_eq!(
            updated.result_version_id.as_deref(),
            Some(child.id.as_str())
        );
    }
}
