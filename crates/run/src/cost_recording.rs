use helixflow_gateway::{CostEstimate, Provider};
use helixflow_store::NewCostLedger;

use super::{RunOutcome, RunResult, RunService};

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub(crate) async fn record_actual_costs(&self, outcome: &RunOutcome) -> RunResult<()> {
        for step in &outcome.steps {
            let (Some(provider), Some(cost_json)) = (&step.provider, &step.cost_actual_json) else {
                continue;
            };
            if self.store.provider_task_for_step(&step.id).await?.is_some() {
                continue;
            }
            let cost: CostEstimate = serde_json::from_str(cost_json)?;
            self.store
                .create_cost_ledger_once(
                    &format!("actual:{}", step.id),
                    NewCostLedger {
                        workspace_id: &outcome.run.workspace_id,
                        run_id: Some(&outcome.run.id),
                        run_step_id: Some(&step.id),
                        provider,
                        amount: cost.amount,
                        currency: &cost.currency,
                        estimated: cost.estimated,
                    },
                )
                .await?;
        }
        Ok(())
    }
}
