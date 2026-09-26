use super::*;

#[test]
fn atlas_outputs_accept_outputs_or_urls() {
    assert_eq!(
        first_output(&json!({ "data": { "outputs": ["https://cdn.example/image.png"] } }))
            .expect("output"),
        "https://cdn.example/image.png"
    );
    assert_eq!(
        first_output(&json!({ "data": { "urls": { "video": "https://cdn.example/out.mp4" } } }))
            .expect("url"),
        "https://cdn.example/out.mp4"
    );
}

#[test]
fn invalid_extra_header_is_rejected_at_startup() {
    let mut config = ApiProviderConfig::atlas("key".to_owned(), "https://api.test".to_owned());
    config.extra_header = Some(("bad header\n".to_owned(), "v".to_owned()));
    assert!(validate_header_config(&config).is_err());
    config.extra_header = Some(("x-team".to_owned(), "ok".to_owned()));
    assert!(validate_header_config(&config).is_ok());
}

#[test]
fn atlas_scope_fingerprint_includes_extra_header_value() {
    let mut first = ApiProviderConfig::atlas("key".to_owned(), "https://api.test".to_owned());
    first.extra_header = Some(("x-team".to_owned(), "alpha".to_owned()));
    let mut second = first.clone();
    second.extra_header = Some(("x-team".to_owned(), "beta".to_owned()));
    assert_ne!(
        AtlasProvider::new(first).config_fingerprint("atlas"),
        AtlasProvider::new(second).config_fingerprint("atlas")
    );
}

#[tokio::test]
async fn atlas_estimates_supported_capabilities() {
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        DEFAULT_ATLAS_API_BASE.to_owned(),
    ));
    for capability in ["prompt_writer", "text_to_image", "text_to_video"] {
        let req = request(capability, json!({ "prompt": "hello" }));
        let estimate = provider.estimate(req).await.expect("estimate");
        assert_eq!(estimate.currency, "USD");
    }
}

#[tokio::test]
async fn atlas_request_failures_redact_auth_material() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server =
        capture_request_and_respond(listener, 401, r#"{"error":"bearer test-key was rejected"}"#);
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        format!("http://{addr}/v1"),
    ));

    let result = provider
        .invoke(request(
            "text_to_image",
            json!({ "prompt": "hello", "aspect_ratio": "1:1" }),
        ))
        .await;
    server.await??;
    let err = result.expect_err("request should fail");

    let message = err.to_string().to_lowercase();
    assert!(message.contains("redacted"));
    assert!(!message.contains("bearer"));
    assert!(!message.contains("test-key"));
    Ok(())
}

#[tokio::test]
async fn gh130_failclosed_invokes_require_resolved_operation() {
    // GH130 T4: every model-bearing capability refuses to invoke without a
    // resolved binding — the internal defaults are gone.
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        DEFAULT_ATLAS_API_BASE.to_owned(),
    ));
    for capability in ["prompt_writer", "text_to_image", "text_to_video"] {
        let err = provider
            .invoke(unresolved_request(
                capability,
                json!({ "prompt": "hello", "aspect_ratio": "1:1" }),
            ))
            .await
            .expect_err("unresolved model must fail");
        assert!(
            matches!(err, ProviderError::ModelUnresolved { .. }),
            "capability {capability}: {err}"
        );
    }
}

#[tokio::test]
async fn gh130_resolved_operation_drives_request_model() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = capture_request_and_respond(
        listener,
        200,
        r#"{"data":{"outputs":["https://cdn.example/image.png"]}}"#,
    );
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        format!("http://{addr}/v1"),
    ));
    let result = provider
        .invoke(request(
            "text_to_image",
            json!({ "prompt": "hello", "aspect_ratio": "1:1" }),
        ))
        .await
        .expect("image result");
    let request_body = server.await??;

    assert_eq!(request_body["model"], "google/nano-banana-2/text-to-image");
    let image = result.outputs.get("image").expect("image output");
    assert_eq!(image.meta["model"], "google/nano-banana-2/text-to-image");
    Ok(())
}

