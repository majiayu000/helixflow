use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use helixflow_gateway::{
    ImageProcessingCapabilities, ImageProcessingIntent, ImageProcessingRequest, ProviderError,
};
use helixflow_store::{ImageProcessingJobRecord, ImageProcessingJobUpdate, NewImageProcessingJob};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{api_error::ApiError, app_state::AppState, upload_routes::persist_workspace_upload};

const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;

pub(crate) fn image_processing_request_limit() -> usize {
    MAX_SOURCE_BYTES + 64 * 1024
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateImageProcessingJobRequest {
    source_node_id: String,
    intent: ImageProcessingIntent,
    profile: Option<String>,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LinkImageProcessingResultRequest {
    result_node_id: String,
    output_upload_id: String,
}

pub(crate) async fn image_processing_capabilities(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<ImageProcessingCapabilities>, ApiError> {
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let provider = state.selected_provider_for_workspace(&workspace);
    state
        .provider_registry
        .image_processing_capabilities(&provider)
        .map(Json)
        .map_err(provider_error)
}

pub(crate) async fn create_image_processing_job(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let mut request = None;
    let mut source = None;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|error| ApiError::bad_request(format!("invalid multipart request: {error}")))?
    {
        match field.name() {
            Some("request") => {
                let text = field.text().await.map_err(|error| {
                    ApiError::bad_request(format!("read image processing request: {error}"))
                })?;
                request = Some(
                    serde_json::from_str::<CreateImageProcessingJobRequest>(&text).map_err(
                        |error| {
                            ApiError::bad_request(format!(
                                "invalid image processing request: {error}"
                            ))
                        },
                    )?,
                );
            }
            Some("file") => {
                let mut bytes = Vec::new();
                while let Some(chunk) = field.chunk().await.map_err(|error| {
                    ApiError::bad_request(format!("read image processing source: {error}"))
                })? {
                    let size = bytes
                        .len()
                        .checked_add(chunk.len())
                        .ok_or_else(|| ApiError::bad_request("source image size overflow"))?;
                    if size > MAX_SOURCE_BYTES {
                        return Err(ApiError::bad_request(
                            "source image exceeds the 32 MiB limit",
                        ));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                if bytes.is_empty() {
                    return Err(ApiError::bad_request("source image is empty"));
                }
                source = Some(bytes);
            }
            _ => {}
        }
    }
    let request =
        request.ok_or_else(|| ApiError::bad_request("multipart field `request` is required"))?;
    let source =
        source.ok_or_else(|| ApiError::bad_request("multipart field `file` is required"))?;
    let source_node_id = required(&request.source_node_id, "sourceNodeId")?;
    if !request.parameters.is_object() {
        return Err(ApiError::bad_request("parameters must be an object"));
    }
    let provider = state.selected_provider_for_workspace(&workspace);
    let capabilities = state
        .provider_registry
        .image_processing_capabilities(&provider)
        .map_err(provider_error)?;
    let (profile, model) =
        resolve_profile(&capabilities, request.intent, request.profile.as_deref())?;
    let job = state
        .store
        .create_image_processing_job(NewImageProcessingJob {
            workspace_id: &workspace_id,
            source_node_id,
            intent: request.intent.as_str(),
            profile: Some(profile),
        })
        .await
        .map_err(ApiError::store)?;
    let execution = ImageJobExecution {
        workspace_id: workspace_id.clone(),
        job_id: job.id.clone(),
        source_node_id: source_node_id.to_owned(),
        intent: request.intent,
        profile: profile.to_owned(),
        provider,
        model: model.to_owned(),
        parameters: request.parameters,
        source,
    };
    tokio::spawn(run_image_processing_job(state, execution));
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "job": image_processing_job_value(&job) })),
    ))
}

pub(crate) async fn get_image_processing_job(
    Path((workspace_id, job_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let job = state
        .store
        .workspace_image_processing_job(&workspace_id, &job_id)
        .await
        .map_err(|error| {
            if error.is_not_found() {
                ApiError::not_found("image processing job was not found")
            } else {
                ApiError::store(error)
            }
        })?;
    Ok(Json(image_processing_job_value(&job)))
}

pub(crate) async fn update_image_processing_job(
    Path((workspace_id, job_id)): Path<(String, String)>,
    State(state): State<AppState>,
    Json(request): Json<LinkImageProcessingResultRequest>,
) -> Result<Json<Value>, ApiError> {
    let current = state
        .store
        .workspace_image_processing_job(&workspace_id, &job_id)
        .await
        .map_err(ApiError::store)?;
    if current.status != "succeeded" {
        return Err(ApiError::conflict(
            "image processing result can only be linked after the job succeeds",
        ));
    }
    let output_upload_id = required(&request.output_upload_id, "outputUploadId")?;
    if current.output_upload_id.as_deref() != Some(output_upload_id) {
        return Err(ApiError::conflict(
            "outputUploadId does not belong to this image processing job",
        ));
    }
    let record = state
        .store
        .link_image_processing_result(
            &workspace_id,
            &job_id,
            required(&request.result_node_id, "resultNodeId")?,
            output_upload_id,
        )
        .await
        .map_err(ApiError::store)?;
    Ok(Json(image_processing_job_value(&record)))
}

pub(crate) fn image_processing_job_value(record: &ImageProcessingJobRecord) -> Value {
    json!({
        "id": record.id,
        "workspaceId": record.workspace_id,
        "sourceNodeId": record.source_node_id,
        "resultNodeId": record.result_node_id,
        "intent": record.intent,
        "profile": record.profile,
        "providerTaskId": record.provider_task_id,
        "provider": record.provider,
        "model": record.model,
        "outputUploadId": record.output_upload_id,
        "status": record.status,
        "error": record.error,
        "createdAt": record.created_at,
        "updatedAt": record.updated_at,
        "completedAt": record.completed_at,
    })
}

fn resolve_profile<'a>(
    capabilities: &'a ImageProcessingCapabilities,
    intent: ImageProcessingIntent,
    requested: Option<&str>,
) -> Result<(&'a str, &'a str), ApiError> {
    let intent = intent.as_str();
    let name = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| capabilities.defaults.get(intent).map(String::as_str))
        .ok_or_else(|| ApiError::service_unavailable(format!("{intent} has no default profile")))?;
    capabilities
        .profiles
        .get(intent)
        .and_then(|profiles| profiles.iter().find(|profile| profile.name == name))
        .map(|profile| (profile.name, profile.model))
        .ok_or_else(|| {
            ApiError::bad_request(format!("profile `{name}` is not available for {intent}"))
        })
}

