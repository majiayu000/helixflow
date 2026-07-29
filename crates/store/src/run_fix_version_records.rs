use sqlx::Row;

use crate::run_fix_records::{fix_attempt_in_tx, require_one_write};

use super::{
    AutoApplyProposalVersionResult, MessageRecord, NewProposal, NewVersion, ProposalRecord,
    RunFixAttemptRecord, Store, StoreError, StoreResult, VersionRecord, new_id,
};

#[derive(Debug, Clone)]
pub struct ApplyRunFixVersionRecord<'a> {
    pub attempt_id: &'a str,
    pub proposal: NewProposal<'a>,
    pub version: NewVersion<'a>,
    pub message_text: &'a str,
    pub actual_effective_provider_id: &'a str,
    pub actual_recovery_scope_fingerprint: &'a str,
    pub actual_provider_catalog_fingerprint: &'a str,
}

#[derive(Debug, Clone)]
pub struct ApplyRunFixVersionResult {
    pub applied: AutoApplyProposalVersionResult,
    pub attempt: RunFixAttemptRecord,
}

impl Store {
    pub async fn apply_run_fix_version(
        &self,
        input: ApplyRunFixVersionRecord<'_>,
    ) -> StoreResult<ApplyRunFixVersionResult> {
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        let attempt = fix_attempt_in_tx(&mut tx, input.attempt_id).await?;
        if matches!(
            attempt.state.as_str(),
            "version_applied" | "child_preparing" | "child_ready"
        ) {
            return applied_fix_result(tx, attempt).await;
        }
        validate_fix_apply_input(&attempt, &input)?;

        let workspace =
            sqlx::query("SELECT cur_version_id, runtime_provider_id FROM workspaces WHERE id = ?")
                .bind(&attempt.workspace_id)
                .fetch_one(&mut *tx)
                .await?;
        let current_version_id: Option<String> = workspace.try_get("cur_version_id")?;
        let runtime_provider_id: Option<String> = workspace.try_get("runtime_provider_id")?;
        if current_version_id.as_deref() != Some(&attempt.source_version_id) {
            return Err(StoreError::VersionConflict {
                workspace_id: attempt.workspace_id,
                expected_version_id: attempt.source_version_id,
                actual_version_id: current_version_id,
            });
        }
        if runtime_provider_id != attempt.expected_runtime_provider_id {
            return Err(StoreError::RunFixGuardConflict {
                code: "FIX_PROVIDER_CHANGED",
            });
        }
        let source_status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = ?")
            .bind(&attempt.source_run_id)
            .fetch_one(&mut *tx)
            .await?;
        if source_status != "failed" {
            return Err(StoreError::RunFixGuardConflict {
                code: "FIX_SOURCE_CHANGED",
            });
        }

        let proposal_id = new_id("proposal");
        let version_id = new_id("ver");
        let message_id = new_id("msg");
        insert_fix_version(&mut tx, &version_id, &input.version).await?;
        let current_update = sqlx::query(
            r#"
            UPDATE workspaces
            SET cur_version_id = ?, updated_at = current_timestamp
            WHERE id = ? AND cur_version_id = ? AND runtime_provider_id IS ?
            "#,
        )
        .bind(&version_id)
        .bind(&attempt.workspace_id)
        .bind(&attempt.source_version_id)
        .bind(attempt.expected_runtime_provider_id.as_deref())
        .execute(&mut *tx)
        .await?;
        require_one_write("advance_run_fix_current", current_update.rows_affected())?;

        let attachments = serde_json::json!({ "versionId": version_id }).to_string();
        let message_insert = sqlx::query(
            r#"
            INSERT INTO messages (
                id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
            )
            VALUES (?, ?, 'agent', ?, 'proposal_applied', ?, ?, current_timestamp)
            "#,
        )
        .bind(&message_id)
        .bind(&attempt.workspace_id)
        .bind(input.message_text)
        .bind(&proposal_id)
        .bind(&attachments)
        .execute(&mut *tx)
        .await?;
        require_one_write("insert_run_fix_message", message_insert.rows_affected())?;
        insert_proposal(
            &mut tx,
            &proposal_id,
            &version_id,
            &message_id,
            &input.proposal,
        )
        .await?;

        let attempt_update = sqlx::query(
            r#"
            UPDATE run_fix_attempts
            SET state = 'version_applied', proposal_id = ?, target_version_id = ?,
                updated_at = current_timestamp
            WHERE id = ? AND state = 'agent_running'
            "#,
        )
        .bind(&proposal_id)
        .bind(&version_id)
        .bind(input.attempt_id)
        .execute(&mut *tx)
        .await?;
        require_one_write("link_run_fix_version", attempt_update.rows_affected())?;
        let attempt = fix_attempt_in_tx(&mut tx, input.attempt_id).await?;
        applied_fix_result(tx, attempt).await
    }
}

