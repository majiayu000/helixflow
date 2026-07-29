use std::collections::BTreeMap;

use helixflow_gateway::{ArtifactRef, CostEstimate, Provider, ProviderRequest};
use helixflow_graph::WorkflowGraph;
use helixflow_store::{
    CompleteRunFixChildRecord, NewCostLedger, NewRun, PrepareRunFixChildRecord,
    RunFixAttemptRecord, RunRecord,
};
use sha2::{Digest, Sha256};

use super::{
    CostSummary, PendingRun, RunError, RunOutcome, RunResult, RunService, RunStatus,
    agent_fix_policy, run_requires_confirmation,
};

pub fn provider_catalog_fingerprint<P: Provider>(provider: &P, provider_id: &str) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(provider.catalog_revision(provider_id).as_bytes())
    )
}

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub(crate) async fn create_run_with_optional_fix_chain(
        &self,
        input: NewRun<'_>,
    ) -> RunResult<RunRecord> {
        let policy = agent_fix_policy()?;
        if policy.enabled && input.trigger == "agent" {
            return Ok(self
                .store
                .create_agent_run_with_repair_chain(input)
                .await?
                .0);
        }
        Ok(self.store.create_run(input).await?)
    }

    pub(crate) async fn prepare_recommended_sweep_fix_chain(
        &self,
        group_id: &str,
        recommended_run_id: &str,
    ) -> RunResult<()> {
        if agent_fix_policy()?.enabled {
            self.store
                .create_recommended_sweep_repair_chain(recommended_run_id, group_id)
                .await?;
        }
        Ok(())
    }

    pub async fn prepare_run_fix_child(
        &self,
        attempt_id: &str,
        graph: WorkflowGraph,
    ) -> RunResult<PendingRun> {
        let attempt = self
            .store
            .run_fix_attempt(attempt_id)
            .await?
            .ok_or_else(|| RunError::RunFixGuard {
                code: "FIX_RECOVERY_INTERRUPTED",
            })?;
        self.validate_run_fix_guard(&attempt, true).await?;
        let target_version_id =
            attempt
                .target_version_id
                .as_deref()
                .ok_or_else(|| RunError::RunFixGuard {
                    code: "FIX_RECOVERY_INTERRUPTED",
                })?;
        let mut plan =
            self.graph
                .compile_plan(&graph, target_version_id, &attempt.effective_provider_id)?;
        let semantics =
            crate::resolved::version_semantics(&graph, &self.store, target_version_id).await?;
        crate::resolved::attach_resolved_bindings(
            &mut plan,
            &attempt.effective_provider_id,
            semantics.as_ref(),
        )?;
        if plan.steps.is_empty() {
            return Err(RunError::NoExecutableSteps);
        }
        let plan_json = serde_json::to_string(&plan)?;
        let workspace = self.store.workspace(&attempt.workspace_id).await?;
        let scope = self
            .provider
            .recovery_scope_fingerprint(&attempt.effective_provider_id);
        let catalog = provider_catalog_fingerprint(&self.provider, &attempt.effective_provider_id);
        let (_, child) = self
            .store
            .prepare_run_fix_child(PrepareRunFixChildRecord {
                attempt_id,
                label: "Agent 修复后运行",
                plan_json: &plan_json,
                actual_runtime_provider_id: workspace.runtime_provider_id.as_deref(),
                actual_effective_provider_id: &attempt.effective_provider_id,
                actual_recovery_scope_fingerprint: &scope,
                actual_provider_catalog_fingerprint: &catalog,
            })
            .await?;
        let steps = self.ensure_run_steps(&child.id, &plan).await?;
        let estimate = self
            .estimate_run_fix_child(&attempt, &child.id, &plan.steps, &steps)
            .await?;
        let estimate_json = serde_json::to_string(&estimate)?;
        let requires_confirmation = run_requires_confirmation(&estimate)?;
        let (_, child) = self
            .store
            .complete_run_fix_child(CompleteRunFixChildRecord {
                attempt_id,
                estimate_json: &estimate_json,
                requires_confirmation,
            })
            .await?;
        self.emit(
            &child.workspace_id,
            &child.id,
            "run.estimate.updated",
            serde_json::json!({ "estimate": estimate }),
        )
        .await?;
        let child_id = child.id.clone();
        let pending = PendingRun {
            run: child,
            steps: self.store.run_steps(&child_id).await?,
            estimate,
            ledger: self.store.cost_ledger_for_run(&child_id).await?,
        };
        self.announce_run_requested(&pending, requires_confirmation)
            .await?;
        if !requires_confirmation {
            self.start_confirmed_run(&pending.run.id).await?;
        }
        Ok(pending)
    }

    pub(crate) async fn validate_run_fix_child_guard(&self, run_id: &str) -> RunResult<()> {
        let Some(attempt) = self.store.run_fix_attempt_for_child(run_id).await? else {
            return Ok(());
        };
        self.validate_run_fix_guard(&attempt, true).await
    }

    pub(crate) fn ignore_quiescent_fix_children(&self) -> RunResult<bool> {
        Ok(!agent_fix_policy()?.enabled)
    }

    pub(crate) async fn is_quiescent_fix_child(&self, run_id: &str) -> RunResult<bool> {
        let Some(attempt) = self.store.run_fix_attempt_for_child(run_id).await? else {
            return Ok(false);
        };
        if !matches!(attempt.state.as_str(), "child_preparing" | "child_ready") {
            return Ok(false);
        }
        Ok(!self
            .store
            .provider_tasks_for_run(run_id)
            .await?
            .iter()
            .any(|task| {
                matches!(
                    task.state.as_str(),
                    "dispatching" | "active" | "result_ready"
                )
            }))
    }

    pub(crate) async fn cancel_waiting_fix_child(
        &self,
        run_id: &str,
    ) -> RunResult<Option<RunOutcome>> {
        let Some(run) = self.store.cancel_run_fix_child(run_id).await? else {
            return Ok(None);
        };
        self.emit(
            &run.workspace_id,
            &run.id,
            "run.interrupted",
            serde_json::json!({ "reason_code": "FIX_USER_CANCELLED" }),
        )
        .await?;
        Ok(Some(self.outcome(run_id).await?))
    }

    pub(crate) async fn cancel_active_fix_child(&self, run_id: &str) -> RunResult<bool> {
        let Some(run) = self.store.cancel_run_fix_child(run_id).await? else {
            return Ok(false);
        };
        if run.status == RunStatus::Running.as_str() {
            self.request_and_settle_terminal(
                &run.workspace_id,
                run_id,
                RunStatus::Interrupted.as_str(),
                None,
            )
            .await?;
        } else {
            self.emit(
                &run.workspace_id,
                &run.id,
                "run.interrupted",
                serde_json::json!({ "reason_code": "FIX_USER_CANCELLED" }),
            )
            .await?;
        }
        Ok(true)
    }

    async fn validate_run_fix_guard(
        &self,
        attempt: &RunFixAttemptRecord,
        require_target: bool,
    ) -> RunResult<()> {
        let policy = agent_fix_policy()?;
        if !policy.enabled {
            return Err(RunError::RunFixGuard {
                code: "FIX_DISABLED",
            });
        }
        let workspace = self.store.workspace(&attempt.workspace_id).await?;
        if workspace.runtime_provider_id != attempt.expected_runtime_provider_id {
            return Err(RunError::RunFixGuard {
                code: "FIX_PROVIDER_CHANGED",
            });
        }
        if require_target && workspace.cur_version_id != attempt.target_version_id {
            return Err(RunError::RunFixGuard {
                code: "FIX_TARGET_DESELECTED",
            });
        }
        let scope = self
            .provider
            .recovery_scope_fingerprint(&attempt.effective_provider_id);
        if scope != attempt.expected_recovery_scope_fingerprint {
            return Err(RunError::RunFixGuard {
                code: "FIX_PROVIDER_SCOPE_CHANGED",
            });
        }
        let catalog = provider_catalog_fingerprint(&self.provider, &attempt.effective_provider_id);
        if catalog != attempt.provider_catalog_fingerprint {
            return Err(RunError::RunFixGuard {
                code: "FIX_PROVIDER_CHANGED",
            });
        }
        Ok(())
    }

    async fn estimate_run_fix_child(
        &self,
        attempt: &RunFixAttemptRecord,
        run_id: &str,
        plan_steps: &[helixflow_graph::ExecutionStep],
        records: &[helixflow_store::RunStepRecord],
    ) -> RunResult<CostSummary> {
        let mut total = CostSummary {
            amount: 0.0,
            currency: "USD".to_owned(),
            estimated: true,
            unknown: false,
        };
        for (step, record) in plan_steps.iter().zip(records.iter()) {
            let (Some(provider), Some(capability)) = (&step.provider, &step.capability) else {
                continue;
            };
            self.validate_run_fix_guard(attempt, true).await?;
            let estimate = if let Some(existing) = record.cost_estimate_json.as_deref() {
                serde_json::from_str::<CostEstimate>(existing)?
            } else {
                let request = ProviderRequest {
                    provider: provider.clone(),
                    capability: capability.clone(),
                    node_id: step.node_id.clone(),
                    run_id: run_id.to_owned(),
                    inputs: BTreeMap::<String, ArtifactRef>::new(),
                    input_texts: BTreeMap::new(),
                    params: step.params.clone(),
                    resolved_model_id: step
                        .resolved
                        .as_ref()
                        .map(|resolved| resolved.resolved_model_id.clone()),
                    operation_id: step
                        .resolved
                        .as_ref()
                        .map(|resolved| resolved.operation_id.clone()),
                };
                let estimate = self.provider.estimate(request).await?;
                self.validate_run_fix_guard(attempt, true).await?;
                let estimate_json = serde_json::to_string(&estimate)?;
                self.store
                    .update_run_step_cost_estimate(&record.id, Some(&estimate_json))
                    .await?;
                estimate
            };
            self.store
                .create_cost_ledger_once(
                    &format!("estimate:{}", record.id),
                    NewCostLedger {
                        workspace_id: &attempt.workspace_id,
                        run_id: Some(run_id),
                        run_step_id: Some(&record.id),
                        provider,
                        amount: estimate.amount,
                        currency: &estimate.currency,
                        estimated: true,
                    },
                )
                .await?;
            add_estimate(&mut total, &estimate)?;
        }
        Ok(total)
    }
}

fn add_estimate(total: &mut CostSummary, estimate: &CostEstimate) -> RunResult<()> {
    if total.currency != estimate.currency {
        return Err(RunError::MixedCostCurrency {
            expected: total.currency.clone(),
            actual: estimate.currency.clone(),
        });
    }
    total.amount += estimate.amount;
    total.estimated &= estimate.estimated;
    total.unknown |= estimate.unknown
        || !estimate.amount.is_finite()
        || estimate.amount < 0.0
        || !estimate.estimated;
    Ok(())
}
