use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::{Json, extract::Path, extract::State};
use helixflow_store::Store;
use serde::Serialize;
use tokio::sync::oneshot;

use crate::api_error::ApiError;
use crate::app_state::AppState;

#[derive(Clone, Default)]
pub(crate) struct ActiveAgentTurns {
    inner: Arc<Mutex<BTreeMap<String, ActiveAgentTurn>>>,
}

struct ActiveAgentTurn {
    turn_id: String,
    interrupt: Option<oneshot::Sender<()>>,
}

pub(crate) struct ActiveAgentTurnGuard {
    active: ActiveAgentTurns,
    workspace_id: String,
    turn_id: String,
}

pub(crate) struct DurableAgentTurnGuard {
    store: Store,
    turn_id: String,
    observation_id: Option<String>,
    armed: bool,
}

impl DurableAgentTurnGuard {
    pub(crate) fn new(store: Store, turn_id: &str, observation_id: Option<&str>) -> Self {
        Self {
            store,
            turn_id: turn_id.to_owned(),
            observation_id: observation_id.map(str::to_owned),
            armed: true,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }

    pub(crate) fn attach_observation(&mut self, observation_id: &str) {
        self.observation_id = Some(observation_id.to_owned());
    }
}

impl ActiveAgentTurns {
    pub(crate) fn register(
        &self,
        workspace_id: &str,
        turn_id: &str,
    ) -> Result<(ActiveAgentTurnGuard, oneshot::Receiver<()>), ApiError> {
        let mut active = self
            .inner
            .lock()
            .map_err(|_| ApiError::server_error("active agent turn registry is unavailable"))?;
        if let Some(existing) = active.get(workspace_id) {
            return Err(ApiError::conflict(format!(
                "workspace already has an active agent turn `{}`",
                existing.turn_id
            )));
        }
        let (interrupt, receiver) = oneshot::channel();
        active.insert(
            workspace_id.to_owned(),
            ActiveAgentTurn {
                turn_id: turn_id.to_owned(),
                interrupt: Some(interrupt),
            },
        );
        Ok((
            ActiveAgentTurnGuard {
                active: self.clone(),
                workspace_id: workspace_id.to_owned(),
                turn_id: turn_id.to_owned(),
            },
            receiver,
        ))
    }

    fn interrupt(&self, workspace_id: &str) -> Result<Option<String>, ApiError> {
        let mut active = self
            .inner
            .lock()
            .map_err(|_| ApiError::server_error("active agent turn registry is unavailable"))?;
        let Some(turn) = active.get_mut(workspace_id) else {
            return Ok(None);
        };
        let turn_id = turn.turn_id.clone();
        if let Some(interrupt) = turn.interrupt.take() {
            let _ = interrupt.send(());
        }
        Ok(Some(turn_id))
    }
}

impl Drop for ActiveAgentTurnGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.inner.lock()
            && active
                .get(&self.workspace_id)
                .is_some_and(|turn| turn.turn_id == self.turn_id)
        {
            active.remove(&self.workspace_id);
        }
    }
}

impl Drop for DurableAgentTurnGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let store = self.store.clone();
        let turn_id = self.turn_id.clone();
        let observation_id = self.observation_id.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let Err(error) = store
                    .settle_dropped_agent_turn(&turn_id, observation_id.as_deref())
                    .await
                {
                    eprintln!("failed to settle dropped Agent turn `{turn_id}`: {error}");
                }
            });
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InterruptAgentTurnResponse {
    pub(crate) turn_id: String,
    pub(crate) status: &'static str,
}

