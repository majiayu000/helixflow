use helixflow_gateway::Provider;
use helixflow_graph::ExecutionPlan;
use helixflow_store::RunRecord;
use serde_json::json;

use super::cost_types::CostSummary;
use super::run_policy::{max_run_retries, run_requires_confirmation};
use super::{RunError, RunInterrupt, RunResult, RunService, RunStatus};

enum RetryDecision {
    Continue(Box<RunRecord>, ExecutionPlan, RunInterrupt),
    Pending,
    Exhausted,
    Stop,
}

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
                .execute_created_run(&run, &workspace_id, &plan, interrupt, run.force_rerun)
                .await;
            if let Err(err) = result {
                eprintln!("background run failed: {}", err.public_message());
            }
            self.interrupts.lock().await.remove(&run_id);

            match self.prepare_retry(&workspace_id, &run_id, attempt).await {
                Ok(RetryDecision::Continue(next_run, next_plan, next_interrupt)) => {
                    run = *next_run;
                    plan = next_plan;
                    interrupt = next_interrupt;
                }
                Ok(RetryDecision::Pending | RetryDecision::Exhausted | RetryDecision::Stop) => {
                    break;
                }
                Err(err) => {
                    self.emit_retry_failure(&workspace_id, &run_id, &err).await;
                    eprintln!("background self-heal failed: {}", err.public_message());
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
        let estimate = run
            .estimate_json
            .as_deref()
            .map(serde_json::from_str::<CostSummary>)
            .transpose()?;
        let within_budget = match estimate {
            Some(estimate) => !run_requires_confirmation(&estimate)?,
            None => false,
        };
        if within_budget {
            self.start_confirmed_run(run_id).await?;
        }
        Ok(within_budget)
    }

    /// Derive a force-rerun child after an output is rejected and emit the
    /// same observable retry events used by failure self-repair.
    pub async fn retry_rejected_run(&self, parent_run_id: &str) -> RunResult<RunRecord> {
        let parent = self.store.run(parent_run_id).await?;
        if matches!(parent.status.as_str(), "queued" | "estimating" | "running") {
            return Err(RunError::InvalidRunStatus {
                run_id: parent.id,
                expected: "terminal",
                actual: parent.status,
            });
        }
        let estimate = parent
            .estimate_json
            .as_deref()
            .map(serde_json::from_str::<CostSummary>)
            .transpose()?;
        let requires_confirmation = match estimate.as_ref() {
            Some(estimate) => run_requires_confirmation(estimate)?,
            None => true,
        };
        let (child, created) = self
            .store
            .create_reject_retry_run_once(parent_run_id)
            .await?;
        if created {
            self.emit(
                &parent.workspace_id,
                &parent.id,
                "run.retry",
                json!({
                    "child_run_id": child.id,
                    "attempt": child.attempt,
                    "requires_confirmation": requires_confirmation,
                    "previous_error": parent.error_json,
                    "reason": "output_rejected",
                }),
            )
            .await?;
            if requires_confirmation {
                self.emit(
                    &child.workspace_id,
                    &child.id,
                    "run.retry_pending",
                    json!({
                        "parent_run_id": parent.id,
                        "estimate": estimate,
                        "reason": "output_rejected",
                    }),
                )
                .await?;
            }
        }
        if !requires_confirmation && child.status == RunStatus::WaitingConfirmation.as_str() {
            if let Err(err) = self.start_confirmed_run(&child.id).await
                && !concurrent_reject_retry_start_lost(&err)
            {
                self.emit_retry_failure(&child.workspace_id, &child.id, &err)
                    .await;
                return Err(err);
            }
        }
        Ok(self.store.run(&child.id).await?)
    }

    pub(crate) async fn continue_self_heal_from_failed(&self, run: &RunRecord) -> RunResult<()> {
        match self
            .prepare_retry(&run.workspace_id, &run.id, run.attempt)
            .await
        {
            Ok(RetryDecision::Continue(next_run, next_plan, next_interrupt)) => {
                self.run_with_self_heal(*next_run, next_plan, next_interrupt)
                    .await;
                Ok(())
            }
            Ok(RetryDecision::Pending | RetryDecision::Exhausted | RetryDecision::Stop) => Ok(()),
            Err(err) => {
                self.emit_retry_failure(&run.workspace_id, &run.id, &err)
                    .await;
                Err(err)
            }
        }
    }

    async fn emit_retry_failure(&self, workspace_id: &str, run_id: &str, err: &RunError) {
        if let Err(emit_err) = self
            .emit(
                workspace_id,
                run_id,
                "run.retry_failed",
                json!({ "error": err.public_message() }),
            )
            .await
        {
            eprintln!("run `{run_id}` failed to persist retry failure `{err}`: {emit_err}");
        }
    }

    /// Decide whether the just-finished run should be retried. Returns the
    /// prepared (claimed, steps-ensured) retry run when it should auto-start.
    /// Pending confirmation, explicit exhaustion, and non-retryable stops stay
    /// distinct so retry state remains durable and observable.
    async fn prepare_retry(
        &self,
        workspace_id: &str,
        run_id: &str,
        attempt: i64,
    ) -> RunResult<RetryDecision> {
        let run = self.store.run(run_id).await?;
        if run.status != RunStatus::Failed.as_str() {
            return Ok(RetryDecision::Stop);
        }
        if attempt < 0 || attempt as u32 >= max_run_retries()? {
            self.store
                .complete_failure_continuation(run_id, true)
                .await?;
            return Ok(RetryDecision::Exhausted);
        }
        // Without an estimate we cannot budget-check the retry; leave failed.
        let Some(estimate_json) = run.estimate_json.as_deref() else {
            self.store
                .complete_failure_continuation(run_id, false)
                .await?;
            return Ok(RetryDecision::Stop);
        };
        let estimate: CostSummary = serde_json::from_str(estimate_json)?;

        let requires_confirmation = run_requires_confirmation(&estimate)?;
        let child = self.store.create_retry_run_once(run_id, false).await?;
        self.emit(
            workspace_id,
            run_id,
            "run.retry",
            json!({
                "child_run_id": child.id,
                "attempt": child.attempt,
                "requires_confirmation": requires_confirmation,
                "previous_error": run.error_json,
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
            self.store
                .complete_failure_continuation(run_id, false)
                .await?;
            return Ok(RetryDecision::Pending);
        }

        // Within budget: claim (waiting_confirmation -> running) and ensure
        // steps, mirroring start_confirmed_run's preamble, then execute in
        // the next loop iteration.
        let (child_run, child_plan, child_interrupt) =
            self.claim_run_for_background(&child.id).await?;
        self.ensure_run_steps(&child_run.id, &child_plan).await?;
        self.store
            .complete_failure_continuation(run_id, false)
            .await?;
        Ok(RetryDecision::Continue(
            Box::new(child_run),
            child_plan,
            child_interrupt,
        ))
    }
}

fn concurrent_reject_retry_start_lost(err: &RunError) -> bool {
    matches!(
        err,
        RunError::InvalidRunStatus { .. } | RunError::RunClaimContention(_)
    )
}
