use serde::{Deserialize, Serialize};

use super::{MessageRecord, Store, StoreError, StoreResult, new_id};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedAgentTurnSettlement {
    pub turn: AgentTurnRecord,
    pub message: Option<MessageRecord>,
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
        let id = new_id("turn");
        self.start_agent_turn_with_id(&id, workspace_id, conversation_id, mode)
            .await
    }

    pub async fn start_agent_turn_with_id(
        &self,
        turn_id: &str,
        workspace_id: &str,
        conversation_id: &str,
        mode: &str,
    ) -> StoreResult<AgentTurnRecord> {
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        let inserted = sqlx::query(
            r#"
            INSERT INTO agent_turns (
                id, conversation_id, workspace_id, mode, status, started_at
            )
            SELECT ?, id, workspace_id, ?, 'running', current_timestamp
            FROM conversations
            WHERE id = ? AND workspace_id = ? AND archived_at IS NULL
            "#,
        )
        .bind(turn_id)
        .bind(mode)
        .bind(conversation_id)
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;
        if inserted.rows_affected() != 1 {
            return Err(StoreError::Sqlx(sqlx::Error::RowNotFound));
        }
        sqlx::query("UPDATE conversations SET updated_at = current_timestamp WHERE id = ?")
            .bind(conversation_id)
            .execute(&mut *tx)
            .await?;
        let turn = sqlx::query_as::<_, AgentTurnRecord>(
            r#"
            SELECT id, conversation_id, workspace_id, user_message_id, execution_id, codex_turn_id,
                   mode, status, reason_code, started_at, completed_at
            FROM agent_turns
            WHERE id = ?
            "#,
        )
        .bind(turn_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(turn)
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

    /// Atomically settles a durable turn whose request future disappeared.
    ///
    /// A graph-edit observation may have reached a terminal state just before
    /// the request was dropped. In that case its outcome wins and the turn is
    /// aligned with it. Otherwise the turn (and optional observation) become
    /// interrupted/error together with one visible terminal message.
    pub async fn settle_dropped_agent_turn(
        &self,
        turn_id: &str,
        observation_id: Option<&str>,
    ) -> StoreResult<Option<DroppedAgentTurnSettlement>> {
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        let Some(current) = sqlx::query_as::<_, AgentTurnRecord>(
            r#"
            SELECT id, conversation_id, workspace_id, user_message_id, execution_id, codex_turn_id,
                   mode, status, reason_code, started_at, completed_at
            FROM agent_turns
            WHERE id = ?
            "#,
        )
        .bind(turn_id)
        .fetch_optional(&mut *tx)
        .await?
        else {
            tx.commit().await?;
            return Ok(None);
        };
        if current.status != "running" {
            tx.commit().await?;
            return Ok(None);
        }

        let observation = match observation_id {
            Some(observation_id) => {
                sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
                    r#"
                SELECT outcome, reason_code, session_id
                FROM agent_contract_observations
                WHERE id = ? AND workspace_id = ?
                "#,
                )
                .bind(observation_id)
                .bind(&current.workspace_id)
                .fetch_optional(&mut *tx)
                .await?
            }
            None => None,
        };
        let existing_primary_message = sqlx::query_as::<_, MessageRecord>(
            r#"
            SELECT id, workspace_id, role, text, kind, ref_id, attachment_ids_json,
                   conversation_id, turn_id, created_at
            FROM messages
            WHERE turn_id = ? AND role = 'agent' AND kind NOT LIKE 'agent_log:%'
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(turn_id)
        .fetch_optional(&mut *tx)
        .await?;
        let observation_started = observation
            .as_ref()
            .is_some_and(|(outcome, _, _)| outcome == "started");

        let (status, reason_code, execution_id, recovery_message) =
            match (observation, existing_primary_message.as_ref()) {
                (Some((outcome, reason_code, session_id)), _) if outcome != "started" => {
                    let recovery_message = match outcome.as_str() {
                        "success" => Some((
                            "agent_status",
                            "Agent 请求已成功完成；终态已从持久化执行结果恢复。",
                        )),
                        "clarify" => Some((
                            "clarify",
                            "Agent 需要补充信息；请刷新工作台查看并继续本轮。",
                        )),
                        "error" => Some((
                            "agent_error",
                            "Agent 请求执行失败；终态已从持久化执行结果恢复。",
                        )),
                        _ => {
                            return Err(StoreError::AgentContractObservationInvariant {
                                code: "INVALID_DROPPED_TURN_OBSERVATION_OUTCOME",
                            });
                        }
                    };
                    let status = match outcome.as_str() {
                        "success" => "succeeded",
                        "clarify" => "clarify",
                        "error" => "error",
                        _ => unreachable!("outcome was validated above"),
                    };
                    (status, reason_code, session_id, recovery_message)
                }
                (None, Some(message)) => match message.kind.as_str() {
                    "agent_error" | "run_failed" => (
                        "error",
                        Some("TERMINAL_ERROR_PERSISTED".to_owned()),
                        message.ref_id.clone(),
                        None,
                    ),
                    "agent_interrupted" => (
                        "interrupted",
                        Some("USER_INTERRUPTED".to_owned()),
                        message.ref_id.clone(),
                        None,
                    ),
                    _ => ("succeeded", None, message.ref_id.clone(), None),
                },
                _ => {
                    if observation_started && let Some(observation_id) = observation_id {
                        let updated = sqlx::query(
                            r#"
                        UPDATE agent_contract_observations
                        SET outcome = 'error', reason_code = 'REQUEST_DROPPED',
                            completed_at = current_timestamp
                        WHERE id = ? AND workspace_id = ? AND outcome = 'started'
                        "#,
                        )
                        .bind(observation_id)
                        .bind(&current.workspace_id)
                        .execute(&mut *tx)
                        .await?;
                        if updated.rows_affected() != 1 {
                            return Err(StoreError::AgentContractObservationInvariant {
                                code: "DROPPED_TURN_OBSERVATION_CONFLICT",
                            });
                        }
                    }
                    (
                        "interrupted",
                        Some("REQUEST_DROPPED".to_owned()),
                        None,
                        Some((
                            "agent_interrupted",
                            "Agent 请求已中断。本轮已安全结束，可以重新发送。",
                        )),
                    )
                }
            };

        let message_id = recovery_message
            .filter(|_| existing_primary_message.is_none())
            .map(|_| new_id("msg"));
        if let (Some(message_id), Some((message_kind, message_text))) =
            (message_id.as_deref(), recovery_message)
        {
            sqlx::query(
                r#"
                INSERT INTO messages (
                    id, workspace_id, role, text, kind, ref_id, attachment_ids_json,
                    conversation_id, turn_id, created_at
                )
                VALUES (?, ?, 'agent', ?, ?, ?, NULL, ?, ?, current_timestamp)
                "#,
            )
            .bind(message_id)
            .bind(&current.workspace_id)
            .bind(message_text)
            .bind(message_kind)
            .bind(execution_id.as_deref())
            .bind(&current.conversation_id)
            .bind(turn_id)
            .execute(&mut *tx)
            .await?;
        }

        let updated = sqlx::query(
            r#"
            UPDATE agent_turns
            SET status = ?, reason_code = ?, execution_id = ?, completed_at = current_timestamp
            WHERE id = ? AND status = 'running'
            "#,
        )
        .bind(status)
        .bind(reason_code.as_deref())
        .bind(execution_id.as_deref())
        .bind(turn_id)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(None);
        }
        sqlx::query("UPDATE conversations SET updated_at = current_timestamp WHERE id = ?")
            .bind(&current.conversation_id)
            .execute(&mut *tx)
            .await?;

        let turn = sqlx::query_as::<_, AgentTurnRecord>(
            r#"
            SELECT id, conversation_id, workspace_id, user_message_id, execution_id, codex_turn_id,
                   mode, status, reason_code, started_at, completed_at
            FROM agent_turns
            WHERE id = ?
            "#,
        )
        .bind(turn_id)
        .fetch_one(&mut *tx)
        .await?;
        let message = match message_id {
            Some(message_id) => Some(
                sqlx::query_as::<_, MessageRecord>(
                    r#"
                    SELECT id, workspace_id, role, text, kind, ref_id, attachment_ids_json,
                           conversation_id, turn_id, created_at
                    FROM messages
                    WHERE id = ?
                    "#,
                )
                .bind(message_id)
                .fetch_one(&mut *tx)
                .await?,
            ),
            None => None,
        };
        tx.commit().await?;
        Ok(Some(DroppedAgentTurnSettlement { turn, message }))
    }

    /// A process-local runtime cannot resume an in-flight Helixflow turn after
    /// a server restart. Close those durable rows before accepting new work so
    /// clients never render an orphaned turn as running forever.
    pub async fn finalize_interrupted_agent_turns(&self) -> StoreResult<u64> {
        let updated = sqlx::query(
            r#"
            UPDATE agent_turns
            SET status = 'interrupted',
                reason_code = 'PROCESS_INTERRUPTED',
                completed_at = current_timestamp
            WHERE status = 'running'
            "#,
        )
        .execute(self.pool())
        .await?;
        Ok(updated.rows_affected())
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

    #[tokio::test]
    async fn finalizes_orphaned_running_turns_after_restart() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&format!(
            "sqlite://{}",
            dir.path().join("helixflow.sqlite").display()
        ))
        .await
        .expect("store");
        let workspace = store.create_workspace("Restart").await.expect("workspace");
        let conversation = store
            .create_conversation(&workspace.id, "Interrupted")
            .await
            .expect("conversation");
        let turn = store
            .start_agent_turn(&workspace.id, &conversation.id, "chat")
            .await
            .expect("turn");

        assert_eq!(store.finalize_interrupted_agent_turns().await.unwrap(), 1);
        assert_eq!(store.finalize_interrupted_agent_turns().await.unwrap(), 0);

        let interrupted = store.agent_turn(&turn.id).await.expect("interrupted turn");
        assert_eq!(interrupted.status, "interrupted");
        assert_eq!(
            interrupted.reason_code.as_deref(),
            Some("PROCESS_INTERRUPTED")
        );
        assert!(interrupted.completed_at.is_some());
    }
}
