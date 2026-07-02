use serde::Deserialize;

use crate::{
    AgentError, AgentResult, AgentRuntime, AgentService, AgentSession, AgentSessionRequest,
    AgentTurn, RuntimeEvent, create_session_contract, read_output_file,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedAgentChat {
    pub session_id: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatOutput {
    message: String,
}

impl<R> AgentService<R>
where
    R: AgentRuntime,
{
    pub async fn answer_chat(
        &self,
        request: AgentSessionRequest,
    ) -> AgentResult<ValidatedAgentChat> {
        let turn = AgentTurn {
            message: request.user_message.clone(),
            skill: request.skill,
        };
        let session = create_session_contract(&request)?;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            1,
            "ctx.created",
            serde_json::json!({}),
        );
        let handle = self
            .runtime
            .start(session.clone())
            .await
            .map_err(|err| AgentError::Runtime(err.to_string()))?;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            2,
            "runtime.started",
            serde_json::json!({}),
        );
        self.runtime
            .send(&handle, turn)
            .await
            .map_err(|err| AgentError::Runtime(err.to_string()))?;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            3,
            "turn.sent",
            serde_json::json!({}),
        );

        let mut seq = 4;
        while let Some(event) = self.runtime.next_event(&handle).await {
            match event {
                RuntimeEvent::Status { message } => {
                    self.emit_status(
                        &session.workspace_id,
                        &session.id,
                        seq,
                        "runtime.status",
                        serde_json::json!({ "message": message }),
                    );
                    seq += 1;
                }
                RuntimeEvent::Log {
                    kind,
                    label,
                    text,
                    raw_json,
                } => {
                    self.emit_status(
                        &session.workspace_id,
                        &session.id,
                        seq,
                        "runtime.log",
                        serde_json::json!({
                            "kind": kind,
                            "label": label,
                            "text": text,
                            "raw": raw_json
                        }),
                    );
                    seq += 1;
                }
                RuntimeEvent::Failed { message } => {
                    return Err(AgentError::Runtime(message));
                }
                RuntimeEvent::Finished => break,
            }
        }

        let reply = read_validated_chat_reply(&session)?;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            seq,
            "agent.status.end",
            serde_json::json!({ "message": "chat reply ready" }),
        );
        Ok(reply)
    }
}

pub fn read_validated_chat_reply(session: &AgentSession) -> AgentResult<ValidatedAgentChat> {
    let output_path = session.out_dir.join("reply.json");
    let output: ChatOutput =
        serde_json::from_slice(&read_output_file(&session.out_dir, &output_path)?)?;
    let message = output.message.trim().to_owned();
    if message.is_empty() {
        return Err(AgentError::InvalidOutputFile {
            path: output_path,
            reason: "chat reply message is empty".to_owned(),
        });
    }
    Ok(ValidatedAgentChat {
        session_id: session.id.clone(),
        message,
    })
}
