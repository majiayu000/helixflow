use helixflow_gateway::Provider;
use helixflow_store::NewMessage;
use serde_json::json;

use super::{RunResult, RunService};

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub(crate) async fn persist_billing_risk(
        &self,
        workspace_id: &str,
        run_id: &str,
        provider: &str,
        reason_code: &str,
    ) -> RunResult<()> {
        let message = "远端任务终态无法确认，可能继续产生费用。";
        let ref_id = format!("recovery-risk:{run_id}:{provider}:{reason_code}");
        self.store
            .create_message_once(NewMessage {
                workspace_id,
                role: "system",
                kind: "run_failed",
                text: Some(message),
                ref_id: Some(&ref_id),
                attachment_ids_json: None,
            })
            .await?;
        self.emit(
            workspace_id,
            run_id,
            "run.recovery_abandoned",
            json!({
                "provider": provider,
                "reason_code": reason_code,
                "message": message
            }),
        )
        .await
    }
}
