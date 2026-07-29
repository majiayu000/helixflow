use helixflow_gateway::Provider;
use helixflow_graph::ExecutionPlan;
use helixflow_store::{RunFailureContinuationRecord, RunRecord};
use sha2::{Digest, Sha256};

use super::{RunResult, RunService};

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub(crate) async fn claim_restart_queued_run(&self, run: &RunRecord) -> RunResult<bool> {
        if !self.restart_intent_is_complete(run).await? {
            return Ok(false);
        }
        Ok(self
            .store
            .claim_run_if_workspace_idle(
                &run.id,
                "queued",
                "running",
                &run.workspace_id,
                run.group_id.as_deref(),
                self.ignore_quiescent_fix_children()?,
            )
            .await?
            .is_some())
    }

    pub(crate) async fn restart_intent_is_complete(&self, run: &RunRecord) -> RunResult<bool> {
        let Some(intent) = self.store.run_execution_intent(&run.id).await? else {
            return Ok(false);
        };
        let Some(plan_json) = run.plan_json.as_deref() else {
            return Ok(false);
        };
        let plan: ExecutionPlan = serde_json::from_str(plan_json)?;
        let plan_fingerprint = format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(&plan)?));
        let estimate_fingerprint = format!(
            "sha256:{:x}",
            Sha256::digest(run.estimate_json.as_deref().unwrap_or("").as_bytes())
        );
        if intent.plan_fingerprint != plan_fingerprint
            || intent.estimate_fingerprint != estimate_fingerprint
            || intent.cost_decision != "approved"
            || !self.store.provider_tasks_for_run(&run.id).await?.is_empty()
        {
            return Ok(false);
        }
        let provider_steps = plan
            .steps
            .iter()
            .filter(|step| step.provider.is_some())
            .collect::<Vec<_>>();
        let estimate_step_ids = self
            .store
            .cost_ledger_for_run(&run.id)
            .await?
            .into_iter()
            .filter(|cost| cost.estimated)
            .filter_map(|cost| cost.run_step_id)
            .collect::<BTreeSet<_>>();
        let run_steps = self.store.run_steps(&run.id).await?;
        let estimates_are_complete = provider_steps.iter().all(|plan_step| {
            run_steps
                .iter()
                .find(|run_step| run_step.node_id == plan_step.node_id)
                .is_some_and(|run_step| estimate_step_ids.contains(&run_step.id))
        });
        if !estimates_are_complete || provider_steps.len() != estimate_step_ids.len() {
            return Ok(false);
        }
        Ok(true)
    }

    pub(crate) async fn resume_retry_created(
        &self,
        continuation: &RunFailureContinuationRecord,
    ) -> RunResult<()> {
        if let Some(child_run_id) = continuation.child_run_id.as_deref() {
            let child = self.store.run(child_run_id).await?;
            if child.status == "waiting_confirmation" {
                self.start_confirmed_run_within_budget(child_run_id).await?;
            }
        }
        self.store
            .complete_failure_continuation(&continuation.run_id, false)
            .await?;
        Ok(())
    }
}
use std::collections::BTreeSet;