fn validate_fix_apply_input(
    attempt: &RunFixAttemptRecord,
    input: &ApplyRunFixVersionRecord<'_>,
) -> StoreResult<()> {
    if attempt.state != "agent_running"
        || input.proposal.kind != "fix"
        || input.proposal.workspace_id != attempt.workspace_id
        || input.version.workspace_id != attempt.workspace_id
        || input.proposal.base_version_id != attempt.source_version_id
        || input.version.parent_id != Some(attempt.source_version_id.as_str())
    {
        return Err(StoreError::RunFixGuardConflict {
            code: "FIX_PROPOSAL_INVALID",
        });
    }
    if input.actual_effective_provider_id != attempt.effective_provider_id {
        return Err(StoreError::RunFixGuardConflict {
            code: "FIX_PROVIDER_CHANGED",
        });
    }
    if input.actual_recovery_scope_fingerprint != attempt.expected_recovery_scope_fingerprint {
        return Err(StoreError::RunFixGuardConflict {
            code: "FIX_PROVIDER_SCOPE_CHANGED",
        });
    }
    if input.actual_provider_catalog_fingerprint != attempt.provider_catalog_fingerprint {
        return Err(StoreError::RunFixGuardConflict {
            code: "FIX_PROVIDER_CHANGED",
        });
    }
    Ok(())
}

async fn insert_fix_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    version_id: &str,
    version: &NewVersion<'_>,
) -> StoreResult<()> {
    let result = sqlx::query(
        r#"
        INSERT INTO versions (
            id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
            semantics_json, created_at
        )
        VALUES (
            ?, ?, (SELECT COALESCE(MAX(idx), 0) + 1 FROM versions WHERE workspace_id = ?),
            ?, ?, ?, ?, ?, ?, current_timestamp
        )
        "#,
    )
    .bind(version_id)
    .bind(version.workspace_id)
    .bind(version.workspace_id)
    .bind(version.label)
    .bind(version.source.as_str())
    .bind(version.graph_path)
    .bind(version.graph_hash)
    .bind(version.parent_id)
    .bind(version.semantics_json)
    .execute(&mut **tx)
    .await?;
    require_one_write("insert_run_fix_version", result.rows_affected())
}

async fn insert_proposal(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    proposal_id: &str,
    version_id: &str,
    message_id: &str,
    proposal: &NewProposal<'_>,
) -> StoreResult<()> {
    let result = sqlx::query(
        r#"
        INSERT INTO proposals (
            id, workspace_id, base_version_id, kind, title, summary,
            ops_path, preview_graph_path, state, result_version_id, message_id,
            created_at, resolved_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'applied', ?, ?, current_timestamp, current_timestamp)
        "#,
    )
    .bind(proposal_id)
    .bind(proposal.workspace_id)
    .bind(proposal.base_version_id)
    .bind(proposal.kind)
    .bind(proposal.title)
    .bind(proposal.summary)
    .bind(proposal.ops_path)
    .bind(proposal.preview_graph_path)
    .bind(version_id)
    .bind(message_id)
    .execute(&mut **tx)
    .await?;
    require_one_write("insert_run_fix_proposal", result.rows_affected())
}

async fn applied_fix_result(
    mut tx: sqlx::Transaction<'_, sqlx::Sqlite>,
    attempt: RunFixAttemptRecord,
) -> StoreResult<ApplyRunFixVersionResult> {
    let proposal_id = attempt
        .proposal_id
        .as_deref()
        .ok_or(StoreError::RunFixGuardConflict {
            code: "FIX_RECOVERY_INTERRUPTED",
        })?;
    let version_id =
        attempt
            .target_version_id
            .as_deref()
            .ok_or(StoreError::RunFixGuardConflict {
                code: "FIX_RECOVERY_INTERRUPTED",
            })?;
    let proposal = sqlx::query_as::<_, ProposalRecord>(
        r#"
        SELECT id, workspace_id, base_version_id, kind, title, summary, ops_path,
               preview_graph_path, state, result_version_id, message_id, created_at, resolved_at
        FROM proposals WHERE id = ?
        "#,
    )
    .bind(proposal_id)
    .fetch_one(&mut *tx)
    .await?;
    let version = sqlx::query_as::<_, VersionRecord>(
        r#"
        SELECT id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
               semantics_json, created_at FROM versions WHERE id = ?
        "#,
    )
    .bind(version_id)
    .fetch_one(&mut *tx)
    .await?;
    let message = sqlx::query_as::<_, MessageRecord>(
        r#"
        SELECT id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
        FROM messages WHERE id = ?
        "#,
    )
    .bind(proposal.message_id.as_deref())
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApplyRunFixVersionResult {
        applied: AutoApplyProposalVersionResult {
            proposal,
            version,
            message,
        },
        attempt,
    })
}
