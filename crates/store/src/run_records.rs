use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::{Store, StoreResult, new_id};

#[derive(Debug, Clone)]
pub struct NewRun<'a> {
    pub workspace_id: &'a str,
    pub version_id: &'a str,
    pub group_id: Option<&'a str>,
    pub label: &'a str,
    pub trigger: &'a str,
    pub plan_json: Option<&'a str>,
    pub estimate_json: Option<&'a str>,
    pub status: &'a str,
}

#[derive(Debug, Clone)]
pub struct NewRunStep<'a> {
    pub run_id: &'a str,
    pub node_id: &'a str,
    pub node_type: &'a str,
    pub provider: Option<&'a str>,
    pub state: &'a str,
}

#[derive(Debug, Clone)]
pub struct NewArtifact<'a> {
    pub workspace_id: &'a str,
    pub run_id: Option<&'a str>,
    pub run_step_id: Option<&'a str>,
    pub node_id: Option<&'a str>,
    pub kind: &'a str,
    pub storage_uri: &'a str,
    pub sha256: Option<&'a str>,
    pub mime: Option<&'a str>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub selected: bool,
    pub meta_json: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct NewCostLedger<'a> {
    pub workspace_id: &'a str,
    pub run_id: Option<&'a str>,
    pub run_step_id: Option<&'a str>,
    pub provider: &'a str,
    pub amount: f64,
    pub currency: &'a str,
    pub estimated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct RunRecord {
    pub id: String,
    pub workspace_id: String,
    pub version_id: String,
    pub group_id: Option<String>,
    pub label: String,
    pub trigger: String,
    pub plan_json: Option<String>,
    pub estimate_json: Option<String>,
    pub status: String,
    pub error_json: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunStepRecord {
    pub id: String,
    pub run_id: String,
    pub node_id: String,
    pub node_type: String,
    pub provider: Option<String>,
    pub state: String,
    pub progress: Option<f64>,
    pub cost_estimate_json: Option<String>,
    pub cost_actual_json: Option<String>,
    pub error_json: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct RunEventRecord {
    pub id: String,
    pub run_id: String,
    pub seq: i64,
    pub ev: String,
    pub data_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactRecord {
    pub id: String,
    pub workspace_id: String,
    pub run_id: Option<String>,
    pub run_step_id: Option<String>,
    pub node_id: Option<String>,
    pub kind: String,
    pub storage_uri: String,
    pub sha256: Option<String>,
    pub mime: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub selected: bool,
    pub meta_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostLedgerRecord {
    pub id: String,
    pub workspace_id: String,
    pub run_id: Option<String>,
    pub run_step_id: Option<String>,
    pub provider: String,
    pub amount: f64,
    pub currency: String,
    pub estimated: bool,
    pub created_at: String,
}

impl Store {
    pub async fn create_run(&self, input: NewRun<'_>) -> StoreResult<RunRecord> {
        let id = new_id("run");
        let actual_workspace_id: Option<String> = sqlx::query_scalar(
            r#"
            SELECT workspace_id
            FROM versions
            WHERE id = ?
            "#,
        )
        .bind(input.version_id)
        .fetch_optional(self.pool())
        .await?;
        if actual_workspace_id.as_deref() != Some(input.workspace_id) {
            return Err(super::StoreError::RunVersionMismatch {
                workspace_id: input.workspace_id.to_owned(),
                version_id: input.version_id.to_owned(),
                actual_workspace_id,
            });
        }

        sqlx::query(
            r#"
            INSERT INTO runs (
                id, workspace_id, version_id, group_id, label, trigger, plan_json,
                estimate_json, status, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.version_id)
        .bind(input.group_id)
        .bind(input.label)
        .bind(input.trigger)
        .bind(input.plan_json)
        .bind(input.estimate_json)
        .bind(input.status)
        .execute(self.pool())
        .await?;

        self.run(&id).await
    }

    pub async fn run(&self, run_id: &str) -> StoreResult<RunRecord> {
        Ok(sqlx::query_as::<_, RunRecord>(
            r#"
            SELECT id, workspace_id, version_id, group_id, label, trigger, plan_json,
                   estimate_json, status, error_json, started_at, ended_at, created_at
            FROM runs
            WHERE id = ?
            "#,
        )
        .bind(run_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn update_run_status(
        &self,
        run_id: &str,
        status: &str,
        error_json: Option<&str>,
    ) -> StoreResult<RunRecord> {
        sqlx::query(
            r#"
            UPDATE runs
            SET status = ?,
                error_json = ?,
                started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN current_timestamp ELSE started_at END,
                ended_at = CASE WHEN ? IN ('succeeded', 'failed', 'interrupted') THEN current_timestamp ELSE ended_at END
            WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(error_json)
        .bind(status)
        .bind(status)
        .bind(run_id)
        .execute(self.pool())
        .await?;

        self.run(run_id).await
    }

    pub async fn update_run_status_if_current(
        &self,
        run_id: &str,
        expected_status: &str,
        next_status: &str,
        error_json: Option<&str>,
    ) -> StoreResult<Option<RunRecord>> {
        let result = sqlx::query(
            r#"
            UPDATE runs
            SET status = ?,
                error_json = ?,
                started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN current_timestamp ELSE started_at END,
                ended_at = CASE WHEN ? IN ('succeeded', 'failed', 'interrupted') THEN current_timestamp ELSE ended_at END
            WHERE id = ? AND status = ?
            "#,
        )
        .bind(next_status)
        .bind(error_json)
        .bind(next_status)
        .bind(next_status)
        .bind(run_id)
        .bind(expected_status)
        .execute(self.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.run(run_id).await.map(Some)
    }

    pub async fn update_run_estimate_and_status(
        &self,
        run_id: &str,
        estimate_json: Option<&str>,
        status: &str,
    ) -> StoreResult<RunRecord> {
        sqlx::query(
            r#"
            UPDATE runs
            SET estimate_json = ?,
                status = ?
            WHERE id = ?
            "#,
        )
        .bind(estimate_json)
        .bind(status)
        .bind(run_id)
        .execute(self.pool())
        .await?;

        self.run(run_id).await
    }

    pub async fn create_run_step(&self, input: NewRunStep<'_>) -> StoreResult<RunStepRecord> {
        let id = new_id("step");
        sqlx::query(
            r#"
            INSERT INTO run_steps (id, run_id, node_id, node_type, provider, state)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(input.run_id)
        .bind(input.node_id)
        .bind(input.node_type)
        .bind(input.provider)
        .bind(input.state)
        .execute(self.pool())
        .await?;

        self.run_step(&id).await
    }

    pub async fn run_step(&self, step_id: &str) -> StoreResult<RunStepRecord> {
        let row = sqlx::query(
            r#"
            SELECT id, run_id, node_id, node_type, provider, state, progress,
                   cost_estimate_json, cost_actual_json, error_json, started_at, ended_at
            FROM run_steps
            WHERE id = ?
            "#,
        )
        .bind(step_id)
        .fetch_one(self.pool())
        .await?;

        run_step_from_row(row)
    }

    pub async fn update_run_step_state(
        &self,
        step_id: &str,
        state: &str,
        progress: Option<f64>,
        cost_actual_json: Option<&str>,
        error_json: Option<&str>,
    ) -> StoreResult<RunStepRecord> {
        sqlx::query(
            r#"
            UPDATE run_steps
            SET state = ?,
                progress = ?,
                cost_actual_json = ?,
                error_json = ?,
                started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN current_timestamp ELSE started_at END,
                ended_at = CASE WHEN ? IN ('succeeded', 'failed', 'skipped') THEN current_timestamp ELSE ended_at END
            WHERE id = ?
            "#,
        )
        .bind(state)
        .bind(progress)
        .bind(cost_actual_json)
        .bind(error_json)
        .bind(state)
        .bind(state)
        .bind(step_id)
        .execute(self.pool())
        .await?;

        self.run_step(step_id).await
    }

    pub async fn run_steps(&self, run_id: &str) -> StoreResult<Vec<RunStepRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, run_id, node_id, node_type, provider, state, progress,
                   cost_estimate_json, cost_actual_json, error_json, started_at, ended_at
            FROM run_steps
            WHERE run_id = ?
            ORDER BY rowid
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;

        rows.into_iter().map(run_step_from_row).collect()
    }

    pub async fn update_run_step_cost_estimate(
        &self,
        step_id: &str,
        cost_estimate_json: Option<&str>,
    ) -> StoreResult<RunStepRecord> {
        sqlx::query(
            r#"
            UPDATE run_steps
            SET cost_estimate_json = ?
            WHERE id = ?
            "#,
        )
        .bind(cost_estimate_json)
        .bind(step_id)
        .execute(self.pool())
        .await?;

        self.run_step(step_id).await
    }

    pub async fn append_run_event(
        &self,
        run_id: &str,
        ev: &str,
        data_json: &str,
    ) -> StoreResult<RunEventRecord> {
        let id = new_id("evt");
        let mut tx = self.pool().begin().await?;
        let seq: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(MAX(seq), 0) + 1
            FROM run_events
            WHERE run_id = ?
            "#,
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO run_events (id, run_id, seq, ev, data_json, created_at)
            VALUES (?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(run_id)
        .bind(seq)
        .bind(ev)
        .bind(data_json)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        self.run_event(&id).await
    }

    pub async fn run_event(&self, event_id: &str) -> StoreResult<RunEventRecord> {
        Ok(sqlx::query_as::<_, RunEventRecord>(
            r#"
            SELECT id, run_id, seq, ev, data_json, created_at
            FROM run_events
            WHERE id = ?
            "#,
        )
        .bind(event_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn run_events(&self, run_id: &str) -> StoreResult<Vec<RunEventRecord>> {
        Ok(sqlx::query_as::<_, RunEventRecord>(
            r#"
            SELECT id, run_id, seq, ev, data_json, created_at
            FROM run_events
            WHERE run_id = ?
            ORDER BY seq
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn create_artifact(&self, input: NewArtifact<'_>) -> StoreResult<ArtifactRecord> {
        let id = new_id("art");
        sqlx::query(
            r#"
            INSERT INTO artifacts (
                id, workspace_id, run_id, run_step_id, node_id, kind, storage_uri, sha256, mime,
                width, height, duration_ms, selected, meta_json, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.run_id)
        .bind(input.run_step_id)
        .bind(input.node_id)
        .bind(input.kind)
        .bind(input.storage_uri)
        .bind(input.sha256)
        .bind(input.mime)
        .bind(input.width)
        .bind(input.height)
        .bind(input.duration_ms)
        .bind(if input.selected { 1_i64 } else { 0_i64 })
        .bind(input.meta_json)
        .execute(self.pool())
        .await?;

        self.artifact(&id).await
    }

    pub async fn update_artifact_selected(
        &self,
        artifact_id: &str,
        selected: bool,
    ) -> StoreResult<ArtifactRecord> {
        sqlx::query(
            r#"
            UPDATE artifacts
            SET selected = ?
            WHERE id = ?
            "#,
        )
        .bind(if selected { 1_i64 } else { 0_i64 })
        .bind(artifact_id)
        .execute(self.pool())
        .await?;

        self.artifact(artifact_id).await
    }

    pub async fn artifact(&self, artifact_id: &str) -> StoreResult<ArtifactRecord> {
        let row = sqlx::query(
            r#"
            SELECT id, workspace_id, run_id, run_step_id, node_id, kind, storage_uri, sha256,
                   mime, width, height, duration_ms, selected, meta_json, created_at
            FROM artifacts
            WHERE id = ?
            "#,
        )
        .bind(artifact_id)
        .fetch_one(self.pool())
        .await?;

        artifact_from_row(row)
    }

    pub async fn run_artifacts(&self, run_id: &str) -> StoreResult<Vec<ArtifactRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, workspace_id, run_id, run_step_id, node_id, kind, storage_uri, sha256,
                   mime, width, height, duration_ms, selected, meta_json, created_at
            FROM artifacts
            WHERE run_id = ?
            ORDER BY created_at, id
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;

        rows.into_iter().map(artifact_from_row).collect()
    }

    pub async fn create_cost_ledger(
        &self,
        input: NewCostLedger<'_>,
    ) -> StoreResult<CostLedgerRecord> {
        let id = new_id("cost");
        sqlx::query(
            r#"
            INSERT INTO cost_ledger (
                id, workspace_id, run_id, run_step_id, provider, amount, currency, estimated, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.run_id)
        .bind(input.run_step_id)
        .bind(input.provider)
        .bind(input.amount)
        .bind(input.currency)
        .bind(if input.estimated { 1_i64 } else { 0_i64 })
        .execute(self.pool())
        .await?;

        self.cost_ledger(&id).await
    }

    pub async fn cost_ledger(&self, cost_id: &str) -> StoreResult<CostLedgerRecord> {
        let row = sqlx::query(
            r#"
            SELECT id, workspace_id, run_id, run_step_id, provider, amount, currency, estimated, created_at
            FROM cost_ledger
            WHERE id = ?
            "#,
        )
        .bind(cost_id)
        .fetch_one(self.pool())
        .await?;

        cost_ledger_from_row(row)
    }

    pub async fn cost_ledger_for_run(&self, run_id: &str) -> StoreResult<Vec<CostLedgerRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, workspace_id, run_id, run_step_id, provider, amount, currency, estimated, created_at
            FROM cost_ledger
            WHERE run_id = ?
            ORDER BY created_at, id
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?;

        rows.into_iter().map(cost_ledger_from_row).collect()
    }
}

fn run_step_from_row(row: sqlx::sqlite::SqliteRow) -> StoreResult<RunStepRecord> {
    Ok(RunStepRecord {
        id: row.try_get("id")?,
        run_id: row.try_get("run_id")?,
        node_id: row.try_get("node_id")?,
        node_type: row.try_get("node_type")?,
        provider: row.try_get("provider")?,
        state: row.try_get("state")?,
        progress: row.try_get("progress")?,
        cost_estimate_json: row.try_get("cost_estimate_json")?,
        cost_actual_json: row.try_get("cost_actual_json")?,
        error_json: row.try_get("error_json")?,
        started_at: row.try_get("started_at")?,
        ended_at: row.try_get("ended_at")?,
    })
}

fn artifact_from_row(row: sqlx::sqlite::SqliteRow) -> StoreResult<ArtifactRecord> {
    let selected: i64 = row.try_get("selected")?;
    Ok(ArtifactRecord {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        run_id: row.try_get("run_id")?,
        run_step_id: row.try_get("run_step_id")?,
        node_id: row.try_get("node_id")?,
        kind: row.try_get("kind")?,
        storage_uri: row.try_get("storage_uri")?,
        sha256: row.try_get("sha256")?,
        mime: row.try_get("mime")?,
        width: row.try_get("width")?,
        height: row.try_get("height")?,
        duration_ms: row.try_get("duration_ms")?,
        selected: selected != 0,
        meta_json: row.try_get("meta_json")?,
        created_at: row.try_get("created_at")?,
    })
}

fn cost_ledger_from_row(row: sqlx::sqlite::SqliteRow) -> StoreResult<CostLedgerRecord> {
    let estimated: i64 = row.try_get("estimated")?;
    Ok(CostLedgerRecord {
        id: row.try_get("id")?,
        workspace_id: row.try_get("workspace_id")?,
        run_id: row.try_get("run_id")?,
        run_step_id: row.try_get("run_step_id")?,
        provider: row.try_get("provider")?,
        amount: row.try_get("amount")?,
        currency: row.try_get("currency")?,
        estimated: estimated != 0,
        created_at: row.try_get("created_at")?,
    })
}