#[tokio::test]
async fn atlas_image_mime_matches_the_remote_output_extension()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = capture_request_and_respond(
        listener,
        200,
        r#"{"data":{"outputs":["https://atlas-media.example/generated/image.jpg"]}}"#,
    );
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        format!("http://{addr}/v1"),
    ));

    let result = provider
        .invoke(request(
            "text_to_image",
            json!({ "prompt": "hello", "output_format": "png" }),
        ))
        .await?;
    server.await??;

    assert_eq!(result.outputs["image"].mime, "image/jpeg");
    Ok(())
}

#[test]
fn atlas_image_mime_rejects_unknown_outputs_without_exposing_the_url() {
    let output = "https://atlas-media.example/generated/image.gif?token=secret";
    let error = image_mime_from_output(output).expect_err("GIF must not be accepted");

    let message = error.to_string();
    assert!(message.contains("unsupported media type"));
    assert!(!message.contains(output));
    assert!(!message.contains("secret"));
}

#[tokio::test]
async fn atlas_requests_send_a_stable_user_agent() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = capture_raw_request_and_respond(
        listener,
        200,
        r#"{"data":{"outputs":["https://cdn.example/image.png"]}}"#,
    );
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        format!("http://{addr}/v1"),
    ));

    provider
        .invoke(request(
            "text_to_image",
            json!({ "prompt": "hello", "aspect_ratio": "1:1" }),
        ))
        .await
        .expect("image result");
    let raw_request = server.await??;

    let expected = concat!("helixflow/", env!("CARGO_PKG_VERSION"));
    assert!(
        raw_request
            .lines()
            .any(|line| line.eq_ignore_ascii_case(&format!("user-agent: {expected}")))
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires Atlas credentials and incurs a real image-generation charge"]
async fn atlas_live_image_generation_returns_a_remote_artifact()
-> Result<(), Box<dyn std::error::Error>> {
    let provider = AtlasProvider::from_env().ok_or("Atlas credentials are not configured")?;

    let result = provider
        .invoke(request(
            "text_to_image",
            json!({
                "prompt": "A minimal black square centered on a plain white background, Helixflow integration test",
                "aspect_ratio": "1:1",
                "output_format": "png",
                "num_images": 1
            }),
        ))
        .await?;
    let image = result.outputs.get("image").ok_or("missing image output")?;

    assert_eq!(image.kind, ArtifactKind::Image);
    let ArtifactContent::RemoteUrl { url } = &image.content else {
        return Err("image output was not a remote URL".into());
    };
    assert!(url.starts_with("https://"));
    assert_eq!(image.mime, image_mime_from_output(url)?);
    Ok(())
}

fn request(capability: &str, params: Value) -> ProviderRequest {
    let operation_id = match capability {
        "prompt_writer" => "deepseek-ai/DeepSeek-V3-0324",
        "text_to_image" => "google/nano-banana-2/text-to-image",
        "image_edit" => "google/nano-banana-2/edit",
        "image_to_video" => "bytedance/seedance-v1.5-pro/image-to-video",
        _ => "bytedance/seedance-v1.5-pro/text-to-video-fast",
    };
    ProviderRequest {
        provider: "atlas".to_owned(),
        capability: capability.to_owned(),
        node_id: "node".to_owned(),
        run_id: "run".to_owned(),
        inputs: BTreeMap::new(),
        input_texts: BTreeMap::new(),
        params,
        resolved_model_id: Some("resolved/model".to_owned()),
        operation_id: Some(operation_id.to_owned()),
    }
}

#[tokio::test]
async fn atlas_image_to_video_dispatch_sends_the_wired_image()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = capture_request_and_respond(listener, 200, r#"{"data":{"id":"prediction_i2v"}}"#);
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        format!("http://{addr}/v1"),
    ));

    let dispatch = provider
        .dispatch(request(
            "image_to_video",
            json!({
                "prompt": "slow camera move",
                "duration_sec": 5,
                "__helixflow_wired_image": "data:image/png;base64,iVBORw0KGgo="
            }),
        ))
        .await
        .expect("dispatch");
    let request_body = server.await??;

    assert!(matches!(dispatch, ProviderDispatch::Accepted(_)));
    assert_eq!(
        request_body["model"],
        "bytedance/seedance-v1.5-pro/image-to-video"
    );
    assert_eq!(request_body["image"], "data:image/png;base64,iVBORw0KGgo=");
    assert_eq!(request_body["generate_audio"], true);
    assert_eq!(request_body["resolution"], "720p");
    Ok(())
}

