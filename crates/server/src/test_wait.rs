use std::time::{Duration, Instant};

use helixflow_store::{RunRecord, Store};

const ASYNC_TEST_TIMEOUT: Duration = Duration::from_secs(15);
const ASYNC_TEST_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) async fn wait_for_run_status(store: &Store, run_id: &str, expected: &str) -> RunRecord {
    let deadline = Instant::now() + ASYNC_TEST_TIMEOUT;
    loop {
        let run = store.run(run_id).await.expect("run");
        if run.status == expected {
            return run;
        }
        assert!(
            Instant::now() < deadline,
            "run status stayed {}",
            run.status
        );
        tokio::time::sleep(ASYNC_TEST_POLL_INTERVAL).await;
    }
}

pub(crate) async fn wait_for_actual_cost(store: &Store, run_id: &str) {
    let deadline = Instant::now() + ASYNC_TEST_TIMEOUT;
    loop {
        let ledger = store
            .cost_ledger_for_run(run_id)
            .await
            .expect("cost ledger");
        if ledger.iter().any(|entry| !entry.estimated) {
            return;
        }
        assert!(Instant::now() < deadline, "actual cost was not recorded");
        tokio::time::sleep(ASYNC_TEST_POLL_INTERVAL).await;
    }
}