struct ImageJobExecution {
    workspace_id: String,
    job_id: String,
    source_node_id: String,
    intent: ImageProcessingIntent,
    profile: String,
    provider: String,
    model: String,
    parameters: Value,
    source: Vec<u8>,
}

async fn run_image_processing_job(state: AppState, execution: ImageJobExecution) {
    let started = state
        .store
        .update_image_processing_job(
            &execution.workspace_id,
            &execution.job_id,
            ImageProcessingJobUpdate {
                status: "running",
                provider_task_id: None,
                provider: Some(&execution.provider),
                model: Some(&execution.model),
                result_node_id: None,
                output_upload_id: None,
                error: None,
            },
        )
        .await;
    if let Err(error) = started {
        eprintln!("image job {} could not start: {error}", execution.job_id);
        return;
    }

    let submission = match state
        .provider_registry
        .submit_image(
            &execution.provider,
            ImageProcessingRequest {
                intent: execution.intent,
                profile: Some(execution.profile),
                parameters: execution.parameters,
                source: execution.source,
            },
        )
        .await
    {
        Ok(submission) => submission,
        Err(error) => {
            mark_job_failed(
                &state,
                &execution.workspace_id,
                &execution.job_id,
                provider_error(error).message,
            )
            .await;
            return;
        }
    };
    if let Err(error) = state
        .store
        .update_image_processing_job(
            &execution.workspace_id,
            &execution.job_id,
            ImageProcessingJobUpdate {
                status: "running",
                provider_task_id: submission.provider_task_id.as_deref(),
                provider: None,
                model: Some(&submission.model),
                result_node_id: None,
                output_upload_id: None,
                error: None,
            },
        )
        .await
    {
        eprintln!(
            "image job {} could not save provider task: {error}",
            execution.job_id
        );
        return;
    }

    let output = match submission.complete().await {
        Ok(output) => output,
        Err(error) => {
            mark_job_failed(
                &state,
                &execution.workspace_id,
                &execution.job_id,
                provider_error(error).message,
            )
            .await;
            return;
        }
    };
    let filename = format!(
        "{}-{}.png",
        safe_filename(&execution.source_node_id),
        execution.intent.as_str()
    );
    let uploaded =
        match persist_workspace_upload(&state, &execution.workspace_id, &filename, &output.bytes)
            .await
        {
            Ok(uploaded) => uploaded,
            Err(error) => {
                mark_job_failed(
                    &state,
                    &execution.workspace_id,
                    &execution.job_id,
                    error.message,
                )
                .await;
                return;
            }
        };
    if let Err(error) = state
        .store
        .update_image_processing_job(
            &execution.workspace_id,
            &execution.job_id,
            ImageProcessingJobUpdate {
                status: "succeeded",
                provider_task_id: None,
                provider: None,
                model: Some(&output.model),
                result_node_id: None,
                output_upload_id: Some(&uploaded.id),
                error: None,
            },
        )
        .await
    {
        eprintln!(
            "image job {} could not save success: {error}",
            execution.job_id
        );
    }
}

async fn mark_job_failed(state: &AppState, workspace_id: &str, job_id: &str, message: String) {
    if let Err(error) = state
        .store
        .update_image_processing_job(
            workspace_id,
            job_id,
            ImageProcessingJobUpdate {
                status: "failed",
                provider_task_id: None,
                provider: None,
                model: None,
                result_node_id: None,
                output_upload_id: None,
                error: Some(&message),
            },
        )
        .await
    {
        eprintln!("image job {job_id} could not save failure `{message}`: {error}");
    }
}

fn provider_error(error: ProviderError) -> ApiError {
    match error {
        ProviderError::InvalidRequest(message) => ApiError::bad_request(message),
        ProviderError::Unavailable { .. } | ProviderError::UnsupportedCapability(_) => {
            ApiError::service_unavailable(error.to_string())
        }
        _ => ApiError::bad_gateway(error.to_string()),
    }
}

fn required<'a>(value: &'a str, field: &str) -> Result<&'a str, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiError::bad_request(format!("{field} must not be empty")));
    }
    Ok(trimmed)
}

fn safe_filename(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let value = value.trim_matches('-');
    if value.is_empty() {
        "image".to_owned()
    } else {
        value.chars().take(80).collect()
    }
}
