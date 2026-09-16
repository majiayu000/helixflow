use std::time::Duration;

use reqwest::{Url, multipart};
use serde_json::{Value, json};

use super::{
    super::AtlasProvider,
    pixel_geometry::{
        MAX_IMAGE_EDGE, decode_image, encode_png, gpt_edit_size, has_transparent_and_opaque,
        nearest_nano_aspect_ratio, prepare_image, restore_protected_pixels, size_tier,
        upscale_scale,
    },
};
use crate::{
    ImageProcessingCapabilities, ImageProcessingIntent, ImageProcessingOutput,
    ImageProcessingProfile, ImageProcessingRequest, ImageProcessingSubmission, ProviderError,
    ProviderResultValue,
    image_processing::{IMAGE_PROCESSING_PROFILES, ProfileSpec},
};

const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

impl AtlasProvider {
    pub fn image_processing_capabilities(&self) -> ImageProcessingCapabilities {
        let intents = [
            ImageProcessingIntent::Outpaint,
            ImageProcessingIntent::Inpaint,
            ImageProcessingIntent::Cutout,
            ImageProcessingIntent::Upscale,
            ImageProcessingIntent::Enhance,
        ];
        let defaults = intents
            .into_iter()
            .map(|intent| {
                (
                    intent.as_str().to_owned(),
                    intent.default_profile().to_owned(),
                )
            })
            .collect();
        let profiles = intents
            .into_iter()
            .map(|intent| {
                let values = IMAGE_PROCESSING_PROFILES
                    .iter()
                    .filter(|profile| profile.family == intent.family())
                    .map(|profile| ImageProcessingProfile {
                        name: profile.name,
                        model: profile.model,
                        supports_quality: profile.supports_quality,
                        supports_size_tier: profile.supports_size_tier,
                    })
                    .collect();
                (intent.as_str().to_owned(), values)
            })
            .collect();
        ImageProcessingCapabilities {
            provider: self.config.provider_id.to_owned(),
            defaults,
            profiles,
        }
    }

    pub async fn process_image(
        &self,
        request: ImageProcessingRequest,
    ) -> ProviderResultValue<ImageProcessingOutput> {
        self.submit_image(request).await?.complete().await
    }

    pub async fn submit_image(
        &self,
        request: ImageProcessingRequest,
    ) -> ProviderResultValue<ImageProcessingSubmission> {
        if request.source.is_empty() || request.source.len() > MAX_IMAGE_BYTES {
            return Err(invalid("source image must be between 1 byte and 32 MiB"));
        }
        let profile = resolve_profile(request.intent, request.profile.as_deref())?;
        let prepared = prepare_image(request.intent, &request.source, &request.parameters)?;
        let width = prepared.image.width();
        let height = prepared.image.height();
        let uploaded = self
            .upload_processing_image(encode_png(&prepared.image)?)
            .await?;
        let (body, expected_size, require_alpha) = provider_body(
            request.intent,
            profile,
            &uploaded,
            &request.parameters,
            width,
            height,
        )?;
        let response = self
            .post_json(
                format!("{}/api/v1/model/generateImage", self.api_root()),
                body,
            )
            .await?;
        let provider_task_id = prediction_id(&response);
        let provider = self.clone();
        let model = profile.model.to_owned();
        let submission_model = model.clone();
        Ok(ImageProcessingSubmission::new(
            provider_task_id,
            submission_model,
            async move {
                let (_, output_url) = provider.wait_for_processing_output(response).await?;
                let mut output =
                    decode_image(&provider.download_processing_output(&output_url).await?)?;
                if let Some((expected_width, expected_height)) = expected_size
                    && (output.width() != expected_width || output.height() != expected_height)
                {
                    return Err(ProviderError::InvalidResponse(format!(
                        "image output size {}x{} does not match expected {expected_width}x{expected_height}",
                        output.width(),
                        output.height()
                    )));
                }
                if matches!(
                    request.intent,
                    ImageProcessingIntent::Outpaint | ImageProcessingIntent::Inpaint
                ) {
                    output = restore_protected_pixels(output, &prepared);
                }
                if require_alpha && !has_transparent_and_opaque(&output) {
                    return Err(ProviderError::InvalidResponse(
                        "cutout output must contain transparent background and non-empty foreground"
                            .to_owned(),
                    ));
                }
                Ok(ImageProcessingOutput {
                    model,
                    bytes: encode_png(&output)?,
                    width: output.width(),
                    height: output.height(),
                })
            },
        ))
    }

