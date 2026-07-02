use std::time::{Duration, Instant};

use helixflow_store::{RunRecord, Store};

pub(crate) async fn wait_for_run_status(store: &Store, run_id: &str, expected: &str) -> RunRecord {
    let deadline = Instant::now() + Duration::from_secs(2);
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
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub(crate) async fn wait_for_actual_cost(store: &Store, run_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let ledger = store
            .cost_ledger_for_run(run_id)
            .await
            .expect("cost ledger");
        if ledger.iter().any(|entry| !entry.estimated) {
            return;
        }
        assert!(Instant::now() < deadline, "actual cost was not recorded");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