#[tokio::test]
async fn atlas_image_edit_sends_wired_images() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = capture_request_and_respond(
        listener,
        200,
        r#"{"data":{"outputs":["https://cdn.example/edited.png"]}}"#,
    );
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        format!("http://{addr}/v1"),
    ));
    let result = provider
        .invoke(request(
            "image_edit",
            json!({
                "prompt": "fill the transparent border",
                "__helixflow_wired_image": "https://cdn.example/source.png"
            }),
        ))
        .await
        .expect("image edit result");
    let request_body = server.await??;
    assert_eq!(request_body["model"], "google/nano-banana-2/edit");
    assert_eq!(request_body["images"][0], "https://cdn.example/source.png");
    assert_eq!(request_body["prompt"], "fill the transparent border");
    assert!(result.outputs.contains_key("image"));
    Ok(())
}

fn unresolved_request(capability: &str, params: Value) -> ProviderRequest {
    ProviderRequest {
        resolved_model_id: None,
        operation_id: None,
        ..request(capability, params)
    }
}

fn capture_request_and_respond(
    listener: tokio::net::TcpListener,
    status: u16,
    response_body: &'static str,
) -> tokio::task::JoinHandle<Result<Value, std::io::Error>> {
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;

        let (mut stream, _) = listener.accept().await?;
        let request = read_full_request(&mut stream).await?;
        let status_text = if status == 200 { "OK" } else { "Error" };
        let response = format!(
            "HTTP/1.1 {status} {status_text}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(response.as_bytes()).await?;
        let body = request
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .unwrap_or("");
        Ok(serde_json::from_str(body).unwrap_or(Value::Null))
    })
}

fn capture_raw_request_and_respond(
    listener: tokio::net::TcpListener,
    status: u16,
    response_body: &'static str,
) -> tokio::task::JoinHandle<Result<String, std::io::Error>> {
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;

        let (mut stream, _) = listener.accept().await?;
        let request = read_full_request(&mut stream).await?;
        let status_text = if status == 200 { "OK" } else { "Error" };
        let response = format!(
            "HTTP/1.1 {status} {status_text}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(response.as_bytes()).await?;
        Ok(request)
    })
}

async fn read_full_request(stream: &mut tokio::net::TcpStream) -> std::io::Result<String> {
    use tokio::io::AsyncReadExt;

    let mut buf = Vec::new();
    let mut chunk = [0_u8; 2048];
    loop {
        let bytes_read = stream.read(&mut chunk).await?;
        if bytes_read == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..bytes_read]);
        let text = String::from_utf8_lossy(&buf);
        if let Some(header_end) = text.find("\r\n\r\n") {
            let content_length = text
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    if name.eq_ignore_ascii_case("content-length") {
                        value.trim().parse::<usize>().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            if buf.len() >= header_end + 4 + content_length {
                break;
            }
        }
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

#[tokio::test]
async fn atlas_rejects_legacy_capability_id_from_pre_rename_runs() {
    // Pre-GH145 plans carry `image_generate`; retries fail explicitly and
    // never silently swap to `text_to_image` (product invariant 2).
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        DEFAULT_ATLAS_API_BASE.to_owned(),
    ));

    let err = provider
        .invoke(request(
            "image_generate",
            json!({ "prompt": "hello", "aspect_ratio": "1:1" }),
        ))
        .await
        .expect_err("legacy capability id");

    assert_eq!(
        err,
        ProviderError::UnsupportedCapability("image_generate".to_owned())
    );
}

