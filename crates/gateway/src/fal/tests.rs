use super::*;

fn request(capability: &str) -> ProviderRequest {
    ProviderRequest {
        provider: "fal".to_owned(),
        capability: capability.to_owned(),
        node_id: "image_node".to_owned(),
        run_id: "run_1".to_owned(),
        inputs: BTreeMap::new(),
        input_texts: BTreeMap::new(),
        params: json!({ "prompt": "a product image", "aspect_ratio": "1:1" }),
        resolved_model_id: Some("google/nano-banana-2".to_owned()),
        operation_id: Some("fal-ai/nano-banana-2".to_owned()),
    }
}

fn unresolved_request(capability: &str) -> ProviderRequest {
    ProviderRequest {
        resolved_model_id: None,
        operation_id: None,
        ..request(capability)
    }
}

#[test]
fn fal_catalog_exposes_image_generation_only() {
    let catalog = FalProvider::catalog_value();

    assert!(catalog.capabilities.contains_key("image_generate"));
    assert!(!catalog.capabilities.contains_key("text_to_video"));
}

#[tokio::test]
async fn fal_rejects_unsupported_capability() {
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        DEFAULT_FAL_API_BASE.to_owned(),
    ));

    let err = provider
        .invoke(request("text_to_video"))
        .await
        .expect_err("unsupported capability");

    assert_eq!(
        err,
        ProviderError::UnsupportedCapability("text_to_video".to_owned())
    );
}

#[tokio::test]
async fn fal_queue_image_generation_returns_remote_image() -> Result<(), Box<dyn std::error::Error>>
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        serve_response(
            &listener,
            r#"{"request_id":"req_1","status_url":"http://ADDR/status","response_url":"http://ADDR/response"}"#,
            addr,
        )
        .await?;
        serve_response(
            &listener,
            r#"{"status":"COMPLETED","request_id":"req_1"}"#,
            addr,
        )
        .await?;
        serve_response(
            &listener,
            r#"{"images":[{"url":"https://cdn.example/fal.png","width":768,"height":768,"content_type":"image/png"}]}"#,
            addr,
        )
        .await?;
        Ok::<(), std::io::Error>(())
    });
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        format!("http://{addr}"),
    ));

    let result = provider
        .invoke(request("image_generate"))
        .await
        .expect("fal image result");
    server.await??;
    let image = result.outputs.get("image").expect("image output");

    assert_eq!(image.kind, ArtifactKind::Image);
    assert_eq!(image.width, Some(768));
    assert_eq!(image.height, Some(768));
    assert!(matches!(image.content, ArtifactContent::RemoteUrl { .. }));
    assert_eq!(image.meta["provider"], "fal");
    Ok(())
}

#[tokio::test]
async fn fal_errors_redact_api_key() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        serve_status_response(
            &listener,
            401,
            r#"{"detail":"FAL_KEY test-key was rejected"}"#,
            addr,
        )
        .await?;
        Ok::<(), std::io::Error>(())
    });
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        format!("http://{addr}"),
    ));

    let err = provider
        .invoke(request("image_generate"))
        .await
        .expect_err("auth failure");
    server.await??;
    let message = err.to_string().to_lowercase();

    assert!(message.contains("authentication"));
    assert!(!message.contains("test-key"));
    assert!(!message.contains("fal_key"));
    Ok(())
}

#[tokio::test]
async fn fal_rate_limit_errors_are_generic() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        serve_status_response(
            &listener,
            429,
            r#"{"detail":"Key test-key is over quota"}"#,
            addr,
        )
        .await?;
        Ok::<(), std::io::Error>(())
    });
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        format!("http://{addr}"),
    ));

    let err = provider
        .invoke(request("image_generate"))
        .await
        .expect_err("rate limit failure");
    server.await??;
    let message = err.to_string().to_lowercase();

    assert!(message.contains("rate limit"));
    assert!(!message.contains("test-key"));
    assert!(!message.contains("over quota"));
    Ok(())
}