    async fn upload_processing_image(&self, bytes: Vec<u8>) -> ProviderResultValue<String> {
        let part = multipart::Part::bytes(bytes)
            .file_name("helixflow-image.png")
            .mime_str("image/png")
            .map_err(|_| invalid("could not build image upload"))?;
        let response = self
            .client
            .post(format!("{}/api/v1/model/uploadMedia", self.api_root()))
            .bearer_auth(&self.config.api_key)
            .headers(self.extra_headers())
            .multipart(multipart::Form::new().part("file", part))
            .send()
            .await
            .map_err(|error| {
                ProviderError::RequestFailed(crate::redact::redact_sensitive(
                    &error.to_string(),
                    &self.config.api_key,
                ))
            })?;
        let payload = super::super::response_json(response, &self.config.api_key).await?;
        payload
            .get("url")
            .or_else(|| payload.get("download_url"))
            .or_else(|| payload.pointer("/data/url"))
            .or_else(|| payload.pointer("/data/download_url"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                ProviderError::InvalidResponse("image upload response missing URL".to_owned())
            })
    }

    async fn wait_for_processing_output(
        &self,
        response: Value,
    ) -> ProviderResultValue<(Option<String>, String)> {
        if let Some(output) = first_output_url(&response) {
            return Ok((prediction_id(&response), output));
        }
        let id = prediction_id(&response).ok_or_else(|| {
            ProviderError::InvalidResponse(
                "image response missing output URL and provider task ID".to_owned(),
            )
        })?;
        let deadline = tokio::time::Instant::now() + self.config.poll_timeout;
        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(ProviderError::RequestFailed(
                    "image processing timed out".to_owned(),
                ));
            }
            let payload = self
                .get_json(format!("{}/api/v1/model/prediction/{id}", self.api_root()))
                .await?;
            if let Some(output) = first_output_url(&payload) {
                return Ok((Some(id), output));
            }
            match prediction_status(&payload) {
                Some("failed" | "error" | "canceled" | "cancelled") => {
                    let detail = prediction_error(&payload)
                        .map(|message| format!(": {message}"))
                        .unwrap_or_default();
                    return Err(ProviderError::RequestRejected(format!(
                        "remote image task failed{detail}"
                    )));
                }
                _ => tokio::time::sleep(self.config.poll_interval).await,
            }
        }
    }

    async fn download_processing_output(&self, raw_url: &str) -> ProviderResultValue<Vec<u8>> {
        let response = self
            .client
            .get(safe_output_url(raw_url)?)
            .timeout(Duration::from_secs(90))
            .send()
            .await
            .map_err(|error| ProviderError::RequestFailed(error.to_string()))?;
        if !response.status().is_success() {
            return Err(ProviderError::InvalidResponse(format!(
                "image output returned HTTP {}",
                response.status().as_u16()
            )));
        }
        if response
            .content_length()
            .is_some_and(|length| length == 0 || length > MAX_IMAGE_BYTES as u64)
        {
            return Err(ProviderError::InvalidResponse(
                "image output size is invalid".to_owned(),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ProviderError::RequestFailed(error.to_string()))?;
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
            return Err(ProviderError::InvalidResponse(
                "image output must be between 1 byte and 32 MiB".to_owned(),
            ));
        }
        Ok(bytes.to_vec())
    }
}

fn resolve_profile(
    intent: ImageProcessingIntent,
    requested: Option<&str>,
) -> ProviderResultValue<&'static ProfileSpec> {
    let name = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| intent.default_profile());
    IMAGE_PROCESSING_PROFILES
        .iter()
        .find(|profile| profile.family == intent.family() && profile.name == name)
        .ok_or_else(|| {
            invalid(&format!(
                "profile `{name}` is not supported for {}",
                intent.as_str()
            ))
        })
}

type ProcessingBody = (Value, Option<(u32, u32)>, bool);

