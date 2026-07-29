use helixflow_gateway::Provider;
use helixflow_store::{NewRun, RunRecord};

use super::{RunResult, RunService, agent_fix_policy};

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
}