#[tokio::test]
async fn fal_status_errors_redact_api_key() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        serve_response(
            &listener,
            r#"{"request_id":"req_1","status_url":"http://ADDR/status","response_url":"http://ADDR/response"}"#,
            addr,
        )
        .await?;
        serve_response(
            &listener,
            r#"{"status":"FAILED","error":"Bearer test-key failed upstream"}"#,
            addr,
        )
        .await?;
        Ok::<(), std::io::Error>(())
    });
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        format!("http://{addr}"),
    ));

    let err = provider
        .invoke(request("image_generate"))
        .await
        .expect_err("status failure");
    server.await??;
    let message = err.to_string().to_lowercase();

    assert!(message.contains("provider message was redacted"));
    assert!(!message.contains("test-key"));
    assert!(!message.contains("bearer"));
    Ok(())
}

#[tokio::test]
async fn fal_rejects_cross_base_callback_urls() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        serve_response(
            &listener,
            r#"{"request_id":"req_1","status_url":"https://evil.example/status","response_url":"https://evil.example/response"}"#,
            addr,
        )
        .await?;
        Ok::<(), std::io::Error>(())
    });
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        format!("http://{addr}"),
    ));

    let err = provider
        .invoke(request("image_generate"))
        .await
        .expect_err("callback URL validation failure");
    server.await??;

    assert!(
        err.to_string()
            .contains("fal response has unexpected callback URL")
    );
    Ok(())
}

#[tokio::test]
async fn gh130_failclosed_image_without_resolved_operation_errors() {
    // GH130 T4: with no resolved binding the provider refuses to invoke —
    // the old config-default fallback is gone.
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        DEFAULT_FAL_API_BASE.to_owned(),
    ));

    let err = provider
        .invoke(unresolved_request("image_generate"))
        .await
        .expect_err("unresolved model must fail");

    assert!(matches!(err, ProviderError::ModelUnresolved { .. }));
}

#[tokio::test]
async fn gh130_resolved_operation_drives_submit_path() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let submit_head = serve_response_capturing(
            &listener,
            r#"{"request_id":"req_1","status_url":"http://ADDR/status","response_url":"http://ADDR/response"}"#,
            addr,
        )
        .await?;
        serve_response(
            &listener,
            r#"{"status":"COMPLETED","request_id":"req_1"}"#,
            addr,
        )
        .await?;
        serve_response(
            &listener,
            r#"{"images":[{"url":"https://cdn.example/fal.png","width":768,"height":768,"content_type":"image/png"}]}"#,
            addr,
        )
        .await?;
        Ok::<String, std::io::Error>(submit_head)
    });
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        format!("http://{addr}"),
    ));
    let result = provider
        .invoke(request("image_generate"))
        .await
        .expect("fal image result");
    let submit_head = server.await??;

    assert!(
        submit_head.starts_with("POST /fal-ai/nano-banana-2 "),
        "submit line was: {}",
        submit_head.lines().next().unwrap_or("")
    );
    let image = result.outputs.get("image").expect("image output");
    assert_eq!(image.meta["model"], "fal-ai/nano-banana-2");
    Ok(())
}

async fn serve_response(
    listener: &tokio::net::TcpListener,
    body: &str,
    addr: std::net::SocketAddr,
) -> std::io::Result<()> {
    serve_status_response(listener, 200, body, addr).await
}

async fn serve_response_capturing(
    listener: &tokio::net::TcpListener,
    body: &str,
    addr: std::net::SocketAddr,
) -> std::io::Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (mut stream, _) = listener.accept().await?;
    let mut buf = [0_u8; 2048];
    let bytes_read = stream.read(&mut buf).await?;
    assert!(bytes_read > 0);
    let body = body.replace("ADDR", &addr.to_string());
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await?;
    Ok(String::from_utf8_lossy(&buf[..bytes_read]).to_string())
}

async fn serve_status_response(
    listener: &tokio::net::TcpListener,
    status: u16,
    body: &str,
    addr: std::net::SocketAddr,
) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (mut stream, _) = listener.accept().await?;
    let mut buf = [0_u8; 2048];
    let bytes_read = stream.read(&mut buf).await?;
    assert!(bytes_read > 0);
    let body = body.replace("ADDR", &addr.to_string());
    let status_text = if status == 200 { "OK" } else { "Error" };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}