fn provider_body(
    intent: ImageProcessingIntent,
    profile: &ProfileSpec,
    image_url: &str,
    parameters: &Value,
    width: u32,
    height: u32,
) -> ProviderResultValue<ProcessingBody> {
    let model = profile.model;
    match intent {
        ImageProcessingIntent::Outpaint | ImageProcessingIntent::Inpaint => {
            let prompt = required_string(parameters, "prompt")?;
            let mut body = json!({ "model": model, "prompt": prompt });
            if model == "atlascloud/qwen-image/edit" {
                body["image"] = json!(image_url);
            } else {
                body["images"] = json!([image_url]);
            }
            if model.starts_with("google/nano-banana") {
                body["enable_sync_mode"] = json!(true);
                body["output_format"] = json!("png");
            }
            if model == "google/nano-banana-2/edit" {
                let tier = size_tier(parameters)?;
                body["aspect_ratio"] = json!(nearest_nano_aspect_ratio(width, height));
                body["resolution"] = json!(tier);
                body["media_resolution"] = json!("high");
            }
            let expected = if model == "openai/gpt-image-2/edit" {
                let quality = parameters
                    .get("quality")
                    .and_then(Value::as_str)
                    .unwrap_or("low");
                if !matches!(quality, "low" | "medium" | "high") {
                    return Err(invalid("quality must be low, medium, or high"));
                }
                let size = gpt_edit_size(width, height, size_tier(parameters)?)?;
                body["quality"] = json!(quality);
                body["size"] = json!(format!("{}x{}", size.0, size.1));
                Some(size)
            } else {
                None
            };
            Ok((body, expected, false))
        }
        ImageProcessingIntent::Cutout => Ok((
            json!({ "model": model, "image": image_url }),
            Some((width, height)),
            true,
        )),
        ImageProcessingIntent::Enhance => Ok((
            json!({ "model": model, "image": image_url, "output_format": "png" }),
            Some((width, height)),
            false,
        )),
        ImageProcessingIntent::Upscale => {
            let scale = upscale_scale(parameters)?;
            let face = parameters
                .get("facePolicy")
                .and_then(Value::as_str)
                .is_some_and(|value| value == "on");
            let body = match model {
                "tencent/image/upscaler" => json!({
                    "model": model,
                    "image_url": image_url,
                    "type": if face { "ultra" } else { "fidelity" },
                    "mode": "percent",
                    "percent": scale,
                    "encode_format": "PNG"
                }),
                "atlascloud/image-upscaler" => json!({
                    "model": model,
                    "image": image_url,
                    "outscale": scale,
                    "output_format": "png"
                }),
                "atlascloud/real-esrgan" => json!({
                    "model": model,
                    "image": image_url,
                    "scale": scale,
                    "face_enhance": face
                }),
                _ => return Err(invalid("upscale profile is not supported")),
            };
            let expected = width
                .checked_mul(scale)
                .zip(height.checked_mul(scale))
                .filter(|(width, height)| *width <= MAX_IMAGE_EDGE && *height <= MAX_IMAGE_EDGE)
                .ok_or_else(|| invalid("upscale output exceeds the image limit"))?;
            Ok((body, Some(expected), false))
        }
    }
}

fn required_string(parameters: &Value, key: &str) -> ProviderResultValue<String> {
    parameters
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid(&format!("{key} must not be empty")))
}

fn first_output_url(payload: &Value) -> Option<String> {
    let data = payload.get("data").unwrap_or(payload);
    data.get("outputs")
        .and_then(Value::as_array)
        .and_then(|outputs| {
            outputs.iter().find_map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .or_else(|| item.get("url").and_then(Value::as_str).map(str::to_owned))
            })
        })
        .or_else(|| {
            data.get("output")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|url| url.starts_with("http") && !url.contains("/prediction/"))
}

fn prediction_id(payload: &Value) -> Option<String> {
    let data = payload.get("data").unwrap_or(payload);
    data.get("id")
        .and_then(Value::as_str)
        .or_else(|| payload.get("id").and_then(Value::as_str))
        .map(str::to_owned)
}

fn prediction_status(payload: &Value) -> Option<&str> {
    payload
        .get("status")
        .or_else(|| payload.pointer("/data/status"))
        .and_then(Value::as_str)
}