#[tokio::test]
async fn atlas_video_dispatch_returns_a_durable_handle_before_polling()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = capture_request_and_respond(listener, 200, r#"{"data":{"id":"prediction_123"}}"#);
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        format!("http://{addr}/v1"),
    ));

    let dispatch = provider
        .dispatch(request(
            "text_to_video",
            json!({ "prompt": "hello", "duration_sec": 5 }),
        ))
        .await
        .expect("dispatch");
    let request_body = server.await??;
    let ProviderDispatch::Accepted(task) = dispatch else {
        panic!("video dispatch must return a durable handle");
    };
    assert_eq!(task.provider_task_id, "prediction_123");
    assert_eq!(task.dispatch_origin, format!("http://{addr}"));
    assert_eq!(
        task.status_url.as_deref(),
        Some(format!("http://{addr}/api/v1/model/prediction/prediction_123").as_str())
    );
    assert!(task.result_url.is_none());
    assert_eq!(request_body["resolution"], "720P");
    Ok(())
}

#[tokio::test]
async fn atlas_resume_polls_a_persisted_handle_to_completion()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let status_url = format!("http://{addr}/api/v1/model/prediction/prediction_123");
    let server = capture_request_and_respond(
        listener,
        200,
        r#"{"data":{"status":"completed","outputs":["https://cdn.example/video.mp4"]}}"#,
    );
    let provider = AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        format!("http://{addr}/v1"),
    ));
    let task = DurableProviderTask {
        provider: "atlas".to_owned(),
        provider_task_id: "prediction_123".to_owned(),
        dispatch_origin: provider.dispatch_origin("atlas")?,
        recovery_scope_fingerprint: provider.recovery_scope_fingerprint("atlas"),
        status_url: Some(status_url),
        result_url: None,
    };

    let resumed = provider
        .resume(
            &task,
            &request(
                "text_to_video",
                json!({ "prompt": "hello", "duration_sec": 5 }),
            ),
        )
        .await
        .expect("resume");
    server.await??;
    let ProviderResume::Completed(result) = resumed else {
        panic!("completed response must finish recovery");
    };
    let video = result.outputs.get("video").expect("video");
    assert_eq!(video.duration_ms, Some(5_000));
    assert!(video.meta.get("prediction_id").is_none());
    Ok(())
}

#[tokio::test]
async fn atlas_resume_rejects_scope_drift_before_network_access() {
    let first = AtlasProvider::new(ApiProviderConfig::atlas(
        "key-a".to_owned(),
        "https://api.atlascloud.ai/v1".to_owned(),
    ));
    let second = AtlasProvider::new(ApiProviderConfig::atlas(
        "key-b".to_owned(),
        "https://api.atlascloud.ai/v1".to_owned(),
    ));
    let task = DurableProviderTask {
        provider: "atlas".to_owned(),
        provider_task_id: "prediction_123".to_owned(),
        dispatch_origin: first.dispatch_origin("atlas").expect("origin"),
        recovery_scope_fingerprint: first.recovery_scope_fingerprint("atlas"),
        status_url: Some(
            "https://api.atlascloud.ai/api/v1/model/prediction/prediction_123".to_owned(),
        ),
        result_url: None,
    };
    let err = second
        .resume(
            &task,
            &request("text_to_video", json!({ "prompt": "hello" })),
        )
        .await
        .expect_err("scope drift must fail");
    assert!(matches!(err, ProviderError::InvalidRequest(_)));
    assert_eq!(
        second.recovery_capabilities("atlas"),
        ProviderRecoveryCapabilities {
            resume: true,
            cancel: false
        }
    );
}
