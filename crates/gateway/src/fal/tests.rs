use super::*;
use crate::ProviderDispatchFailureKind;

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

    assert!(catalog.capabilities.contains_key("text_to_image"));
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
        .invoke(request("text_to_image"))
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
        .invoke(request("text_to_image"))
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
        .invoke(request("text_to_image"))
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
        .invoke(request("text_to_image"))
        .await
        .expect_err("status failure");
    server.await??;
    let message = err.to_string().to_lowercase();

    assert!(message.contains("provider_remote_failed"));
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
        .invoke(request("text_to_image"))
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
        .invoke(unresolved_request("text_to_image"))
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
        .invoke(request("text_to_image"))
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

#[tokio::test]
async fn fal_rejects_legacy_capability_id_from_pre_rename_runs() {
    // Runs created before the GH145 rename persist `image_generate` in their
    // plan_json; retries must fail explicitly instead of silently executing
    // under the canonical capability (product invariant 2).
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        DEFAULT_FAL_API_BASE.to_owned(),
    ));

    let err = provider
        .invoke(request("image_generate"))
        .await
        .expect_err("legacy capability id");

    assert_eq!(
        err,
        ProviderError::UnsupportedCapability("image_generate".to_owned())
    );
}

#[tokio::test]
async fn fal_dispatch_and_resume_use_durable_validated_locators()
-> Result<(), Box<dyn std::error::Error>> {
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
            r#"{"images":[{"url":"https://cdn.example/fal.png","width":512,"height":512,"content_type":"image/png"}]}"#,
            addr,
        )
        .await?;
        Ok::<(), std::io::Error>(())
    });
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        format!("http://{addr}"),
    ));
    let request = request("text_to_image");
    let dispatch = provider.dispatch(request.clone()).await.expect("dispatch");
    let ProviderDispatch::Accepted(task) = dispatch else {
        panic!("fal queue dispatch must return a durable handle");
    };
    assert_eq!(task.provider_task_id, "req_1");
    assert_eq!(task.dispatch_origin, format!("http://{addr}"));

    let resumed = provider.resume(&task, &request).await.expect("resume");
    server.await??;
    let ProviderResume::Completed(result) = resumed else {
        panic!("completed status must materialize the result");
    };
    let image = result.outputs.get("image").expect("image");
    assert_eq!(image.width, Some(512));
    assert!(image.meta.get("request_id").is_none());
    Ok(())
}

#[test]
fn fal_callback_validation_rejects_credential_fragment_query_and_prefix_hosts() {
    let base = "https://queue.fal.run";
    for malicious in [
        "https://user@queue.fal.run/status",
        "https://queue.fal.run/status#secret",
        "https://queue.fal.run/status?token=secret",
        "https://queue.fal.run.evil.example/status",
        "https://queue.fal.run@evil.example/status",
    ] {
        assert!(
            validate_callback_url(malicious.to_owned(), base).is_err(),
            "accepted malicious callback {malicious}"
        );
    }
    assert_eq!(
        validate_callback_url(
            "https://queue.fal.run/model/requests/req/status".to_owned(),
            base
        )
        .expect("valid callback"),
        "https://queue.fal.run/model/requests/req/status"
    );
}

#[tokio::test]
async fn fal_resume_rejects_credential_scope_drift_before_network_access() {
    let first = FalProvider::new(FalProviderConfig::new(
        "key-a".to_owned(),
        "https://queue.fal.run".to_owned(),
    ));
    let second = FalProvider::new(FalProviderConfig::new(
        "key-b".to_owned(),
        "https://queue.fal.run".to_owned(),
    ));
    let task = DurableProviderTask {
        provider: "fal".to_owned(),
        provider_task_id: "req_1".to_owned(),
        dispatch_origin: first.dispatch_origin("fal").expect("origin"),
        recovery_scope_fingerprint: first.recovery_scope_fingerprint("fal"),
        status_url: Some("https://queue.fal.run/model/requests/req_1/status".to_owned()),
        result_url: Some("https://queue.fal.run/model/requests/req_1/response".to_owned()),
    };
    let err = second
        .resume(&task, &request("text_to_image"))
        .await
        .expect_err("scope drift must fail");
    assert!(matches!(err, ProviderError::InvalidRequest(_)));
    assert_eq!(
        second.recovery_capabilities("fal"),
        ProviderRecoveryCapabilities {
            resume: true,
            cancel: true
        }
    );
}

#[tokio::test]
async fn fal_durable_cancel_uses_validated_status_url() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        serve_response_capturing(&listener, r#"{"status":"CANCELLED"}"#, addr).await
    });
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        format!("http://{addr}"),
    ));
    let task = DurableProviderTask {
        provider: "fal".to_owned(),
        provider_task_id: "req_1".to_owned(),
        dispatch_origin: provider.dispatch_origin("fal")?,
        recovery_scope_fingerprint: provider.recovery_scope_fingerprint("fal"),
        status_url: Some(format!("http://{addr}/requests/req_1/status")),
        result_url: Some(format!("http://{addr}/requests/req_1/response")),
    };
    provider
        .cancel_durable(&task)
        .await
        .expect("durable cancel");
    let request_head = server.await??;
    assert!(
        request_head.starts_with("PUT /requests/req_1/cancel "),
        "request line was: {}",
        request_head.lines().next().unwrap_or("")
    );
    Ok(())
}

#[tokio::test]
async fn fal_dispatch_failures_are_typed_without_silent_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let provider = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        DEFAULT_FAL_API_BASE.to_owned(),
    ));
    let not_submitted = provider
        .dispatch(unresolved_request("text_to_image"))
        .await
        .expect_err("missing operation");
    assert_eq!(
        not_submitted.kind,
        ProviderDispatchFailureKind::NotSubmitted
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        serve_status_response(&listener, 400, r#"{"detail":"request rejected"}"#, addr).await
    });
    let rejecting = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        format!("http://{addr}"),
    ));
    let rejected = rejecting
        .dispatch(request("text_to_image"))
        .await
        .expect_err("HTTP rejection");
    server.await??;
    assert_eq!(rejected.kind, ProviderDispatchFailureKind::Rejected);

    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let closed_addr = closed.local_addr()?;
    drop(closed);
    let unreachable = FalProvider::new(FalProviderConfig::new(
        "test-key".to_owned(),
        format!("http://{closed_addr}"),
    ));
    let unknown = unreachable
        .dispatch(request("text_to_image"))
        .await
        .expect_err("transport outcome unknown");
    assert_eq!(unknown.kind, ProviderDispatchFailureKind::OutcomeUnknown);
    Ok(())
}
