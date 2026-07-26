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

fn request(capability: &str, params: Value) -> ProviderRequest {
    let operation_id = match capability {
        "prompt_writer" => "deepseek-ai/DeepSeek-V3-0324",
        "text_to_image" => "google/nano-banana-2/text-to-image",
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
