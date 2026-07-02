use super::{RunRecord, Store, StoreResult};

impl Store {
    pub async fn interrupt_stale_active_runs(&self) -> StoreResult<Vec<RunRecord>> {
        let mut tx = self.pool().begin().await?;
        let stale_runs = sqlx::query_as::<_, RunRecord>(
            r#"
            SELECT id, workspace_id, version_id, group_id, label, trigger, plan_json,
                   estimate_json, status, error_json, started_at, ended_at, created_at
            FROM runs
            WHERE status IN ('queued', 'estimating', 'running')
            ORDER BY created_at, id
            "#,
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut interrupted = Vec::with_capacity(stale_runs.len());
        for run in stale_runs {
            sqlx::query(
                r#"
                UPDATE runs
                SET status = 'interrupted',
                    error_json = NULL,
                    ended_at = current_timestamp
                WHERE id = ?
                "#,
            )
            .bind(&run.id)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                UPDATE run_steps
                SET state = 'skipped',
                    ended_at = current_timestamp
                WHERE run_id = ? AND state IN ('queued', 'running')
                "#,
            )
            .bind(&run.id)
            .execute(&mut *tx)
            .await?;

            interrupted.push(
                sqlx::query_as::<_, RunRecord>(
                    r#"
                    SELECT id, workspace_id, version_id, group_id, label, trigger, plan_json,
                           estimate_json, status, error_json, started_at, ended_at, created_at
                    FROM runs
                    WHERE id = ?
                    "#,
                )
                .bind(&run.id)
                .fetch_one(&mut *tx)
                .await?,
            );
        }

        tx.commit().await?;
        Ok(interrupted)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{NewRun, NewRunStep, NewVersion, VersionSource};

    async fn open_temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let db_path = dir.path().join("helixflow.sqlite");
        let database_url = format!("sqlite://{}", db_path.display());
        let store = Store::open(&database_url).await.expect("open store");
        (store, dir)
    }

    #[tokio::test]
    async fn interrupt_stale_active_runs_skips_active_runs_and_steps_only() {
        let (store, _dir) = open_temp_store().await;
        let workspace = store
            .create_workspace("Stale runs")
            .await
            .expect("create workspace");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Graph",
                source: VersionSource::Manual,
                graph_path: "graphs/current.json",
                graph_hash: "sha256:graph",
                parent_id: None,
            })
            .await
            .expect("create version");

        let queued = run_with_step(&store, &workspace.id, &version.id, "queued", "queued").await;
        let estimating =
            run_with_step(&store, &workspace.id, &version.id, "estimating", "queued").await;
        let running = run_with_step(&store, &workspace.id, &version.id, "running", "running").await;
        let waiting = run_with_step(
            &store,
            &workspace.id,
            &version.id,
            "waiting_confirmation",
            "queued",
        )
        .await;
        let succeeded =
            run_with_step(&store, &workspace.id, &version.id, "succeeded", "succeeded").await;

        let interrupted = store
            .interrupt_stale_active_runs()
            .await
            .expect("interrupt stale runs");
        let interrupted_ids = interrupted
            .iter()
            .map(|run| run.id.as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(
            interrupted_ids,
            BTreeSet::from([
                queued.id.as_str(),
                estimating.id.as_str(),
                running.id.as_str()
            ])
        );
        assert_active_run_interrupted(&store, &queued.id).await;
        assert_active_run_interrupted(&store, &estimating.id).await;
        assert_active_run_interrupted(&store, &running.id).await;
        assert_run_and_step_state(&store, &waiting.id, "waiting_confirmation", "queued").await;
        assert_run_and_step_state(&store, &succeeded.id, "succeeded", "succeeded").await;
    }

    async fn run_with_step(
        store: &Store,
        workspace_id: &str,
        version_id: &str,
        status: &str,
        step_state: &str,
    ) -> RunRecord {
        let run = store
            .create_run(NewRun {
                workspace_id,
                version_id,
                group_id: None,
                label: status,
                trigger: "manual",
                plan_json: None,
                estimate_json: None,
                status,
            })
            .await
            .expect("create run");
        store
            .create_run_step(NewRunStep {
                run_id: &run.id,
                node_id: "node",
                node_type: "llm.prompt_writer",
                provider: Some("atlas"),
                state: step_state,
            })
            .await
            .expect("create run step");
        run
    }

    async fn assert_active_run_interrupted(store: &Store, run_id: &str) {
        let run = store.run(run_id).await.expect("run");
        let steps = store.run_steps(run_id).await.expect("steps");

        assert_eq!(run.status, "interrupted");
        assert!(run.ended_at.is_some());
        assert_eq!(steps[0].state, "skipped");
        assert!(steps[0].ended_at.is_some());
    }

    async fn assert_run_and_step_state(
        store: &Store,
        run_id: &str,
        expected_run: &str,
        expected_step: &str,
    ) {
        let run = store.run(run_id).await.expect("run");
        let steps = store.run_steps(run_id).await.expect("steps");

        assert_eq!(run.status, expected_run);
        assert_eq!(steps[0].state, expected_step);
    }
}
