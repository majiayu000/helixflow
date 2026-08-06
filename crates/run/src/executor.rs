use std::collections::{BTreeMap, BTreeSet, VecDeque};

use helixflow_gateway::{ArtifactKind, ArtifactRef, Provider, ProviderRequest};
use helixflow_graph::{ExecutionPlan, ExecutionStep};
use helixflow_store::{NewArtifact, RunRecord, RunStepRecord};
use serde_json::{Value, json};
use tokio::task::JoinSet;

use super::cache::CachedArtifactLink;
use super::{
    RunError, RunEventEnvelope, RunInterrupt, RunOutcome, RunResult, RunService, RunStatus,
    RunStepState, StepOutput,
};

type OutputMap = BTreeMap<[String; 2], ArtifactRef>;

#[derive(Debug)]
enum StepExecution {
    Succeeded(Vec<StepOutput>),
    Interrupted,
}

#[derive(Debug)]
struct BuiltinExecution {
    outputs: Vec<StepOutput>,
    cache_links: Vec<CachedArtifactLink>,
}

#[derive(Debug)]
struct StepDag {
    upstream_counts: Vec<usize>,
    downstream: Vec<Vec<usize>>,
}

impl StepDag {
    fn from_plan(plan: &ExecutionPlan) -> RunResult<Self> {
        let mut by_node = BTreeMap::new();
        for (index, step) in plan.steps.iter().enumerate() {
            by_node.insert(step.node_id.as_str(), index);
        }

        let mut upstream = vec![BTreeSet::new(); plan.steps.len()];
        let mut downstream = vec![BTreeSet::new(); plan.steps.len()];
        for (index, step) in plan.steps.iter().enumerate() {
            for source in step.inputs.values() {
                let Some(&source_index) = by_node.get(source[0].as_str()) else {
                    return Err(RunError::MissingInput {
                        node_id: source[0].clone(),
                        port: source[1].clone(),
                    });
                };
                upstream[index].insert(source_index);
                downstream[source_index].insert(index);
            }
        }

        Ok(Self {
            upstream_counts: upstream.iter().map(BTreeSet::len).collect(),
            downstream: downstream
                .into_iter()
                .map(|items| items.into_iter().collect())
                .collect(),
        })
    }

