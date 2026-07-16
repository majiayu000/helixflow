use serde::{Deserialize, Serialize};

use super::{MessageRecord, NewVersion, Store, StoreError, StoreResult, VersionRecord, new_id};

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
    pub message_text: &'a str,
}

#[derive(Debug, Clone)]
pub struct ApplyProposalVersionResult {
    pub version: VersionRecord,
    pub message: MessageRecord,
}

#[derive(Debug, Clone)]
pub struct AutoApplyProposalVersionRecord<'a> {
    pub proposal: NewProposal<'a>,
    pub version: NewVersion<'a>,
    pub message_text: &'a str,
}

#[derive(Debug, Clone)]
pub struct AutoApplyProposalVersionResult {
    pub proposal: ProposalRecord,
    pub version: VersionRecord,
    pub message: MessageRecord,
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
    ) -> StoreResult<ApplyProposalVersionResult> {
        ensure_version_parent(&input.version, input.expected_current_version_id)?;
        let version_id = new_id("ver");
        let message_id = new_id("msg");
        let message_attachment_ids_json = format!(r#"{{"versionId":"{version_id}"}}"#);
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;

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
        ensure_proposal_base(&proposal, input.expected_current_version_id)?;

        let actual_version_id: Option<String> =
            sqlx::query_scalar("SELECT cur_version_id FROM workspaces WHERE id = ?")
                .bind(input.version.workspace_id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
        if actual_version_id.as_deref() != Some(input.expected_current_version_id) {
            return Err(StoreError::VersionConflict {
                workspace_id: input.version.workspace_id.to_owned(),
                expected_version_id: input.expected_current_version_id.to_owned(),
                actual_version_id,
            });
        }

        let idx: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(idx), 0) + 1 FROM versions WHERE workspace_id = ?",
        )
        .bind(input.version.workspace_id)
        .fetch_one(&mut *tx)
        .await?;
        let version_insert = sqlx::query(
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
        require_one_write(
            "insert_applied_proposal_version",
            version_insert.rows_affected(),
        )?;

        let current_update = sqlx::query(
            r#"
            UPDATE workspaces
            SET cur_version_id = ?, updated_at = current_timestamp
            WHERE id = ? AND cur_version_id = ?
            "#,
        )
        .bind(&version_id)
        .bind(input.version.workspace_id)
        .bind(input.expected_current_version_id)
        .execute(&mut *tx)
        .await?;
        if current_update.rows_affected() == 0 {
            let actual_version_id: Option<String> =
                sqlx::query_scalar("SELECT cur_version_id FROM workspaces WHERE id = ?")
                    .bind(input.version.workspace_id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .flatten();
            if actual_version_id.as_deref() != Some(input.expected_current_version_id) {
                return Err(StoreError::VersionConflict {
                    workspace_id: input.version.workspace_id.to_owned(),
                    expected_version_id: input.expected_current_version_id.to_owned(),
                    actual_version_id,
                });
            }
            return Err(statement_invariant("advance_applied_proposal_current", 0));
        }
        require_one_write(
            "advance_applied_proposal_current",
            current_update.rows_affected(),
        )?;

        let proposal_update = sqlx::query(
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
        if proposal_update.rows_affected() == 0 {
            let actual_state = sqlx::query_scalar("SELECT state FROM proposals WHERE id = ?")
                .bind(input.proposal_id)
                .fetch_optional(&mut *tx)
                .await?;
            if actual_state.as_deref() != Some("pending") {
                return Err(StoreError::ProposalStateConflict {
                    proposal_id: input.proposal_id.to_owned(),
                    expected_state: "pending".to_owned(),
                    actual_state,
                });
            }
            return Err(statement_invariant("mark_applied_proposal", 0));
        }
        require_one_write("mark_applied_proposal", proposal_update.rows_affected())?;

        let message_insert = sqlx::query(
            r#"
            INSERT INTO messages (
                id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
            )
            VALUES (?, ?, 'agent', ?, 'proposal_applied', ?, ?, current_timestamp)
            "#,
        )
        .bind(&message_id)
        .bind(input.version.workspace_id)
        .bind(input.message_text)
        .bind(input.proposal_id)
        .bind(&message_attachment_ids_json)
        .execute(&mut *tx)
        .await?;
        require_one_write(
            "insert_applied_proposal_message",
            message_insert.rows_affected(),
        )?;

        let version = sqlx::query_as::<_, VersionRecord>(
            r#"
            SELECT id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id, created_at
            FROM versions
            WHERE id = ?
            "#,
        )
        .bind(&version_id)
        .fetch_one(&mut *tx)
        .await?;
        let message = sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
            FROM messages
            WHERE id = ?
            "#,
        )
        .bind(&message_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ApplyProposalVersionResult { version, message })
    }

    pub async fn auto_apply_proposal_version(
        &self,
        input: AutoApplyProposalVersionRecord<'_>,
    ) -> StoreResult<AutoApplyProposalVersionResult> {
        let proposal_id = new_id("proposal");
        let version_id = new_id("ver");
        let message_id = new_id("msg");
        let message_attachment_ids_json = format!(r#"{{"versionId":"{version_id}"}}"#);
        if input.proposal.workspace_id != input.version.workspace_id {
            return Err(StoreError::ProposalWorkspaceMismatch {
                proposal_id,
                workspace_id: input.version.workspace_id.to_owned(),
            });
        }
        ensure_version_parent(&input.version, input.proposal.base_version_id)?;

        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        let actual_version_id: Option<String> =
            sqlx::query_scalar("SELECT cur_version_id FROM workspaces WHERE id = ?")
                .bind(input.version.workspace_id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
        if actual_version_id.as_deref() != Some(input.proposal.base_version_id) {
            return Err(StoreError::VersionConflict {
                workspace_id: input.version.workspace_id.to_owned(),
                expected_version_id: input.proposal.base_version_id.to_owned(),
                actual_version_id,
            });
        }

        let version_insert = sqlx::query(
            r#"
            INSERT INTO versions (
                id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id, created_at
            )
            VALUES (
                ?, ?,
                (SELECT COALESCE(MAX(idx), 0) + 1 FROM versions WHERE workspace_id = ?),
                ?, ?, ?, ?, ?, current_timestamp
            )
            "#,
        )
        .bind(&version_id)
        .bind(input.version.workspace_id)
        .bind(input.version.workspace_id)
        .bind(input.version.label)
        .bind(input.version.source.as_str())
        .bind(input.version.graph_path)
        .bind(input.version.graph_hash)
        .bind(input.version.parent_id)
        .execute(&mut *tx)
        .await?;
        require_one_write(
            "insert_auto_applied_version",
            version_insert.rows_affected(),
        )?;

        let current_update = sqlx::query(
            r#"
            UPDATE workspaces
            SET cur_version_id = ?, updated_at = current_timestamp
            WHERE id = ? AND cur_version_id = ?
            "#,
        )
        .bind(&version_id)
        .bind(input.version.workspace_id)
        .bind(input.proposal.base_version_id)
        .execute(&mut *tx)
        .await?;
        if current_update.rows_affected() == 0 {
            let actual_version_id: Option<String> =
                sqlx::query_scalar("SELECT cur_version_id FROM workspaces WHERE id = ?")
                    .bind(input.version.workspace_id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .flatten();
            if actual_version_id.as_deref() != Some(input.proposal.base_version_id) {
                return Err(StoreError::VersionConflict {
                    workspace_id: input.version.workspace_id.to_owned(),
                    expected_version_id: input.proposal.base_version_id.to_owned(),
                    actual_version_id,
                });
            }
            return Err(statement_invariant("advance_auto_applied_current", 0));
        }
        require_one_write(
            "advance_auto_applied_current",
            current_update.rows_affected(),
        )?;

        let message_insert = sqlx::query(
            r#"
            INSERT INTO messages (
                id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
            )
            VALUES (?, ?, 'agent', ?, 'proposal_applied', ?, ?, current_timestamp)
            "#,
        )
        .bind(&message_id)
        .bind(input.proposal.workspace_id)
        .bind(input.message_text)
        .bind(&proposal_id)
        .bind(&message_attachment_ids_json)
        .execute(&mut *tx)
        .await?;
        require_one_write(
            "insert_auto_applied_message",
            message_insert.rows_affected(),
        )?;

        let proposal_insert = sqlx::query(
            r#"
            INSERT INTO proposals (
                id, workspace_id, base_version_id, kind, title, summary,
                ops_path, preview_graph_path, state, result_version_id, message_id,
                created_at, resolved_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'applied', ?, ?, current_timestamp, current_timestamp)
            "#,
        )
        .bind(&proposal_id)
        .bind(input.proposal.workspace_id)
        .bind(input.proposal.base_version_id)
        .bind(input.proposal.kind)
        .bind(input.proposal.title)
        .bind(input.proposal.summary)
        .bind(input.proposal.ops_path)
        .bind(input.proposal.preview_graph_path)
        .bind(&version_id)
        .bind(&message_id)
        .execute(&mut *tx)
        .await?;
        require_one_write(
            "insert_auto_applied_proposal",
            proposal_insert.rows_affected(),
        )?;

        let proposal = sqlx::query_as::<_, ProposalRecord>(
            r#"
            SELECT id, workspace_id, base_version_id, kind, title, summary,
                   ops_path, preview_graph_path, state, result_version_id, message_id,
                   created_at, resolved_at
            FROM proposals
            WHERE id = ?
            "#,
        )
        .bind(&proposal_id)
        .fetch_one(&mut *tx)
        .await?;
        let version = sqlx::query_as::<_, VersionRecord>(
            r#"
            SELECT id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id, created_at
            FROM versions
            WHERE id = ?
            "#,
        )
        .bind(&version_id)
        .fetch_one(&mut *tx)
        .await?;
        let message = sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
            FROM messages
            WHERE id = ?
            "#,
        )
        .bind(&message_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(AutoApplyProposalVersionResult {
            proposal,
            version,
            message,
        })
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

fn ensure_version_parent(version: &NewVersion<'_>, expected_parent: &str) -> StoreResult<()> {
    if version.parent_id != Some(expected_parent) {
        return Err(StoreError::VersionParentMismatch {
            workspace_id: version.workspace_id.to_owned(),
            expected_parent_version_id: expected_parent.to_owned(),
            actual_parent_version_id: version.parent_id.map(str::to_owned),
        });
    }
    Ok(())
}

fn ensure_proposal_base(proposal: &ProposalRecord, expected_base: &str) -> StoreResult<()> {
    if proposal.base_version_id != expected_base {
        return Err(StoreError::ProposalBaseMismatch {
            proposal_id: proposal.id.clone(),
            expected_base_version_id: expected_base.to_owned(),
            actual_base_version_id: proposal.base_version_id.clone(),
        });
    }
    Ok(())
}

fn require_one_write(operation: &'static str, actual_rows: u64) -> StoreResult<()> {
    if actual_rows == 1 {
        return Ok(());
    }
    Err(statement_invariant(operation, actual_rows))
}

fn statement_invariant(operation: &'static str, actual_rows: u64) -> StoreError {
    StoreError::StatementInvariant {
        operation,
        expected_rows: 1,
        actual_rows,
    }
}
