use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::{Json, extract::Path, extract::State};
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
    use super::ActiveAgentTurns;

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
}