    #[cfg(test)]
    fn initial_ready(&self) -> VecDeque<usize> {
        self.upstream_counts
            .iter()
            .enumerate()
            .filter_map(|(index, count)| (*count == 0).then_some(index))
            .collect()
    }
}

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub(crate) async fn execute_created_run(
        &self,
        run: &RunRecord,
        workspace_id: &str,
        plan: &ExecutionPlan,
        interrupt: RunInterrupt,
        force_rerun: bool,
    ) -> RunResult<RunOutcome> {
        let steps = self.ensure_run_steps(&run.id, plan).await?;
        let dag = StepDag::from_plan(plan)?;
        self.ensure_execution_intent(&run.id, plan, run.estimate_json.as_deref())
            .await?;

        self.store
            .update_run_status(&run.id, RunStatus::Running.as_str(), None)
            .await?;
        self.emit(
            workspace_id,
            &run.id,
            "run.started",
            json!({ "version_id": plan.version_id }),
        )
        .await?;
        if let Some(data) = crate::resolved::resolved_implementations_event(plan) {
            self.emit(workspace_id, &run.id, "run.resolved_implementations", data)
                .await?;
        }

        let mut outputs = BTreeMap::new();
        let mut remaining = dag.upstream_counts.clone();
        let mut started = vec![false; plan.steps.len()];
        let mut finished = vec![false; plan.steps.len()];
        let mut skipped = vec![false; plan.steps.len()];
        let mut running = JoinSet::new();
        let mut first_error = None;
        let mut interrupted = false;
        for output in self.store.run_step_outputs(&run.id).await? {
            let Some((index, step)) = steps
                .iter()
                .enumerate()
                .find(|(_, step)| step.id == output.run_step_id)
            else {
                continue;
            };
            let artifact = self.store.artifact(&output.artifact_id).await?;
            outputs.insert(
                [step.node_id.clone(), output.port],
                ArtifactRef {
                    artifact_id: artifact.id,
                    storage_uri: artifact.storage_uri,
                },
            );
            started[index] = true;
        }
        for (index, step) in steps.iter().enumerate() {
            if step.state == RunStepState::Succeeded.as_str() {
                started[index] = true;
                finished[index] = true;
                for &next in &dag.downstream[index] {
                    remaining[next] = remaining[next].saturating_sub(1);
                }
            }
        }
        let mut ready: VecDeque<usize> = remaining
            .iter()
            .enumerate()
            .filter_map(|(index, count)| {
                (*count == 0 && !finished[index] && !skipped[index]).then_some(index)
            })
            .collect();

        loop {
            if interrupt.is_requested() && first_error.is_none() {
                interrupted = true;
            }

            if first_error.is_some() || interrupted {
                self.skip_unstarted_steps(
                    workspace_id,
                    &run.id,
                    &steps,
                    &started,
                    &finished,
                    &mut skipped,
                )
                .await?;
            } else {
                while running.len() < self.max_parallel_steps && !ready.is_empty() {
                    let index = ready.pop_front().expect("ready item");
                    if started[index] || skipped[index] {
                        continue;
                    }
                    let inputs = resolve_inputs(&plan.steps[index].inputs, &outputs)?;
                    started[index] = true;
                    self.spawn_step(
                        &mut running,
                        workspace_id,
                        &run.id,
                        index,
                        plan.steps[index].clone(),
                        steps[index].clone(),
                        inputs,
                        interrupt.clone(),
                        force_rerun,
                    );
                }
            }

            if finished
                .iter()
                .zip(skipped.iter())
                .all(|(is_finished, is_skipped)| *is_finished || *is_skipped)
            {
                break;
            }

            let Some(joined) = running.join_next().await else {
                break;
            };
            let (index, result) = match joined {
                Ok(value) => value,
                Err(err) => {
                    if first_error.is_none() {
                        let err = RunError::TaskJoin(err.to_string());
                        let error_json =
                            serde_json::to_string(&json!({ "error": err.public_message() }))?;
                        first_error = Some((err, error_json));
                        interrupt.request();
                        self.skip_unfinished_steps(
                            workspace_id,
                            &run.id,
                            &steps,
                            &finished,
                            &mut skipped,
                        )
                        .await?;
                    }
                    continue;
                }
            };
            finished[index] = true;

            match result {
                Ok(StepExecution::Succeeded(step_outputs)) => {
                    for output in step_outputs {
                        outputs.insert(
                            [plan.steps[index].node_id.clone(), output.port],
                            output.artifact,
                        );
                    }
                    if first_error.is_none() && !interrupted {
                        for &next in &dag.downstream[index] {
                            remaining[next] = remaining[next].saturating_sub(1);
                            if remaining[next] == 0 {
                                ready.push_back(next);
                            }
                        }
                    }
                }
                Ok(StepExecution::Interrupted) => {
                    interrupted = true;
                    interrupt.request();
                    self.skip_step(workspace_id, &run.id, &steps[index]).await?;
                    skipped[index] = true;
                }
                Err(err) => {
                    if first_error.is_none() {
                        let error = err.public_message();
                        let error_json = serde_json::to_string(&json!({ "error": error }))?;
                        self.store
                            .update_run_step_state(
                                &steps[index].id,
                                RunStepState::Failed.as_str(),
                                Some(1.0),
                                None,
                                Some(&error_json),
                            )
                            .await?;
                        self.emit_node_state(
                            workspace_id,
                            &run.id,
                            &plan.steps[index],
                            RunStepState::Failed,
                            Some(&error),
                        )
                        .await?;
                        first_error = Some((err, error_json));
                        interrupt.request();
                    }
                }
            }
        }

        let actual_cost_result = async {
            let outcome = self.outcome(&run.id).await?;
            self.record_actual_costs(&outcome).await
        }
        .await;
        if let Err(err) = actual_cost_result {
            let error_json = serde_json::to_string(&json!({
                "error": err.public_message(),
                "phase": "actual_cost_ledger"
            }))?;
            self.request_and_settle_terminal(
                workspace_id,
                &run.id,
                RunStatus::Failed.as_str(),
                Some(&error_json),
            )
            .await?;
            return Err(err);
        }

        if let Some((err, error_json)) = first_error {
            self.request_and_settle_terminal(
                workspace_id,
                &run.id,
                RunStatus::Failed.as_str(),
                Some(&error_json),
            )
            .await?;
            return Err(err);
        }

        if interrupted || interrupt.is_requested() {
            self.request_and_settle_terminal(
                workspace_id,
                &run.id,
                RunStatus::Interrupted.as_str(),
                None,
            )
            .await?;
            return self.outcome(&run.id).await;
        }

        self.store
            .update_run_status(&run.id, RunStatus::Succeeded.as_str(), None)
            .await?;
        self.emit(workspace_id, &run.id, "run.succeeded", json!({}))
            .await?;
        self.outcome(&run.id).await
    }

    #[allow(clippy::too_many_arguments)] // Explicit execution context keeps spawned ownership visible.
    fn spawn_step(
        &self,
        running: &mut JoinSet<(usize, RunResult<StepExecution>)>,
        workspace_id: &str,
        run_id: &str,
        index: usize,
        step: ExecutionStep,
        record: RunStepRecord,
        inputs: BTreeMap<String, ArtifactRef>,
        interrupt: RunInterrupt,
        force_rerun: bool,
    ) {
        let runner = self.clone();
        let workspace_id = workspace_id.to_owned();
        let run_id = run_id.to_owned();
        running.spawn(async move {
            let result = runner
                .execute_step(
                    &workspace_id,
                    &run_id,
                    &step,
                    &record,
                    inputs,
                    &interrupt,
                    force_rerun,
                )
                .await;
            (index, result)
        });
    }

    #[allow(clippy::too_many_arguments)] // Mirrors the durable step execution boundary above.
    async fn execute_step(
        &self,
        workspace_id: &str,
        run_id: &str,
        step: &ExecutionStep,
        record: &RunStepRecord,
        inputs: BTreeMap<String, ArtifactRef>,
        interrupt: &RunInterrupt,
        force_rerun: bool,
    ) -> RunResult<StepExecution> {
        self.validate_run_fix_child_guard(run_id).await?;
        let cache_key = self.step_cache_key(step, &inputs).await?;
        if !force_rerun
            && let Some(outputs) = self
                .try_cache_hit(workspace_id, run_id, step, record, &cache_key)
                .await?
        {
            return Ok(StepExecution::Succeeded(outputs));
        }

        self.store
            .update_run_step_state(
                &record.id,
                RunStepState::Running.as_str(),
                Some(0.0),
                None,
                None,
            )
            .await?;
        self.emit_node_state(workspace_id, run_id, step, RunStepState::Running, None)
            .await?;

        let (cache_links, step_outputs, cost_json) =
            if let (Some(provider), Some(capability)) = (&step.provider, &step.capability) {
                let input_texts = crate::input_materialize::materialize_text_inputs(
                    &self.store,
                    &self.artifact_root,
                    &inputs,
                )
                .await?;
                let request = ProviderRequest {
                    provider: provider.clone(),
                    capability: capability.clone(),
                    node_id: step.node_id.clone(),
                    run_id: run_id.to_owned(),
                    inputs: inputs.clone(),
                    input_texts,
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
                let Some(execution) = self
                    .execute_provider_step(workspace_id, run_id, step, record, request, interrupt)
                    .await?
                else {
                    return Ok(StepExecution::Interrupted);
                };
                (
                    execution.cache_links,
                    execution.outputs,
                    Some(execution.cost_json),
                )
            } else {
                let builtin = self
                    .execute_builtin(workspace_id, run_id, step, record, &inputs)
                    .await?;
                (builtin.cache_links, builtin.outputs, None)
            };

        if interrupt.is_requested() {
            return Ok(StepExecution::Interrupted);
        }

        let output_mappings = step_outputs
            .iter()
            .map(|output| (output.port.clone(), output.artifact.artifact_id.clone()))
            .collect::<Vec<_>>();
        if !self
            .store
            .finalize_run_step_success(&record.id, None, cost_json.as_deref(), &output_mappings)
            .await?
        {
            return Err(RunError::ArtifactPersistence(
                "step finalizer lost its durable state".to_owned(),
            ));
        }
        self.refresh_step_cache(workspace_id, step, &cache_key, &cache_links)
            .await?;
        self.emit_node_state(workspace_id, run_id, step, RunStepState::Succeeded, None)
            .await?;
        Ok(StepExecution::Succeeded(step_outputs))
    }

    async fn execute_builtin(
        &self,
        workspace_id: &str,
        run_id: &str,
        step: &ExecutionStep,
        record: &RunStepRecord,
        inputs: &BTreeMap<String, ArtifactRef>,
    ) -> RunResult<BuiltinExecution> {
        let mut cache_links = Vec::new();
        let mut outputs = Vec::new();
        match step.node_type.as_str() {
            "input.text" => {
                let text = step
                    .params
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RunError::MissingParam {
                        node_id: step.node_id.clone(),
                        param: "text".to_owned(),
                    })?;
                let artifact = self
                    .store
                    .create_artifact(NewArtifact {
                        workspace_id,
                        run_id: Some(run_id),
                        run_step_id: Some(&record.id),
                        node_id: Some(&step.node_id),
                        kind: "text",
                        storage_uri: &format!(
                            "workspace://inputs/{run_id}/{}/text.txt",
                            step.node_id
                        ),
                        sha256: None,
                        mime: Some("text/plain"),
                        width: None,
                        height: None,
                        duration_ms: None,
                        selected: false,
                        meta_json: Some(&serde_json::to_string(&json!({ "text": text }))?),
                    })
                    .await?;
                cache_links.push(CachedArtifactLink {
                    port: Some("text".to_owned()),
                    artifact_id: artifact.id.clone(),
                });
                outputs.push(StepOutput {
                    port: "text".to_owned(),
                    artifact: ArtifactRef {
                        artifact_id: artifact.id,
                        storage_uri: artifact.storage_uri,
                    },
                });
            }
            "input.image" => {
                let storage_uri = step
                    .params
                    .get("storage_uri")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RunError::MissingParam {
                        node_id: step.node_id.clone(),
                        param: "storage_uri".to_owned(),
                    })?;
                // Resolve upload://{id} references to the persisted local
                // file so image inputs are real bytes, not opaque strings
                // (HF-008).
                let resolved = if let Some(upload_id) = storage_uri.strip_prefix("upload://") {
                    // Scope the lookup to the run's workspace so a graph
                    // cannot reference another workspace's uploads.
                    let upload = self.store.workspace_upload(workspace_id, upload_id).await?;
                    let full_path = self.artifact_root.join(&upload.file_path);
                    if tokio::fs::metadata(&full_path).await.is_err() {
                        return Err(RunError::ArtifactPersistence(format!(
                            "uploaded image `{upload_id}` file is missing: {}",
                            upload.file_path
                        )));
                    }
                    Some(upload)
                } else {
                    None
                };
                let artifact = self
                    .store
                    .create_artifact(NewArtifact {
                        workspace_id,
                        run_id: Some(run_id),
                        run_step_id: Some(&record.id),
                        node_id: Some(&step.node_id),
                        kind: "image",
                        storage_uri: resolved
                            .as_ref()
                            .map(|upload| upload.file_path.as_str())
                            .unwrap_or(storage_uri),
                        sha256: resolved.as_ref().map(|upload| upload.sha256.as_str()),
                        mime: resolved.as_ref().and_then(|upload| upload.mime.as_deref()),
                        width: None,
                        height: None,
                        duration_ms: None,
                        selected: false,
                        meta_json: None,
                    })
                    .await?;
                cache_links.push(CachedArtifactLink {
                    port: Some("image".to_owned()),
                    artifact_id: artifact.id.clone(),
                });
                outputs.push(StepOutput {
                    port: "image".to_owned(),
                    artifact: ArtifactRef {
                        artifact_id: artifact.id,
                        storage_uri: artifact.storage_uri,
                    },
                });
            }
            "output.save" => {
                let artifact_ref =
                    inputs
                        .get("artifact")
                        .ok_or_else(|| RunError::MissingInput {
                            node_id: step.node_id.clone(),
                            port: "artifact".to_owned(),
                        })?;
                let source = self.store.artifact(&artifact_ref.artifact_id).await?;
                let artifact = self
                    .store
                    .create_artifact(NewArtifact {
                        workspace_id,
                        run_id: Some(run_id),
                        run_step_id: Some(&record.id),
                        node_id: Some(&step.node_id),
                        kind: &source.kind,
                        storage_uri: &source.storage_uri,
                        sha256: source.sha256.as_deref(),
                        mime: source.mime.as_deref(),
                        width: source.width,
                        height: source.height,
                        duration_ms: source.duration_ms,
                        selected: true,
                        meta_json: source.meta_json.as_deref(),
                    })
                    .await?;
                cache_links.push(CachedArtifactLink {
                    port: None,
                    artifact_id: artifact.id,
                });
            }
            other => return Err(RunError::UnsupportedBuiltin(other.to_owned())),
        }

        Ok(BuiltinExecution {
            outputs,
            cache_links,
        })
    }

    pub(crate) async fn skip_steps(
        &self,
        workspace_id: &str,
        run_id: &str,
        steps: &[RunStepRecord],
    ) -> RunResult<()> {
        for step in steps {
            self.skip_step(workspace_id, run_id, step).await?;
        }
        Ok(())
    }

    async fn skip_unstarted_steps(
        &self,
        workspace_id: &str,
        run_id: &str,
        steps: &[RunStepRecord],
        started: &[bool],
        finished: &[bool],
        skipped: &mut [bool],
    ) -> RunResult<()> {
        for (index, step) in steps.iter().enumerate() {
            if !started[index] && !finished[index] && !skipped[index] {
                self.skip_step(workspace_id, run_id, step).await?;
                skipped[index] = true;
            }
        }
        Ok(())
    }

    async fn skip_unfinished_steps(
        &self,
        workspace_id: &str,
        run_id: &str,
        steps: &[RunStepRecord],
        finished: &[bool],
        skipped: &mut [bool],
    ) -> RunResult<()> {
        for (index, step) in steps.iter().enumerate() {
            if !finished[index] && !skipped[index] {
                self.skip_step(workspace_id, run_id, step).await?;
                skipped[index] = true;
            }
        }
        Ok(())
    }

    async fn skip_step(
        &self,
        workspace_id: &str,
        run_id: &str,
        step: &RunStepRecord,
    ) -> RunResult<()> {
        self.store
            .update_run_step_state(&step.id, RunStepState::Skipped.as_str(), None, None, None)
            .await?;
        self.emit(
            workspace_id,
            run_id,
            "node.state",
            json!({
                "node_id": step.node_id,
                "node_type": step.node_type,
                "state": RunStepState::Skipped.as_str()
            }),
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn emit_node_state(
        &self,
        workspace_id: &str,
        run_id: &str,
        step: &ExecutionStep,
        state: RunStepState,
        error: Option<&str>,
    ) -> RunResult<()> {
        let mut data = json!({
            "node_id": step.node_id,
            "node_type": step.node_type,
            "provider": step.provider,
            "state": state.as_str()
        });
        if let Some(error) = error
            && let Some(map) = data.as_object_mut()
        {
            map.insert("error".to_owned(), Value::String(error.to_owned()));
        }
        self.emit(workspace_id, run_id, "node.state", data).await
    }

    pub(crate) async fn emit(
        &self,
        workspace_id: &str,
        run_id: &str,
        ev: &str,
        data: Value,
    ) -> RunResult<()> {
        let data_json = serde_json::to_string(&data)?;
        let event = self.store.append_run_event(run_id, ev, &data_json).await?;
        let _ = self.events.publish(RunEventEnvelope {
            workspace_id: workspace_id.to_owned(),
            run_id: event.run_id,
            seq: event.seq,
            server_time: event.created_at,
            ev: event.ev,
            data,
        });
        Ok(())
    }

    pub(crate) async fn outcome(&self, run_id: &str) -> RunResult<RunOutcome> {
        Ok(RunOutcome {
            run: self.store.run(run_id).await?,
            steps: self.store.run_steps(run_id).await?,
            artifacts: self.store.run_artifacts(run_id).await?,
        })
    }
}

pub(crate) fn resolve_inputs(
    input_edges: &BTreeMap<String, [String; 2]>,
    outputs: &OutputMap,
) -> RunResult<BTreeMap<String, ArtifactRef>> {
    let mut inputs = BTreeMap::new();
    for (port, source) in input_edges {
        let artifact = outputs
            .get(source)
            .ok_or_else(|| RunError::MissingInput {
                node_id: source[0].clone(),
                port: source[1].clone(),
            })?
            .clone();
        inputs.insert(port.clone(), artifact);
    }
    Ok(inputs)
}

pub(crate) fn artifact_kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Text => "text",
        ArtifactKind::Image => "image",
        ArtifactKind::Video => "video",
        ArtifactKind::Json => "json",
    }
}

#[cfg(test)]
mod tests;
