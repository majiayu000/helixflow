use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{
    ArtifactContent, ArtifactKind, ArtifactPayload, CostEstimate, ProviderError, ProviderRequest,
    ProviderResult, ProviderResultValue,
};

pub(super) async fn response_json(
    response: reqwest::Response,
    api_key: &str,
) -> ProviderResultValue<Value> {
    let status = response.status();
    let text = response.text().await.map_err(|err| {
        ProviderError::RequestFailed(crate::redact::redact_sensitive(&err.to_string(), api_key))
    })?;
    let text = crate::redact::redact_sensitive(&text, api_key);
    if !status.is_success() {
        return Err(ProviderError::RequestRejected(format!(
            "HTTP {}: {}",
            status.as_u16(),
            truncate(&text, 512)
        )));
    }
    serde_json::from_str(&text)
        .map_err(|err| ProviderError::InvalidResponse(format!("{err}: {}", truncate(&text, 512))))
}

pub(super) fn single_output_result(output_name: &str, payload: ArtifactPayload) -> ProviderResult {
    ProviderResult {
        outputs: BTreeMap::from([(output_name.to_owned(), payload)]),
        cost: CostEstimate {
            amount: 0.0,
            currency: "USD".to_owned(),
            // Atlas does not report per-call cost; record the charge as
            // unknown instead of a fake confirmed $0 (HF-004).
            estimated: true,
            unknown: true,
        },
    }
}

pub(super) fn remote_output_result(
    output_name: &str,
    kind: ArtifactKind,
    mime: &str,
    url: String,
    meta: Value,
) -> ProviderResult {
    single_output_result(
        output_name,
        ArtifactPayload {
            kind,
            mime: mime.to_owned(),
            storage_uri: format!("provider://atlas/{}", output_name),
            content: ArtifactContent::RemoteUrl { url },
            width: None,
            height: None,
            duration_ms: None,
            meta,
        },
    )
}

pub(super) fn optional_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn is_reference_to_video(operation_id: &str) -> bool {
    operation_id.contains("/reference-to-video")
}

pub(super) fn atlas_remote_failure_reason(data: &Value) -> String {
    if let Some(text) = data
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return crate::safe_provider_message(text);
    }
    if let Some(text) = data
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("message").or_else(|| error.get("msg")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return crate::safe_provider_message(text);
    }
    if let Some(code) = data
        .get("error_code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return crate::safe_provider_message(code);
    }
    "PROVIDER_REMOTE_FAILED".to_owned()
}

/// Seedance 2.x r2v only successfully processes public HTTPS or `asset://`
/// references on the current Atlas API. Inline data URLs 502 at the gateway;
/// raw Base64 submits then fails with "could not be processed".
fn atlas_fetchable_media_ref(value: &str, kind: &str) -> ProviderResultValue<String> {
    let trimmed = value.trim();
    if trimmed.starts_with("https://") || trimmed.starts_with("asset://") {
        return Ok(trimmed.to_owned());
    }
    Err(ProviderError::InvalidRequest(format!(
        "Atlas reference-to-video {kind} inputs must be public HTTPS or asset references; local uploads cannot be inlined"
    )))
}

fn wired_string_list(params: &Value, key: &str) -> ProviderResultValue<Vec<String>> {
    let Some(value) = params.get(key) else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(ProviderError::InvalidRequest(format!(
            "{key} must be an array of strings"
        )));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(text) = item
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Err(ProviderError::InvalidRequest(format!(
                "{key} must be an array of strings"
            )));
        };
        out.push(text.to_owned());
    }
    Ok(out)
}

pub(super) fn wired_image_to_video_media(
    params: &Value,
) -> ProviderResultValue<(Vec<String>, Vec<String>, Vec<String>)> {
    let mut images = wired_string_list(params, "__helixflow_wired_images")?;
    if images.is_empty()
        && let Some(image) = optional_string(params, "__helixflow_wired_image")
    {
        images.push(image);
    }
    Ok((
        images,
        wired_string_list(params, "__helixflow_wired_videos")?,
        wired_string_list(params, "__helixflow_wired_audios")?,
    ))
}

