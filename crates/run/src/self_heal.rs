use helixflow_gateway::Provider;
use helixflow_graph::ExecutionPlan;
use helixflow_store::RunRecord;
use serde_json::json;

use super::cost_gate::CostSummary;
use super::run_policy::{max_run_retries, run_requires_confirmation};
use super::{RunInterrupt, RunResult, RunService, RunStatus};

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    /// Execute a background run, then apply bounded failure self-repair.
    ///
    /// The first `(run, plan, interrupt)` is already claimed and steps-ensured
    /// by `start_confirmed_run`. When a run ends in `failed` and is within
    /// budget and the retry cap, a retry run is derived, claimed, and executed
    /// in the same loop. Iterative (not recursive) so the spawned future stays
    /// `Send`. The failed run and its `error_json` are preserved for audit.
    pub(crate) async fn run_with_self_heal(
        &self,
        mut run: RunRecord,
        mut plan: ExecutionPlan,
        mut interrupt: RunInterrupt,
    ) {
        loop {
            let run_id = run.id.clone();
            let workspace_id = run.workspace_id.clone();
            let attempt = run.attempt;

            let result = self
                .execute_created_run(&run, &workspace_id, &plan, interrupt, false)
                .await;
            match self.outcome(&run_id).await {
                Ok(outcome) => {
                    if let Err(err) = self.record_actual_costs(&outcome).await {
                        eprintln!("background run `{run_id}` failed to record costs: {err}");
                    }
                }
                Err(err) => eprintln!("background run `{run_id}` failed to load outcome: {err}"),
            }
            if let Err(err) = result {
                eprintln!("background run `{run_id}` failed: {err}");
            }
            self.interrupts.lock().await.remove(&run_id);

            match self.prepare_retry(&workspace_id, &run_id, attempt).await {
                Ok(Some((next_run, next_plan, next_interrupt))) => {
                    run = next_run;
                    plan = next_plan;
                    interrupt = next_interrupt;
                }
                Ok(None) => break,
                Err(err) => {
                    eprintln!("background run `{run_id}` self-heal failed: {err}");
                    break;
                }
            }
        }
    }

    /// Start a run only when its estimate is within the auto-run budget.
    /// Returns whether it was started; over-budget (or estimate-less) runs are
    /// left in `waiting_confirmation` for explicit user confirmation. Shares
    /// the same cost gate as self-repair so the two cannot drift.
    pub async fn start_confirmed_run_within_budget(&self, run_id: &str) -> RunResult<bool> {
        let run = self.store.run(run_id).await?;
        let within_budget = run
            .estimate_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<CostSummary>(json).ok())
            .map(|estimate| !run_requires_confirmation(&estimate))
            .unwrap_or(false);
        if within_budget {
            self.start_confirmed_run(run_id).await?;
        }
        Ok(within_budget)
    }

    /// Decide whether the just-finished run should be retried. Returns the
    /// prepared (claimed, steps-ensured) retry run when it should auto-start,
    /// or `None` to stop the loop (success, cap reached, no estimate, or over
    /// budget — over-budget retries wait in `waiting_confirmation`).
    async fn prepare_retry(
        &self,
        workspace_id: &str,
        run_id: &str,
        attempt: i64,
    ) -> RunResult<Option<(RunRecord, ExecutionPlan, RunInterrupt)>> {
        let run = self.store.run(run_id).await?;
        if run.status != RunStatus::Failed.as_str() {
            return Ok(None);
        }
        if attempt < 0 || attempt as u32 >= max_run_retries() {
            return Ok(None);
        }
        // Without an estimate we cannot budget-check the retry; leave failed.
        let Some(estimate_json) = run.estimate_json.as_deref() else {
            return Ok(None);
        };
        let estimate: CostSummary = serde_json::from_str(estimate_json)?;

        let child = self.store.create_retry_run(run_id).await?;
        let requires_confirmation = run_requires_confirmation(&estimate);
        self.emit(
            workspace_id,
            run_id,
            "run.retry",
            json!({
                "child_run_id": child.id,
                "attempt": child.attempt,
                "requires_confirmation": requires_confirmation,
            }),
        )
        .await?;

        if requires_confirmation {
            // Over budget: the retry waits for explicit user confirmation.
            self.emit(
                workspace_id,
                &child.id,
                "run.retry_pending",
                json!({ "parent_run_id": run_id, "estimate": estimate }),
            )
            .await?;
            return Ok(None);
        }

        // Within budget: claim (waiting_confirmation -> running) and ensure
        // steps, mirroring start_confirmed_run's preamble, then execute in
        // the next loop iteration.
        let (child_run, child_plan, child_interrupt) =
            self.claim_run_for_background(&child.id).await?;
        self.ensure_run_steps(&child_run.id, &child_plan).await?;
        Ok(Some((child_run, child_plan, child_interrupt)))
    }
}
