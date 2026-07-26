use helixflow_gateway::Provider;
use helixflow_store::NewRun;
use serde_json::json;

use super::run_policy::run_requires_confirmation;
use super::{ManualRunRequest, RunInterrupt, RunOutcome, RunResult, RunService, RunStatus};

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub async fn start_manual_run(&self, request: ManualRunRequest) -> RunResult<RunOutcome> {
        let mut plan =
            self.graph
                .compile_plan(&request.graph, &request.version_id, &request.provider)?;
        crate::resolved::attach_resolved_bindings(&mut plan, &request.provider, None)?;
        let plan = plan;
        if plan.steps.is_empty() {
            return Err(super::RunError::NoExecutableSteps);
        }
        self.ensure_workspace_not_busy(&request.workspace_id, request.group_id.as_deref(), None)
            .await?;
        let plan_json = serde_json::to_string(&plan)?;
        let run = self
            .store
            .create_run(NewRun {
                workspace_id: &request.workspace_id,
                version_id: &request.version_id,
                group_id: request.group_id.as_deref(),
                label: &request.label,
                trigger: "manual",
                plan_json: Some(&plan_json),
                estimate_json: None,
                status: RunStatus::Queued.as_str(),
            })
            .await?;
        if request.force_rerun {
            self.store.set_run_force_rerun(&run.id, true).await?;
        }
        let steps = self.ensure_run_steps(&run.id, &plan).await?;
        let interrupt = RunInterrupt::default();
        self.interrupts
            .lock()
            .await
            .insert(run.id.clone(), interrupt.clone());

        // Manual runs pass the same cost gate as agent runs: estimate first,
        // and anything over budget or of unknown cost waits for explicit
        // confirmation instead of executing immediately (HF-004).
        let estimate = match self
            .estimate_created_run(
                &request.workspace_id,
                &run.id,
                &plan.steps,
                &steps,
                &interrupt,
            )
            .await
        {
            Ok(estimate) => estimate,
            Err(err) => {
                self.interrupts.lock().await.remove(&run.id);
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
        if run_requires_confirmation(&estimate)? {
            self.interrupts.lock().await.remove(&run.id);
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
                "run.requested",
                json!({
                    "trigger": "manual",
                    "estimate": estimate,
                    "requires_confirmation": true
                }),
            )
            .await?;
            let steps = self.store.run_steps(&run.id).await?;
            return Ok(RunOutcome {
                run,
                steps,
                artifacts: Vec::new(),
            });
        }
        let run = self
            .store
            .update_run_estimate_and_status(
                &run.id,
                Some(&estimate_json),
                RunStatus::Queued.as_str(),
            )
            .await?;
        let steps = self.store.run_steps(&run.id).await?;

        let outcome = RunOutcome {
            run: run.clone(),
            steps,
            artifacts: Vec::new(),
        };
        let runner = self.clone();
        let workspace_id = request.workspace_id;
        let force_rerun = request.force_rerun;
        tokio::spawn(async move {
            let run_id = run.id.clone();
            if let Err(err) = runner
                .execute_created_run(&run, &workspace_id, &plan, interrupt, force_rerun)
                .await
            {
                eprintln!("background run `{run_id}` failed: {err}");
            }
            runner.interrupts.lock().await.remove(&run_id);
        });

        Ok(outcome)
    }

    pub async fn has_interrupt(&self, run_id: &str) -> bool {
        self.interrupts.lock().await.contains_key(run_id)
    }
}
