use std::path::PathBuf;

use serde_json::Value;

use crate::{
    AgentError, AgentResult, AgentRuntime, AgentService, AgentSessionRequest, AgentTurn,
    RuntimeEvent, create_session_contract, read_validated_design_artifact,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedDesignArtifact {
    pub session_id: String,
    pub title: String,
    pub summary: String,
    pub kind: String,
    pub entry_file: String,
    pub entry_path: PathBuf,
    pub files_dir: PathBuf,
    pub mime: String,
    pub meta: Value,
}

impl<R> AgentService<R>
where
    R: AgentRuntime,
{
    pub async fn create_design_artifact(
        &self,
        request: AgentSessionRequest,
    ) -> AgentResult<ValidatedDesignArtifact> {
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

        let artifact = read_validated_design_artifact(&session)?;
        self.emit_status(
            &session.workspace_id,
            &session.id,
            seq,
            "agent.status.end",
            serde_json::json!({
                "message": format!("Artifact ready: {}", artifact.title),
                "artifact_title": artifact.title
            }),
        );
        Ok(artifact)
    }
}