pub(crate) async fn interrupt_workspace_agent_turn(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<InterruptAgentTurnResponse>, ApiError> {
    let turn_id = state
        .active_agent_turns
        .interrupt(&workspace_id)?
        .ok_or_else(|| ApiError::conflict("workspace has no active agent turn"))?;
    Ok(Json(InterruptAgentTurnResponse {
        turn_id,
        status: "interrupt_requested",
    }))
}

#[cfg(test)]
mod tests {
    use super::{ActiveAgentTurns, DurableAgentTurnGuard};
    use helixflow_store::{
        AgentContractMode, AgentContractOutcome, CompleteAgentContractObservation,
        NewAgentContractObservation, NewMessage, Store,
    };

    #[tokio::test]
    async fn registry_interrupts_one_turn_and_releases_workspace_on_drop() {
        let active = ActiveAgentTurns::default();
        let (guard, receiver) = active.register("ws_1", "turn_1").expect("register");
        assert!(active.register("ws_1", "turn_2").is_err());

        assert_eq!(active.interrupt("ws_1").unwrap().as_deref(), Some("turn_1"));
        receiver.await.expect("interrupt delivered");
        drop(guard);

        assert!(active.register("ws_1", "turn_2").is_ok());
    }

    #[tokio::test]
    async fn dropped_request_guard_persists_one_visible_terminal_turn() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&format!(
            "sqlite://{}",
            dir.path().join("helixflow.sqlite").display()
        ))
        .await
        .expect("store");
        let workspace = store
            .create_workspace("Drop guard")
            .await
            .expect("workspace");
        let conversation = store
            .create_conversation(&workspace.id, "Dropped")
            .await
            .expect("conversation");
        let turn = store
            .start_agent_turn(&workspace.id, &conversation.id, "chat")
            .await
            .expect("turn");
        let user = store
            .create_message(NewMessage {
                workspace_id: &workspace.id,
                role: "user",
                kind: "text",
                text: Some("continue"),
                ref_id: None,
                attachment_ids_json: None,
                conversation_id: Some(&conversation.id),
                turn_id: Some(&turn.id),
            })
            .await
            .expect("user message");
        store
            .attach_turn_user_message(&turn.id, &user.id)
            .await
            .expect("attach");

        let started = std::sync::Arc::new(tokio::sync::Notify::new());
        let task_started = started.clone();
        let guard = DurableAgentTurnGuard::new(store.clone(), &turn.id, None);
        let request = tokio::spawn(async move {
            let _guard = guard;
            task_started.notify_one();
            std::future::pending::<()>().await;
        });
        started.notified().await;
        request.abort();
        assert!(
            request
                .await
                .expect_err("request must be cancelled")
                .is_cancelled()
        );

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if store.agent_turn(&turn.id).await.expect("turn state").status != "running" {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("drop settlement");

        let terminal = store.agent_turn(&turn.id).await.expect("terminal turn");
        assert_eq!(terminal.status, "interrupted");
        assert_eq!(terminal.reason_code.as_deref(), Some("REQUEST_DROPPED"));
        let messages = store
            .conversation_messages(&conversation.id)
            .await
            .expect("messages");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.role == "agent"
                    && message.turn_id.as_deref() == Some(&turn.id))
                .count(),
            1
        );
        assert_eq!(
            messages.last().expect("terminal message").kind,
            "agent_interrupted"
        );
    }

    #[tokio::test]
    async fn dropped_graph_request_terminalizes_turn_and_observation_together() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&format!(
            "sqlite://{}",
            dir.path().join("helixflow.sqlite").display()
        ))
        .await
        .expect("store");
        let workspace = store
            .create_workspace("Graph drop guard")
            .await
            .expect("workspace");
        let conversation = store
            .create_conversation(&workspace.id, "Dropped graph edit")
            .await
            .expect("conversation");
        let turn = store
            .start_agent_turn(&workspace.id, &conversation.id, "create_workflow")
            .await
            .expect("turn");
        let started = store
            .create_graph_edit_message_with_observation(NewAgentContractObservation {
                user_message: NewMessage {
                    workspace_id: &workspace.id,
                    role: "user",
                    kind: "text",
                    text: Some("create an image workflow"),
                    ref_id: None,
                    attachment_ids_json: None,
                    conversation_id: Some(&conversation.id),
                    turn_id: Some(&turn.id),
                },
                contract_mode: AgentContractMode::Intent,
                release_id: Some("v0.2.0"),
                build_revision: Some("test"),
            })
            .await
            .expect("observation");
        store
            .attach_turn_user_message(&turn.id, &started.message.id)
            .await
            .expect("attach");

        let guard =
            DurableAgentTurnGuard::new(store.clone(), &turn.id, Some(&started.observation.id));
        let request = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        request.abort();
        assert!(
            request
                .await
                .expect_err("request must be cancelled")
                .is_cancelled()
        );

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if store.agent_turn(&turn.id).await.expect("turn state").status != "running" {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("drop settlement");

        let terminal = store.agent_turn(&turn.id).await.expect("terminal turn");
        let observation = store
            .agent_contract_observation(&started.observation.id)
            .await
            .expect("terminal observation");
        assert_eq!(terminal.status, "interrupted");
        assert_eq!(terminal.reason_code.as_deref(), Some("REQUEST_DROPPED"));
        assert_eq!(observation.outcome, "error");
        assert_eq!(observation.reason_code.as_deref(), Some("REQUEST_DROPPED"));
        assert!(terminal.completed_at.is_some());
        assert!(observation.completed_at.is_some());
    }

    #[tokio::test]
    async fn dropped_request_recovers_a_completed_observation_as_success() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&format!(
            "sqlite://{}",
            dir.path().join("helixflow.sqlite").display()
        ))
        .await
        .expect("store");
        let workspace = store
            .create_workspace("Recovered")
            .await
            .expect("workspace");
        let conversation = store
            .create_conversation(&workspace.id, "Recovered")
            .await
            .expect("conversation");
        let turn = store
            .start_agent_turn(&workspace.id, &conversation.id, "create_workflow")
            .await
            .expect("turn");
        let started = store
            .create_graph_edit_message_with_observation(NewAgentContractObservation {
                user_message: NewMessage {
                    workspace_id: &workspace.id,
                    role: "user",
                    kind: "text",
                    text: Some("create a workflow"),
                    ref_id: None,
                    attachment_ids_json: None,
                    conversation_id: Some(&conversation.id),
                    turn_id: Some(&turn.id),
                },
                contract_mode: AgentContractMode::Intent,
                release_id: Some("v0.2.0"),
                build_revision: Some("test"),
            })
            .await
            .expect("observation");
        store
            .finalize_agent_contract_observation(CompleteAgentContractObservation {
                observation_id: &started.observation.id,
                workspace_id: &workspace.id,
                contract_mode: AgentContractMode::Intent,
                outcome: AgentContractOutcome::Success,
                reason_code: "INTENT_COMPILED",
                session_id: Some("session_1"),
            })
            .await
            .expect("complete observation");

        drop(DurableAgentTurnGuard::new(
            store.clone(),
            &turn.id,
            Some(&started.observation.id),
        ));
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if store.agent_turn(&turn.id).await.expect("turn state").status != "running" {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("drop settlement");

        assert_eq!(
            store.agent_turn(&turn.id).await.expect("turn").status,
            "succeeded"
        );
        let messages = store
            .conversation_messages(&conversation.id)
            .await
            .expect("messages");
        assert_eq!(
            messages.last().expect("recovery message").kind,
            "agent_status"
        );
    }
}
