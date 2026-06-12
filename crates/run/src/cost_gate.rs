use std::collections::{BTreeMap, BTreeSet};

use helixflow_gateway::{ArtifactRef, CostEstimate, Provider, ProviderRequest};
use helixflow_graph::WorkflowGraph;
use helixflow_store::{
    ArtifactRecord, CostLedgerRecord, NewCostLedger, NewRun, RunRecord, RunStepRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use super::{RunError, RunInterrupt, RunOutcome, RunResult, RunService, RunStatus};

#[derive(Debug, Clone)]
pub struct AgentRunRequest {
    pub workspace_id: String,
    pub version_id: String,
    pub group_id: Option<String>,
    pub label: String,
    pub graph: WorkflowGraph,
}

#[derive(Debug, Clone)]
pub struct SweepPlan {
    pub workspace_id: String,
    pub version_id: String,
    pub label: String,
    pub variants: Vec<SweepVariant>,
}

#[derive(Debug, Clone)]
pub struct SweepVariant {
    pub label: String,
    pub graph: WorkflowGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostSummary {
    pub amount: f64,
    pub currency: String,
    pub estimated: bool,
}

#[derive(Debug, Clone)]
pub struct PendingRun {
    pub run: RunRecord,
    pub steps: Vec<RunStepRecord>,
    pub estimate: CostSummary,
    pub ledger: Vec<CostLedgerRecord>,
}

#[derive(Debug, Clone)]
pub struct PendingSweep {
    pub group_id: String,
    pub runs: Vec<PendingRun>,
    pub estimate: CostSummary,
}

#[derive(Debug, Clone)]
pub struct SweepOutcome {
    pub group_id: String,
    pub runs: Vec<RunOutcome>,
    pub artifacts: Vec<ArtifactRecord>,
    pub recommendation: ArtifactRecord,
}

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub async fn request_agent_run(&self, request: AgentRunRequest) -> RunResult<PendingRun> {
        self.request_confirmed_run("agent", request).await
    }

    pub async fn confirm_run(&self, run_id: &str) -> RunResult<RunOutcome> {
        let (run, plan) = self.claim_run_for_confirmation(run_id).await?;
        self.execute_confirmed_run(run, plan).await
    }

    async fn claim_run_for_confirmation(
        &self,
        run_id: &str,
    ) -> RunResult<(RunRecord, helixflow_graph::ExecutionPlan)> {
        let run = self.store.run(run_id).await?;
        if run.status != RunStatus::WaitingConfirmation.as_str() {
            return Err(RunError::InvalidRunStatus {
                run_id: run.id,
                expected: RunStatus::WaitingConfirmation.as_str(),
                actual: run.status,
            });
        }
        let plan_json = run
            .plan_json
            .as_deref()
            .ok_or_else(|| RunError::InvalidSweepPlan("run has no execution plan".to_owned()))?;
        let plan = serde_json::from_str(plan_json)?;
        let Some(run) = self
            .store
            .update_run_status_if_current(
                run_id,
                RunStatus::WaitingConfirmation.as_str(),
                RunStatus::Running.as_str(),
                None,
            )
            .await?
        else {
            let current = self.store.run(run_id).await?;
            return Err(RunError::InvalidRunStatus {
                run_id: current.id,
                expected: RunStatus::WaitingConfirmation.as_str(),
                actual: current.status,
            });
        };

        Ok((run, plan))
    }

    async fn execute_confirmed_run(
        &self,
        run: RunRecord,
        plan: helixflow_graph::ExecutionPlan,
    ) -> RunResult<RunOutcome> {
        let interrupt = RunInterrupt::default();
        self.interrupts
            .lock()
            .await
            .insert(run.id.clone(), interrupt.clone());

        let result = self
            .execute_created_run(&run, &run.workspace_id, &plan, interrupt)
            .await;
        self.interrupts.lock().await.remove(&run.id);

        let outcome = self.outcome(&run.id).await?;
        self.record_actual_costs(&outcome).await?;
        match result {
            Ok(_) => Ok(outcome),
            Err(err) => Err(err),
        }
    }

    pub async fn request_sweep_plan(&self, plan: SweepPlan) -> RunResult<PendingSweep> {
        validate_sweep_plan(&plan)?;
        let group_id = format!("sweep_{}", Uuid::now_v7().simple());
        let mut runs = Vec::new();
        let mut total = CostSummary::zero();

        for variant in plan.variants {
            let pending = self
                .request_confirmed_run(
                    "sweep",
                    AgentRunRequest {
                        workspace_id: plan.workspace_id.clone(),
                        version_id: plan.version_id.clone(),
                        group_id: Some(group_id.clone()),
                        label: format!("{} · {}", plan.label, variant.label),
                        graph: variant.graph,
                    },
                )
                .await?;
            total.add(&pending.estimate)?;
            runs.push(pending);
        }

        Ok(PendingSweep {
            group_id,
            runs,
            estimate: total,
        })
    }

    pub async fn confirm_sweep_runs(
        &self,
        run_ids: &[String],
        recommended_run_id: &str,
    ) -> RunResult<SweepOutcome> {
        let (group_id, runs) = self
            .validate_sweep_confirmation(run_ids, recommended_run_id)
            .await?;
        let mut claimed: Vec<String> = Vec::new();
        for run in runs {
            let Some(claimed_run) = self
                .store
                .update_run_status_if_current(
                    &run.id,
                    RunStatus::WaitingConfirmation.as_str(),
                    RunStatus::Running.as_str(),
                    None,
                )
                .await?
            else {
                self.revert_claimed_runs(&claimed).await?;
                let current = self.store.run(&run.id).await?;
                return Err(RunError::InvalidRunStatus {
                    run_id: current.id,
                    expected: RunStatus::WaitingConfirmation.as_str(),
                    actual: current.status,
                });
            };
            claimed.push(claimed_run.id.clone());
        }

        let mut outcomes = Vec::new();
        for (index, run_id) in claimed.iter().enumerate() {
            let run = self.store.run(run_id).await?;
            let plan_json = run.plan_json.as_deref().ok_or_else(|| {
                RunError::InvalidSweepPlan("run has no execution plan".to_owned())
            })?;
            let plan = serde_json::from_str(plan_json)?;
            match self.execute_confirmed_run(run, plan).await {
                Ok(outcome) => outcomes.push(outcome),
                Err(err) => {
                    self.revert_claimed_runs(&claimed[(index + 1)..]).await?;
                    return Err(err);
                }
            }
        }

        let mut artifacts = Vec::new();
        for outcome in &outcomes {
            artifacts.extend(outcome.artifacts.clone());
        }
        let recommendation = artifacts
            .iter()
            .find(|artifact| {
                artifact.run_id.as_deref() == Some(recommended_run_id) && artifact.selected
            })
            .cloned()
            .ok_or_else(|| {
                RunError::InvalidSweepPlan(
                    "recommended run did not produce an output artifact".to_owned(),
                )
            })?;

        let mut refreshed_artifacts = Vec::new();
        for artifact in artifacts {
            refreshed_artifacts.push(
                self.store
                    .update_artifact_selected(&artifact.id, artifact.id == recommendation.id)
                    .await?,
            );
        }
        let recommendation = self.store.artifact(&recommendation.id).await?;

        Ok(SweepOutcome {
            group_id,
            runs: outcomes,
            artifacts: refreshed_artifacts,
            recommendation,
        })
    }

    async fn validate_sweep_confirmation(
        &self,
        run_ids: &[String],
        recommended_run_id: &str,
    ) -> RunResult<(String, Vec<RunRecord>)> {
        if run_ids.is_empty() {
            return Err(RunError::InvalidSweepPlan(
                "sweep must include at least one run".to_owned(),
            ));
        }
        if !run_ids.iter().any(|run_id| run_id == recommended_run_id) {
            return Err(RunError::InvalidSweepPlan(
                "recommended run must be part of the sweep".to_owned(),
            ));
        }
        let mut unique = BTreeSet::new();
        for run_id in run_ids {
            if !unique.insert(run_id) {
                return Err(RunError::InvalidSweepPlan(
                    "sweep run ids must be unique".to_owned(),
                ));
            }
        }

        let mut group_id = None;
        let mut runs = Vec::new();
        for run_id in run_ids {
            let run = self.store.run(run_id).await?;
            if run.trigger != "sweep" {
                return Err(RunError::InvalidSweepPlan(
                    "all confirmed runs must be sweep-triggered".to_owned(),
                ));
            }
            if run.status != RunStatus::WaitingConfirmation.as_str() {
                return Err(RunError::InvalidRunStatus {
                    run_id: run.id,
                    expected: RunStatus::WaitingConfirmation.as_str(),
                    actual: run.status,
                });
            }
            let run_group_id = run.group_id.clone().ok_or_else(|| {
                RunError::InvalidSweepPlan("sweep runs must share a group id".to_owned())
            })?;
            if let Some(existing_group_id) = &group_id {
                if run_group_id != *existing_group_id {
                    return Err(RunError::InvalidSweepPlan(
                        "sweep run group ids do not match".to_owned(),
                    ));
                }
            } else {
                group_id = Some(run_group_id);
            }
            runs.push(run);
        }

        let group_id = group_id.ok_or_else(|| {
            RunError::InvalidSweepPlan("sweep runs must share a group id".to_owned())
        })?;
        Ok((group_id, runs))
    }

    async fn request_confirmed_run(
        &self,
        trigger: &str,
        request: AgentRunRequest,
    ) -> RunResult<PendingRun> {
        let plan = self
            .graph
            .compile_plan(&request.graph, &request.version_id)?;
        let plan_json = serde_json::to_string(&plan)?;
        let run = self
            .store
            .create_run(NewRun {
                workspace_id: &request.workspace_id,
                version_id: &request.version_id,
                group_id: request.group_id.as_deref(),
                label: &request.label,
                trigger,
                plan_json: Some(&plan_json),
                estimate_json: None,
                status: RunStatus::Estimating.as_str(),
            })
            .await?;
        let steps = self.ensure_run_steps(&run.id, &plan).await?;
        let estimate = match self
            .estimate_created_run(&request.workspace_id, &run.id, &plan.steps, &steps)
            .await
        {
            Ok(estimate) => estimate,
            Err(err) => {
                let error_json = serde_json::to_string(&json!({ "error": err.to_string() }))?;
                self.store
                    .update_run_status(&run.id, RunStatus::Failed.as_str(), Some(&error_json))
                    .await?;
                self.emit(
                    &request.workspace_id,
                    &run.id,
                    "run.failed",
                    json!({ "error": err.to_string(), "phase": "estimate" }),
                )
                .await?;
                return Err(err);
            }
        };
        let estimate_json = serde_json::to_string(&estimate)?;
        let run = self
            .store
            .update_run_estimate_and_status(
                &run.id,
                Some(&estimate_json),
                RunStatus::WaitingConfirmation.as_str(),
            )
            .await?;

        self.emit(
            &request.workspace_id,
            &run.id,
            "run.estimate.updated",
            json!({ "estimate": estimate }),
        )
        .await?;
        self.emit(
            &request.workspace_id,
            &run.id,
            "run.requested",
            json!({
                "trigger": trigger,
                "estimate": estimate,
                "requires_confirmation": true
            }),
        )
        .await?;

        let steps = self.store.run_steps(&run.id).await?;
        let ledger = self.store.cost_ledger_for_run(&run.id).await?;
        Ok(PendingRun {
            run,
            steps,
            estimate,
            ledger,
        })
    }

    async fn estimate_created_run(
        &self,
        workspace_id: &str,
        run_id: &str,
        plan_steps: &[helixflow_graph::ExecutionStep],
        records: &[RunStepRecord],
    ) -> RunResult<CostSummary> {
        let mut total = CostSummary::zero();
        for (step, record) in plan_steps.iter().zip(records.iter()) {
            let (Some(provider), Some(capability)) = (&step.provider, &step.capability) else {
                continue;
            };
            let estimate = self
                .provider
                .estimate(ProviderRequest {
                    provider: provider.clone(),
                    capability: capability.clone(),
                    node_id: step.node_id.clone(),
                    run_id: run_id.to_owned(),
                    inputs: BTreeMap::<String, ArtifactRef>::new(),
                    params: step.params.clone(),
                })
                .await?;
            let estimate_json = serde_json::to_string(&estimate)?;
            self.store
                .update_run_step_cost_estimate(&record.id, Some(&estimate_json))
                .await?;
            self.store
                .create_cost_ledger(NewCostLedger {
                    workspace_id,
                    run_id: Some(run_id),
                    run_step_id: Some(&record.id),
                    provider,
                    amount: estimate.amount,
                    currency: &estimate.currency,
                    estimated: true,
                })
                .await?;
            total.add(&CostSummary::from_estimate(&estimate))?;
        }
        Ok(total)
    }

    async fn record_actual_costs(&self, outcome: &RunOutcome) -> RunResult<()> {
        for step in &outcome.steps {
            let (Some(provider), Some(cost_json)) = (&step.provider, &step.cost_actual_json) else {
                continue;
            };
            let cost: CostEstimate = serde_json::from_str(cost_json)?;
            self.store
                .create_cost_ledger(NewCostLedger {
                    workspace_id: &outcome.run.workspace_id,
                    run_id: Some(&outcome.run.id),
                    run_step_id: Some(&step.id),
                    provider,
                    amount: cost.amount,
                    currency: &cost.currency,
                    estimated: false,
                })
                .await?;
        }
        Ok(())
    }

    async fn revert_claimed_runs(&self, run_ids: &[String]) -> RunResult<()> {
        for run_id in run_ids {
            if self
                .store
                .update_run_status_if_current(
                    run_id,
                    RunStatus::Running.as_str(),
                    RunStatus::WaitingConfirmation.as_str(),
                    None,
                )
                .await?
                .is_none()
            {
                let current = self.store.run(run_id).await?;
                return Err(RunError::InvalidRunStatus {
                    run_id: current.id,
                    expected: RunStatus::Running.as_str(),
                    actual: current.status,
                });
            }
        }
        Ok(())
    }
}

impl CostSummary {
    fn zero() -> Self {
        Self {
            amount: 0.0,
            currency: "USD".to_owned(),
            estimated: true,
        }
    }

    fn from_estimate(estimate: &CostEstimate) -> Self {
        Self {
            amount: estimate.amount,
            currency: estimate.currency.clone(),
            estimated: estimate.estimated,
        }
    }

    fn add(&mut self, cost: &Self) -> RunResult<()> {
        if self.amount == 0.0 {
            self.currency.clone_from(&cost.currency);
        }
        if self.currency != cost.currency {
            return Err(RunError::MixedCostCurrency {
                expected: self.currency.clone(),
                actual: cost.currency.clone(),
            });
        }

        self.amount += cost.amount;
        self.estimated &= cost.estimated;
        Ok(())
    }
}

fn validate_sweep_plan(plan: &SweepPlan) -> RunResult<()> {
    if plan.variants.len() < 2 {
        return Err(RunError::InvalidSweepPlan(
            "sweep must contain at least two variants".to_owned(),
        ));
    }
    if plan
        .variants
        .iter()
        .any(|variant| variant.label.trim().is_empty())
    {
        return Err(RunError::InvalidSweepPlan(
            "sweep variants must have labels".to_owned(),
        ));
    }

    Ok(())
}
