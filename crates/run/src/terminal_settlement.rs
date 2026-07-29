use std::time::Duration;

use helixflow_gateway::Provider;
use helixflow_store::{
    PROVIDER_TASK_ACTIVE, PROVIDER_TASK_DISPATCHING, PROVIDER_TASK_RESULT_READY,
};
use serde_json::json;
use uuid::Uuid;

use super::provider_recovery::durable_task;
use super::{RunEventEnvelope, RunResult, RunService};

const TERMINALIZATION_LEASE_SECONDS: i64 = 60;
const REMOTE_CANCEL_TIMEOUT: Duration = Duration::from_secs(30);

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub(crate) async fn request_and_settle_terminal(
        &self,
        workspace_id: &str,
        run_id: &str,
        desired_status: &str,
        error_json: Option<&str>,
    ) -> RunResult<()> {
        self.store
            .request_run_terminalization(run_id, desired_status, error_json)
            .await?;
        let owner_id = format!("settler-{}", Uuid::now_v7());
        loop {
            if self
                .store
                .claim_run_terminalization(run_id, &owner_id, TERMINALIZATION_LEASE_SECONDS)
                .await?
            {
                break;
            }
            let run = self.store.run(run_id).await?;
            if !matches!(run.status.as_str(), "queued" | "estimating" | "running") {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        loop {
            if !self
                .store
                .renew_run_terminalization_lease(run_id, &owner_id, TERMINALIZATION_LEASE_SECONDS)
                .await?
            {
                return Ok(());
            }
            let mut wait_for_durable_owner = false;
            for task in self.store.provider_tasks_for_run(run_id).await? {
                match task.state.as_str() {
                    PROVIDER_TASK_ACTIVE => {
                        match tokio::time::timeout(
                            REMOTE_CANCEL_TIMEOUT,
                            self.provider.cancel_durable(&durable_task(&task)?),
                        )
                        .await
                        {
                            Ok(Ok(())) => {
                                self.store.cancel_provider_task(&task.id, None).await?;
                                self.emit(
                                    workspace_id,
                                    run_id,
                                    "run.recovery_cancelled",
                                    json!({ "provider": task.provider }),
                                )
                                .await?;
                            }
                            Ok(Err(_)) | Err(_) => {
                                self.store
                                    .abandon_provider_task(
                                        &task.id,
                                        PROVIDER_TASK_ACTIVE,
                                        "REMOTE_CANCEL_UNCONFIRMED",
                                    )
                                    .await?;
                                self.persist_billing_risk(
                                    workspace_id,
                                    run_id,
                                    &task.provider,
                                    "cancel_unconfirmed",
                                )
                                .await?;
                            }
                        }
                    }
                    PROVIDER_TASK_DISPATCHING => {
                        if self
                            .store
                            .provider_task_timing(&task.id)
                            .await?
                            .dispatch_owner_live
                        {
                            wait_for_durable_owner = true;
                            continue;
                        }
                        self.store
                            .abandon_provider_task(
                                &task.id,
                                PROVIDER_TASK_DISPATCHING,
                                "DISPATCH_OWNER_STOPPED",
                            )
                            .await?;
                        self.persist_billing_risk(
                            workspace_id,
                            run_id,
                            &task.provider,
                            "dispatch_unknown",
                        )
                        .await?;
                    }
                    PROVIDER_TASK_RESULT_READY => {
                        if self
                            .store
                            .provider_task_timing(&task.id)
                            .await?
                            .materialization_expired
                        {
                            self.store
                                .complete_provider_task(
                                    &task.id,
                                    PROVIDER_TASK_RESULT_READY,
                                    Some("ARTIFACT_MATERIALIZATION_DEADLINE"),
                                )
                                .await?;
                        } else {
                            wait_for_durable_owner = true;
                        }
                    }
                    _ => {}
                }
            }
            if wait_for_durable_owner {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            break;
        }
        if let Some(run) = self
            .store
            .complete_run_terminalization(run_id, &owner_id)
            .await?
        {
            let event_name = format!("run.{}", run.status);
            if let Some(event) = self
                .store
                .run_events(run_id)
                .await?
                .into_iter()
                .rev()
                .find(|event| event.ev == event_name)
            {
                let data = serde_json::from_str(&event.data_json)?;
                drop(self.events.publish(RunEventEnvelope {
                    workspace_id: workspace_id.to_owned(),
                    run_id: event.run_id,
                    seq: event.seq,
                    server_time: event.created_at,
                    ev: event.ev,
                    data,
                }));
            }
        }
        Ok(())
    }
}
