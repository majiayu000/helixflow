use serde::{Deserialize, Serialize};

use super::{Store, StoreError, StoreResult, new_id};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct ConversationRecord {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub codex_thread_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct AgentTurnRecord {
    pub id: String,
    pub conversation_id: String,
    pub workspace_id: String,
    pub user_message_id: Option<String>,
    pub execution_id: Option<String>,
    pub codex_turn_id: Option<String>,
    pub mode: String,
    pub status: String,
    pub reason_code: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

impl Store {
    pub async fn create_conversation(
        &self,
        workspace_id: &str,
        title: &str,
    ) -> StoreResult<ConversationRecord> {
        let id = new_id("conv");
        let inserted = sqlx::query(
            r#"
            INSERT INTO conversations (
                id, workspace_id, title, created_at, updated_at
            )
            SELECT ?, id, ?, current_timestamp, current_timestamp
            FROM workspaces
            WHERE id = ?
            "#,
        )
        .bind(&id)
        .bind(title)
        .bind(workspace_id)
        .execute(self.pool())
        .await?;
        if inserted.rows_affected() != 1 {
            return Err(StoreError::Sqlx(sqlx::Error::RowNotFound));
        }
        self.conversation(workspace_id, &id).await
    }

    pub async fn ensure_workspace_conversation(
        &self,
        workspace_id: &str,
    ) -> StoreResult<ConversationRecord> {
        if let Some(conversation) = sqlx::query_as::<_, ConversationRecord>(
            r#"
            SELECT id, workspace_id, title, codex_thread_id, created_at, updated_at, archived_at
            FROM conversations
            WHERE workspace_id = ? AND archived_at IS NULL
            ORDER BY updated_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(workspace_id)
        .fetch_optional(self.pool())
        .await?
        {
            return Ok(conversation);
        }
        self.create_conversation(workspace_id, "新对话").await
    }

    pub async fn conversation(
        &self,
        workspace_id: &str,
        conversation_id: &str,
    ) -> StoreResult<ConversationRecord> {
        Ok(sqlx::query_as(
            r#"
            SELECT id, workspace_id, title, codex_thread_id, created_at, updated_at, archived_at
            FROM conversations
            WHERE id = ? AND workspace_id = ? AND archived_at IS NULL
            "#,
        )
        .bind(conversation_id)
        .bind(workspace_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn workspace_conversations(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Vec<ConversationRecord>> {
        Ok(sqlx::query_as(
            r#"
            SELECT id, workspace_id, title, codex_thread_id, created_at, updated_at, archived_at
            FROM conversations
            WHERE workspace_id = ? AND archived_at IS NULL
            ORDER BY updated_at DESC, id DESC
            "#,
        )
        .bind(workspace_id)
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn bind_conversation_codex_thread(
        &self,
        workspace_id: &str,
        conversation_id: &str,
        codex_thread_id: &str,
    ) -> StoreResult<ConversationRecord> {
        let conversation = self.conversation(workspace_id, conversation_id).await?;
        if let Some(existing) = conversation.codex_thread_id.as_deref() {
            if existing == codex_thread_id {
                return Ok(conversation);
            }
            return Err(StoreError::AgentContractObservationInvariant {
                code: "CODEX_THREAD_ID_CONFLICT",
            });
        }
        sqlx::query(
            r#"
            UPDATE conversations
            SET codex_thread_id = ?, updated_at = current_timestamp
            WHERE id = ? AND workspace_id = ? AND codex_thread_id IS NULL
            "#,
        )
        .bind(codex_thread_id)
        .bind(conversation_id)
        .bind(workspace_id)
        .execute(self.pool())
        .await?;
        let bound = self.conversation(workspace_id, conversation_id).await?;
        if bound.codex_thread_id.as_deref() == Some(codex_thread_id) {
            return Ok(bound);
        }
        Err(StoreError::AgentContractObservationInvariant {
            code: "CODEX_THREAD_ID_CONFLICT",
        })
    }

    pub async fn start_agent_turn(
        &self,
        workspace_id: &str,
        conversation_id: &str,
        mode: &str,
    ) -> StoreResult<AgentTurnRecord> {
        self.conversation(workspace_id, conversation_id).await?;
        let id = new_id("turn");
        sqlx::query(
            r#"
            INSERT INTO agent_turns (
                id, conversation_id, workspace_id, mode, status, started_at
            )
            VALUES (?, ?, ?, ?, 'running', current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(conversation_id)
        .bind(workspace_id)
        .bind(mode)
        .execute(self.pool())
        .await?;
        sqlx::query("UPDATE conversations SET updated_at = current_timestamp WHERE id = ?")
            .bind(conversation_id)
            .execute(self.pool())
            .await?;
        self.agent_turn(&id).await
    }

    pub async fn attach_turn_user_message(
        &self,
        turn_id: &str,
        user_message_id: &str,
    ) -> StoreResult<()> {
        let updated = sqlx::query(
            r#"
            UPDATE agent_turns
            SET user_message_id = ?
            WHERE id = ? AND status = 'running' AND user_message_id IS NULL
            "#,
        )
        .bind(user_message_id)
        .bind(turn_id)
        .execute(self.pool())
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::StatementInvariant {
                operation: "attach_turn_user_message",
                expected_rows: 1,
                actual_rows: updated.rows_affected(),
            });
        }
        Ok(())
    }

    pub async fn finalize_agent_turn(
        &self,
        turn_id: &str,
        status: &str,
        reason_code: Option<&str>,
        execution_id: Option<&str>,
    ) -> StoreResult<AgentTurnRecord> {
        if !matches!(status, "succeeded" | "clarify" | "error" | "interrupted") {
            return Err(StoreError::AgentContractObservationInvariant {
                code: "INVALID_AGENT_TURN_STATUS",
            });
        }
        let updated = sqlx::query(
            r#"
            UPDATE agent_turns
            SET status = ?, reason_code = ?, execution_id = ?, completed_at = current_timestamp
            WHERE id = ? AND status = 'running'
            "#,
        )
        .bind(status)
        .bind(reason_code)
        .bind(execution_id)
        .bind(turn_id)
        .execute(self.pool())
        .await?;
        let turn = self.agent_turn(turn_id).await?;
        if updated.rows_affected() == 1
            || (turn.status == status
                && turn.reason_code.as_deref() == reason_code
                && turn.execution_id.as_deref() == execution_id)
        {
            return Ok(turn);
        }
        Err(StoreError::AgentContractObservationInvariant {
            code: "AGENT_TURN_TERMINAL_CONFLICT",
        })
    }

    pub async fn attach_agent_turn_codex_identity(
        &self,
        turn_id: &str,
        codex_turn_id: &str,
    ) -> StoreResult<AgentTurnRecord> {
        let updated = sqlx::query(
            r#"
            UPDATE agent_turns
            SET codex_turn_id = ?
            WHERE id = ? AND codex_turn_id IS NULL
            "#,
        )
        .bind(codex_turn_id)
        .bind(turn_id)
        .execute(self.pool())
        .await?;
        let turn = self.agent_turn(turn_id).await?;
        if updated.rows_affected() == 1 || turn.codex_turn_id.as_deref() == Some(codex_turn_id) {
            return Ok(turn);
        }
        Err(StoreError::AgentContractObservationInvariant {
            code: "CODEX_TURN_ID_CONFLICT",
        })
    }

    pub async fn agent_turn(&self, turn_id: &str) -> StoreResult<AgentTurnRecord> {
        Ok(sqlx::query_as(
            r#"
            SELECT id, conversation_id, workspace_id, user_message_id, execution_id, codex_turn_id, mode,
                   status, reason_code, started_at, completed_at
            FROM agent_turns
            WHERE id = ?
            "#,
        )
        .bind(turn_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn conversation_turns(
        &self,
        conversation_id: &str,
    ) -> StoreResult<Vec<AgentTurnRecord>> {
        Ok(sqlx::query_as(
            r#"
            SELECT id, conversation_id, workspace_id, user_message_id, execution_id, codex_turn_id, mode,
                   status, reason_code, started_at, completed_at
            FROM agent_turns
            WHERE conversation_id = ?
            ORDER BY started_at, id
            "#,
        )
        .bind(conversation_id)
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn workspace_agent_turns(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Vec<AgentTurnRecord>> {
        Ok(sqlx::query_as(
            r#"
            SELECT id, conversation_id, workspace_id, user_message_id, execution_id, codex_turn_id, mode,
                   status, reason_code, started_at, completed_at
            FROM agent_turns
            WHERE workspace_id = ?
            ORDER BY started_at, id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(self.pool())
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use crate::{NewMessage, Store};

    #[tokio::test]
    async fn separates_conversations_and_persists_terminal_turns() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&format!(
            "sqlite://{}",
            dir.path().join("helixflow.sqlite").display()
        ))
        .await
        .expect("store");
        let workspace = store.create_workspace("Sessions").await.expect("workspace");
        let first = store
            .create_conversation(&workspace.id, "First")
            .await
            .expect("first conversation");
        let second = store
            .create_conversation(&workspace.id, "Second")
            .await
            .expect("second conversation");
        let turn = store
            .start_agent_turn(&workspace.id, &first.id, "chat")
            .await
            .expect("turn");
        let message = store
            .create_message(NewMessage {
                workspace_id: &workspace.id,
                role: "user",
                kind: "text",
                text: Some("continue"),
                ref_id: None,
                attachment_ids_json: None,
                conversation_id: Some(&first.id),
                turn_id: Some(&turn.id),
            })
            .await
            .expect("message");
        store
            .attach_turn_user_message(&turn.id, &message.id)
            .await
            .expect("attach user message");
        let terminal = store
            .finalize_agent_turn(
                &turn.id,
                "error",
                Some("AGENT_RUNTIME_ERROR"),
                Some("agent_exec"),
            )
            .await
            .expect("terminal turn");

        assert_eq!(terminal.status, "error");
        assert_eq!(terminal.execution_id.as_deref(), Some("agent_exec"));
        assert_eq!(
            store.conversation_messages(&first.id).await.unwrap().len(),
            1
        );
        assert!(
            store
                .conversation_messages(&second.id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn binds_one_stable_codex_thread_per_conversation() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&format!(
            "sqlite://{}",
            dir.path().join("helixflow.sqlite").display()
        ))
        .await
        .expect("store");
        let workspace = store.create_workspace("Threads").await.expect("workspace");
        let conversation = store
            .create_conversation(&workspace.id, "Thread")
            .await
            .expect("conversation");

        let bound = store
            .bind_conversation_codex_thread(&workspace.id, &conversation.id, "thr_1")
            .await
            .expect("bind");
        assert_eq!(bound.codex_thread_id.as_deref(), Some("thr_1"));
        store
            .bind_conversation_codex_thread(&workspace.id, &conversation.id, "thr_1")
            .await
            .expect("same identity is idempotent");
        assert!(
            store
                .bind_conversation_codex_thread(&workspace.id, &conversation.id, "thr_2")
                .await
                .is_err()
        );

        let turn = store
            .start_agent_turn(&workspace.id, &conversation.id, "chat")
            .await
            .expect("turn");
        let bound_turn = store
            .attach_agent_turn_codex_identity(&turn.id, "codex_turn_1")
            .await
            .expect("bind turn");
        assert_eq!(bound_turn.codex_turn_id.as_deref(), Some("codex_turn_1"));
        store
            .attach_agent_turn_codex_identity(&turn.id, "codex_turn_1")
            .await
            .expect("same turn identity is idempotent");
        assert!(
            store
                .attach_agent_turn_codex_identity(&turn.id, "codex_turn_2")
                .await
                .is_err()
        );
    }
}
