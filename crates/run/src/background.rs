use helixflow_gateway::Provider;
use helixflow_store::NewRun;

use super::{ManualRunRequest, RunInterrupt, RunOutcome, RunResult, RunService, RunStatus};

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub async fn start_manual_run(&self, request: ManualRunRequest) -> RunResult<RunOutcome> {
        let plan =
            self.graph
                .compile_plan(&request.graph, &request.version_id, &request.provider)?;
        if plan.steps.is_empty() {
            return Err(super::RunError::NoExecutableSteps);
        }
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
        let steps = self.ensure_run_steps(&run.id, &plan).await?;
        let interrupt = RunInterrupt::default();
        self.interrupts
            .lock()
            .await
            .insert(run.id.clone(), interrupt.clone());

        let outcome = RunOutcome {
            run: run.clone(),
            steps,
            artifacts: Vec::new(),
        };
        let runner = self.clone();
        let workspace_id = request.workspace_id;
        tokio::spawn(async move {
            let run_id = run.id.clone();
            if let Err(err) = runner
                .execute_created_run(&run, &workspace_id, &plan, interrupt)
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