pub(super) fn attach_image_to_video_inputs(
    body: &mut Value,
    req: &ProviderRequest,
    operation_id: &str,
) -> ProviderResultValue<()> {
    let (images, videos, audios) = wired_image_to_video_media(&req.params)?;
    body["generate_audio"] = req
        .params
        .get("generate_audio")
        .cloned()
        .unwrap_or(json!(true));
    body["seed"] = req.params.get("seed").cloned().unwrap_or(json!(-1));

    if is_reference_to_video(operation_id) {
        if images.is_empty() && videos.is_empty() {
            return Err(ProviderError::InvalidRequest(
                "reference-to-video requires at least one wired image or video input".to_owned(),
            ));
        }
        if !images.is_empty() {
            let fetchable = images
                .iter()
                .map(|image| atlas_fetchable_media_ref(image, "image"))
                .collect::<Result<Vec<_>, _>>()?;
            body["reference_images"] = json!(fetchable);
        }
        if !videos.is_empty() {
            let fetchable = videos
                .iter()
                .map(|video| atlas_fetchable_media_ref(video, "video"))
                .collect::<Result<Vec<_>, _>>()?;
            body["reference_videos"] = json!(fetchable);
        }
        if !audios.is_empty() {
            let fetchable = audios
                .iter()
                .map(|audio| atlas_fetchable_media_ref(audio, "audio"))
                .collect::<Result<Vec<_>, _>>()?;
            body["reference_audios"] = json!(fetchable);
        }
        if let Some(ratio) = optional_string(&req.params, "ratio")
            .or_else(|| optional_string(&req.params, "aspect_ratio"))
        {
            body["ratio"] = Value::String(ratio);
        }
        return Ok(());
    }

    if !videos.is_empty() || !audios.is_empty() || images.len() > 2 {
        return Err(ProviderError::InvalidRequest(format!(
            "operation `{operation_id}` is first-frame image-to-video and cannot take extra video/audio references or more than two images; pin a `/reference-to-video` binding"
        )));
    }
    let image = images.first().cloned().ok_or_else(|| {
        ProviderError::InvalidRequest("image_to_video requires wired image input".to_owned())
    })?;
    body["image"] = Value::String(image);
    if let Some(last_image) = images.get(1).cloned() {
        body["last_image"] = Value::String(last_image);
    }
    body["camera_fixed"] = req
        .params
        .get("camera_fixed")
        .cloned()
        .unwrap_or(json!(false));
    if let Some(aspect_ratio) = optional_string(&req.params, "aspect_ratio") {
        body["aspect_ratio"] = Value::String(aspect_ratio);
    }
    Ok(())
}

pub(super) fn first_output(response: &Value) -> ProviderResultValue<String> {
    let data = response.get("data").unwrap_or(response);
    if let Some(output) = data
        .get("outputs")
        .and_then(Value::as_array)
        .and_then(|outputs| outputs.iter().find_map(Value::as_str))
    {
        return Ok(output.to_owned());
    }
    if let Some(output) = data
        .get("urls")
        .and_then(Value::as_object)
        .and_then(|urls| urls.values().find_map(Value::as_str))
    {
        return Ok(output.to_owned());
    }
    Err(ProviderError::InvalidResponse(
        "Atlas response missing outputs or urls".to_owned(),
    ))
}

pub(super) fn image_mime_from_output(output: &str) -> ProviderResultValue<&'static str> {
    let url = reqwest::Url::parse(output).map_err(|_| {
        ProviderError::InvalidResponse("Atlas image output URL is invalid".to_owned())
    })?;
    let extension = url
        .path_segments()
        .and_then(Iterator::last)
        .and_then(|name| name.rsplit_once('.').map(|(_, extension)| extension))
        .map(str::to_ascii_lowercase);

    match extension.as_deref() {
        Some("png") => Ok("image/png"),
        Some("jpg") | Some("jpeg") => Ok("image/jpeg"),
        _ => Err(ProviderError::InvalidResponse(
            "Atlas image output has an unsupported media type".to_owned(),
        )),
    }
}

pub(super) fn is_safe_task_id(task_id: &str) -> bool {
    !task_id.is_empty()
        && task_id.len() <= 256
        && task_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_owned();
    }
    value.chars().take(max_len).collect::<String>()
}
