use helixflow_gateway::Provider;
use helixflow_graph::ExecutionPlan;
use helixflow_store::RunRecord;

use super::{RunError, RunInterrupt, RunResult, RunService, RunStatus, SweepOutcome};

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub async fn start_confirmed_sweep(
        &self,
        run_ids: &[String],
        recommended_run_id: &str,
    ) -> RunResult<SweepOutcome> {
        let (group_id, runs) = self
            .validate_sweep_confirmation(run_ids, recommended_run_id)
            .await?;
        let mut claimed = Vec::new();
        for run in runs {
            let interrupt = RunInterrupt::default();
            self.interrupts
                .lock()
                .await
                .insert(run.id.clone(), interrupt);
            let Some(queued_run) = self
                .store
                .update_run_status_if_current(
                    &run.id,
                    RunStatus::WaitingConfirmation.as_str(),
                    RunStatus::Queued.as_str(),
                    None,
                )
                .await?
            else {
                self.interrupts.lock().await.remove(&run.id);
                self.rollback_queued_sweep_runs(&claimed).await?;
                let current = self.store.run(&run.id).await?;
                return Err(RunError::InvalidRunStatus {
                    run_id: current.id,
                    expected: RunStatus::WaitingConfirmation.as_str(),
                    actual: current.status,
                });
            };
            claimed.push(queued_run);
        }

        let mut outcomes = Vec::new();
        for run in &claimed {
            outcomes.push(self.outcome(&run.id).await?);
        }
        let queued_ids = claimed.iter().map(|run| run.id.clone()).collect::<Vec<_>>();
        self.spawn_confirmed_sweep(group_id.clone(), queued_ids, recommended_run_id.to_owned());

        Ok(SweepOutcome {
            group_id,
            runs: outcomes,
            artifacts: Vec::new(),
            recommendation: None,
        })
    }

    fn spawn_confirmed_sweep(
        &self,
        group_id: String,
        run_ids: Vec<String>,
        recommended_run_id: String,
    ) {
        let runner = self.clone();
        tokio::spawn(async move {
            if let Err(err) = runner
                .execute_started_sweep(&group_id, &run_ids, &recommended_run_id)
                .await
            {
                eprintln!("background sweep `{group_id}` failed: {err}");
            }
        });
    }

    async fn execute_started_sweep(
        &self,
        group_id: &str,
        run_ids: &[String],
        recommended_run_id: &str,
    ) -> RunResult<SweepOutcome> {
        for (index, run_id) in run_ids.iter().enumerate() {
            let Some(run) = self
                .store
                .update_run_status_if_current(
                    run_id,
                    RunStatus::Queued.as_str(),
                    RunStatus::Running.as_str(),
                    None,
                )
                .await?
            else {
                let current = self.store.run(run_id).await?;
                if current.status == RunStatus::Interrupted.as_str() {
                    self.interrupt_queued_sweep_runs(&run_ids[(index + 1)..])
                        .await?;
                    break;
                }
                return Err(RunError::InvalidRunStatus {
                    run_id: current.id,
                    expected: RunStatus::Queued.as_str(),
                    actual: current.status,
                });
            };
            let plan = execution_plan_for_run(&run)?;
            let interrupt = self.interrupt_for_background_run(&run.id).await?;
            let result = self
                .execute_created_run(&run, &run.workspace_id, &plan, interrupt, false)
                .await;
            self.interrupts.lock().await.remove(&run.id);
            let outcome = self.outcome(&run.id).await?;
            if result.is_err() || outcome.run.status == RunStatus::Interrupted.as_str() {
                self.interrupt_queued_sweep_runs(&run_ids[(index + 1)..])
                    .await?;
                break;
            }
        }

        self.finalize_sweep_outcome(group_id, run_ids, recommended_run_id)
            .await
    }

    async fn interrupt_for_background_run(&self, run_id: &str) -> RunResult<RunInterrupt> {
        self.interrupts
            .lock()
            .await
            .get(run_id)
            .cloned()
            .ok_or_else(|| RunError::RunNotActive(run_id.to_owned()))
    }

    async fn rollback_queued_sweep_runs(&self, runs: &[RunRecord]) -> RunResult<()> {
        for run in runs {
            let _ = self
                .store
                .update_run_status_if_current(
                    &run.id,
                    RunStatus::Queued.as_str(),
                    RunStatus::WaitingConfirmation.as_str(),
                    None,
                )
                .await?;
            self.interrupts.lock().await.remove(&run.id);
        }
        Ok(())
    }

    async fn interrupt_queued_sweep_runs(&self, run_ids: &[String]) -> RunResult<()> {
        for run_id in run_ids {
            let Some(run) = self
                .store
                .update_run_status_if_current(
                    run_id,
                    RunStatus::Queued.as_str(),
                    RunStatus::Interrupted.as_str(),
                    None,
                )
                .await?
            else {
                self.interrupts.lock().await.remove(run_id);
                continue;
            };
            let steps = self.store.run_steps(&run.id).await?;
            self.skip_steps(&run.workspace_id, &run.id, &steps).await?;
            self.emit(
                &run.workspace_id,
                &run.id,
                "run.interrupted",
                serde_json::json!({ "reason": "sweep_stopped" }),
            )
            .await?;
            self.interrupts.lock().await.remove(&run.id);
        }
        Ok(())
    }

    async fn finalize_sweep_outcome(
        &self,
        group_id: &str,
        run_ids: &[String],
        recommended_run_id: &str,
    ) -> RunResult<SweepOutcome> {
        let mut outcomes = Vec::new();
        for run_id in run_ids {
            outcomes.push(self.outcome(run_id).await?);
        }
        let mut artifacts = Vec::new();
        for outcome in &outcomes {
            artifacts.extend(outcome.artifacts.clone());
        }
        let all_succeeded = outcomes
            .iter()
            .all(|outcome| outcome.run.status == RunStatus::Succeeded.as_str());
        let selected_id = artifacts
            .iter()
            .find(|artifact| {
                all_succeeded
                    && artifact.run_id.as_deref() == Some(recommended_run_id)
                    && artifact.selected
            })
            .map(|artifact| artifact.id.clone());
        if all_succeeded && selected_id.is_none() {
            return Err(RunError::InvalidSweepPlan(
                "recommended run did not produce an output artifact".to_owned(),
            ));
        }

        let mut refreshed_artifacts = Vec::new();
        for artifact in artifacts {
            refreshed_artifacts.push(
                self.store
                    .update_artifact_selected(
                        &artifact.id,
                        selected_id.as_ref().is_some_and(|id| id == &artifact.id),
                    )
                    .await?,
            );
        }
        let recommendation = selected_id
            .as_deref()
            .map(|artifact_id| self.store.artifact(artifact_id));
        let recommendation = match recommendation {
            Some(fut) => Some(fut.await?),
            None => None,
        };
        if let Some(artifact) = &recommendation
            && let Some(run_id) = artifact.run_id.as_deref()
        {
            let run = self.store.run(run_id).await?;
            self.emit(
                &run.workspace_id,
                run_id,
                "run.succeeded",
                serde_json::json!({ "sweep_finalized": true }),
            )
            .await?;
        }

        Ok(SweepOutcome {
            group_id: group_id.to_owned(),
            runs: outcomes,
            artifacts: refreshed_artifacts,
            recommendation,
        })
    }
}

fn execution_plan_for_run(run: &RunRecord) -> RunResult<ExecutionPlan> {
    let plan_json = run
        .plan_json
        .as_deref()
        .ok_or_else(|| RunError::InvalidSweepPlan("run has no execution plan".to_owned()))?;
    Ok(serde_json::from_str(plan_json)?)
}
