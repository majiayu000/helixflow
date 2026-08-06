use std::time::Duration;

/// Background tests share the host with hundreds of async cases. Keep waits
/// bounded, but leave enough scheduling headroom for loaded CI and local runs.
pub(crate) const ASYNC_TEST_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const ASYNC_TEST_POLL_INTERVAL: Duration = Duration::from_millis(10);
