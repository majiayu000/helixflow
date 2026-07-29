use std::time::Duration;

use helixflow_gateway::{
    Provider, ProviderDispatchFailure, ProviderDispatchFailureKind, ProviderDispatchResult,
    ProviderError, ProviderRequest,
};

use super::{RunInterrupt, RunResult, RunService};

const DISPATCH_LEASE_SECONDS: i64 = 60;
const DISPATCH_RENEW_INTERVAL: Duration = Duration::from_secs(20);

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub(crate) async fn dispatch_with_durable_owner(
        &self,
        task_id: &str,
        owner_id: &str,
        request: ProviderRequest,
        interrupt: &RunInterrupt,
    ) -> RunResult<ProviderDispatchResult> {
        let capabilities = self.provider.recovery_capabilities(&request.provider);
        let dispatch = self.provider.dispatch(request);
        tokio::pin!(dispatch);
        let mut interrupted = false;
        loop {
            tokio::select! {
                result = &mut dispatch => return Ok(result),
                _ = interrupt.cancelled(), if !interrupted => {
                    if !capabilities.resume && !capabilities.cancel {
                        return Ok(Err(ProviderDispatchFailure {
                            kind: ProviderDispatchFailureKind::NotSubmitted,
                            error: ProviderError::RequestFailed(
                                "local provider execution was interrupted".to_owned(),
                            ),
                        }));
                    }
                    interrupted = true;
                }
                _ = tokio::time::sleep(DISPATCH_RENEW_INTERVAL) => {
                    if !self
                        .store
                        .renew_dispatch_owner(task_id, owner_id, DISPATCH_LEASE_SECONDS)
                        .await?
                    {
                        return Ok(Err(ProviderDispatchFailure {
                            kind: ProviderDispatchFailureKind::OutcomeUnknown,
                            error: ProviderError::RequestFailed(
                                "provider dispatch exceeded its durable deadline".to_owned(),
                            ),
                        }));
                    }
                }
            }
        }
    }
}