fn prediction_error(payload: &Value) -> Option<String> {
    let raw = payload
        .pointer("/data/error")
        .or_else(|| payload.get("error"))?;
    let message = raw
        .as_str()
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .and_then(|value| value.get("error_message")?.as_str().map(str::to_owned))
        .or_else(|| {
            raw.get("error_message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })?;
    let message = message.trim();
    (!message.is_empty()).then(|| message.chars().take(500).collect())
}

fn safe_output_url(raw: &str) -> ProviderResultValue<Url> {
    let url = Url::parse(raw)
        .map_err(|_| ProviderError::InvalidResponse("image output URL is invalid".to_owned()))?;
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ProviderError::InvalidResponse(
            "image output URL is not allowed".to_owned(),
        ));
    }
    Ok(url)
}

pub(super) fn invalid(message: &str) -> ProviderError {
    ProviderError::InvalidRequest(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::image_edit::pixel_geometry::{pad_image, prepare_inpaint};
    use image::{ImageBuffer, Rgba, RgbaImage};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn solid(width: u32, height: u32, alpha: u8) -> RgbaImage {
        ImageBuffer::from_pixel(width, height, Rgba([20, 40, 60, alpha]))
    }

    #[test]
    fn capabilities_cover_all_five_intents() {
        let provider = AtlasProvider::new(crate::ApiProviderConfig::atlas(
            "test-key".to_owned(),
            "http://127.0.0.1:1/v1".to_owned(),
        ));
        let capabilities = provider.image_processing_capabilities();
        assert_eq!(capabilities.defaults.len(), 5);
        assert_eq!(capabilities.defaults["outpaint"], "gpt-image-2");
        assert!(
            capabilities.profiles["cutout"]
                .iter()
                .any(|item| item.name == "youchuan-v8.2")
        );
    }

    #[test]
    fn outpaint_prepares_a_transparent_border() {
        let source = solid(2, 2, 255);
        let output = pad_image(
            &source,
            &json!({"left": 1, "top": 0, "right": 0, "bottom": 1, "prompt": "fill"}),
        )
        .unwrap();
        assert_eq!((output.width(), output.height()), (3, 3));
        assert_eq!(output.get_pixel(0, 0).0[3], 0);
        assert_eq!(output.get_pixel(1, 0).0[3], 255);
    }

    #[test]
    fn outpaint_rejects_overflowing_borders() {
        let error = pad_image(
            &solid(2, 2, 255),
            &json!({
                "left": u32::MAX,
                "top": u32::MAX,
                "right": 0,
                "bottom": 0,
                "prompt": "fill"
            }),
        )
        .unwrap_err();
        assert!(error.to_string().contains("border exceeds"));
    }

    #[test]
    fn inpaint_requires_a_real_alpha_selection() {
        assert!(prepare_inpaint(solid(4, 4, 255), &json!({"prompt": "fill"})).is_err());
        let output = prepare_inpaint(
            solid(4, 4, 255),
            &json!({"x": 1, "y": 1, "width": 2, "height": 2, "prompt": "fill"}),
        )
        .unwrap();
        assert_eq!(output.get_pixel(1, 1).0[3], 0);
        assert_eq!(output.get_pixel(0, 0).0[3], 255);
    }

    #[test]
    fn bodies_are_operation_specific() {
        let cutout = IMAGE_PROCESSING_PROFILES
            .iter()
            .find(|item| item.name == "youchuan-v8.2")
            .unwrap();
        let (body, expected, alpha) = provider_body(
            ImageProcessingIntent::Cutout,
            cutout,
            "https://cdn.example/input.png",
            &json!({}),
            100,
            80,
        )
        .unwrap();
        assert_eq!(body["image"], "https://cdn.example/input.png");
        assert!(body.get("prompt").is_none());
        assert_eq!(expected, Some((100, 80)));
        assert!(alpha);

        let upscale = IMAGE_PROCESSING_PROFILES
            .iter()
            .find(|item| item.name == "tencent")
            .unwrap();
        let (body, expected, _) = provider_body(
            ImageProcessingIntent::Upscale,
            upscale,
            "https://cdn.example/input.png",
            &json!({"scale": 4, "facePolicy": "off"}),
            100,
            80,
        )
        .unwrap();
        assert_eq!(body["percent"], 4);
        assert_eq!(expected, Some((400, 320)));

        let banana = IMAGE_PROCESSING_PROFILES
            .iter()
            .find(|item| item.name == "banana-2")
            .unwrap();
        let (body, _, _) = provider_body(
            ImageProcessingIntent::Outpaint,
            banana,
            "https://cdn.example/input.png",
            &json!({"prompt": "fill", "sizeTier": "2k"}),
            1600,
            900,
        )
        .unwrap();
        assert_eq!(body["aspect_ratio"], "16:9");
        assert_eq!(body["resolution"], "2k");
        assert_eq!(body["media_resolution"], "high");
    }

    #[test]
    fn edit_geometry_preserves_canvas_size_and_inpaint_locked_pixels() {
        assert_eq!(gpt_edit_size(1600, 900, "2k").unwrap(), (2048, 1152));
        assert_eq!(gpt_edit_size(900, 1600, "2k").unwrap(), (1152, 2048));
        let one_k = gpt_edit_size(500, 307, "1k").unwrap();
        assert!(u64::from(one_k.0) * u64::from(one_k.1) >= 1024 * 1024);
        assert_eq!(one_k, (1312, 800));

        let source = solid(2, 2, 255);
        let prepared = prepare_image(
            ImageProcessingIntent::Outpaint,
            &encode_png(&source).unwrap(),
            &json!({
                "left": 1,
                "top": 1,
                "right": 1,
                "bottom": 1,
                "prompt": "fill"
            }),
        )
        .unwrap();
        let generated = ImageBuffer::from_pixel(8, 8, Rgba([200, 180, 160, 255]));
        let restored = restore_protected_pixels(generated, &prepared);
        assert_eq!(restored.dimensions(), (4, 4));
        assert_eq!(restored.get_pixel(0, 0).0, [200, 180, 160, 255]);

        let inpaint = prepare_image(
            ImageProcessingIntent::Inpaint,
            &encode_png(&solid(4, 4, 255)).unwrap(),
            &json!({"x": 1, "y": 1, "width": 2, "height": 2, "prompt": "fill"}),
        )
        .unwrap();
        let restored = restore_protected_pixels(
            ImageBuffer::from_pixel(8, 8, Rgba([200, 180, 160, 255])),
            &inpaint,
        );
        assert_eq!(restored.get_pixel(0, 0).0, [20, 40, 60, 255]);
        assert_eq!(restored.get_pixel(1, 1).0, [200, 180, 160, 255]);
    }

    #[test]
    fn output_urls_fail_closed() {
        assert!(safe_output_url("file:///tmp/out.png").is_err());
        assert!(safe_output_url("http://example.com/out.png").is_err());
        assert!(safe_output_url("https://example.com/out.png").is_ok());
        assert!(safe_output_url("http://127.0.0.1:9000/out.png").is_ok());
    }

    #[tokio::test]
    async fn atlas_processing_runs_upload_generate_and_download() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let output = encode_png(&solid(3, 2, 255)).unwrap();
        let server = tokio::spawn(async move {
            let (upload, stream) = read_request(&listener).await;
            assert!(upload.starts_with("POST /api/v1/model/uploadMedia HTTP/1.1"));
            respond_json(
                stream,
                &format!(r#"{{"url":"http://{address}/uploaded.png"}}"#),
            )
            .await;

            let (generate, stream) = read_request(&listener).await;
            assert!(generate.starts_with("POST /api/v1/model/generateImage HTTP/1.1"));
            assert!(generate.contains("google/nano-banana-2/edit"));
            assert!(generate.contains("fill border"));
            respond_json(
                stream,
                &format!(
                    r#"{{"data":{{"id":"provider-task-1","outputs":["http://{address}/output.png"]}}}}"#
                ),
            )
            .await;

            let (download, stream) = read_request(&listener).await;
            assert!(download.starts_with("GET /output.png HTTP/1.1"));
            respond_bytes(stream, "image/png", &output).await;
        });
        let provider = AtlasProvider::new(crate::ApiProviderConfig::atlas(
            "test-key".to_owned(),
            format!("http://{address}/v1"),
        ));
        let source = encode_png(&solid(2, 2, 255)).unwrap();
        let result = provider
            .process_image(ImageProcessingRequest {
                intent: ImageProcessingIntent::Outpaint,
                profile: Some("banana-2".to_owned()),
                parameters: json!({
                    "left": 1,
                    "top": 0,
                    "right": 0,
                    "bottom": 0,
                    "prompt": "fill border"
                }),
                source,
            })
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(result.model, "google/nano-banana-2/edit");
        assert_eq!((result.width, result.height), (3, 2));
        assert!(!result.bytes.is_empty());
    }

    async fn read_request(listener: &tokio::net::TcpListener) -> (String, tokio::net::TcpStream) {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0_u8; 4096];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let header = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = header
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|value| value.parse::<usize>().ok())
            })
            .unwrap_or(0);
        while bytes.len() < header_end + content_length {
            let mut chunk = [0_u8; 4096];
            let count = stream.read(&mut chunk).await.unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
        }
        (String::from_utf8_lossy(&bytes).into_owned(), stream)
    }

    async fn respond_json(stream: tokio::net::TcpStream, body: &str) {
        respond_bytes(stream, "application/json", body.as_bytes()).await;
    }

    async fn respond_bytes(mut stream: tokio::net::TcpStream, mime: &str, body: &[u8]) {
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {mime}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        stream.write_all(body).await.unwrap();
    }
}
