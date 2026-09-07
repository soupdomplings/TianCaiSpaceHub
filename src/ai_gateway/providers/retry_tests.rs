use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    body::{Bytes, to_bytes},
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};

use super::{anthropic_messages, execute_provider_request, openai_responses};
use crate::ai_gateway::{
    config::{ProviderConfig, ProviderType},
    context::GatewayContext,
    error::GatewayError,
    workbuddy,
};

#[derive(Clone, Debug, PartialEq)]
struct CapturedRequest {
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
}

struct UpstreamState {
    statuses: Vec<StatusCode>,
    requests: Mutex<Vec<CapturedRequest>>,
}

struct TestUpstream {
    base_url: String,
    state: Arc<UpstreamState>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for TestUpstream {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl TestUpstream {
    async fn start(statuses: Vec<StatusCode>) -> Self {
        let state = Arc::new(UpstreamState {
            statuses,
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/v1/responses", post(upstream_reply))
            .route("/v1/chat/completions", post(upstream_reply))
            .route("/v1/messages", post(upstream_reply))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            base_url,
            state,
            server,
        }
    }

    fn requests(&self) -> Vec<CapturedRequest> {
        self.state.requests.lock().unwrap().clone()
    }

    fn provider(&self, name: &str, provider_type: ProviderType) -> ProviderConfig {
        ProviderConfig {
            name: name.to_string(),
            base_url: self.base_url.clone(),
            provider_type,
            api_key: "test-key".to_string(),
            timeout_secs: 5,
            ..ProviderConfig::default()
        }
    }
}

async fn upstream_reply(
    State(state): State<Arc<UpstreamState>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let index = {
        let mut requests = state.requests.lock().unwrap();
        let index = requests.len();
        requests.push(CapturedRequest {
            uri: uri.clone(),
            headers,
            body: body.clone(),
        });
        index
    };
    let status = state.statuses[index.min(state.statuses.len() - 1)];
    if !status.is_success() {
        return (
            status,
            Json(json!({"error": {
                "message": format!("upstream failure {}", index + 1),
                "type": "api_error",
            }})),
        )
            .into_response();
    }
    let body: Value = serde_json::from_slice(&body).unwrap();
    if body["stream"] == true {
        let sse = if uri.path() == "/v1/chat/completions" {
            "data: {\"choices\":[{\"delta\":{\"content\":\"retry-recovered\"},\"index\":0}]}\n\ndata: [DONE]\n\n".to_string()
        } else if uri.path() == "/v1/messages" {
            [
                json!({"type":"message_start","message":{"id":"msg_test","type":"message","role":"assistant","model":"test-model","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"retry-recovered"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
                json!({"type":"message_stop"}),
            ].iter().map(|event| format!("event: {}\ndata: {event}\n\n", event["type"].as_str().unwrap())).collect()
        } else {
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"retry-recovered\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_test\",\"status\":\"completed\",\"output\":[]}}\n\n".to_string()
        };
        return ([("content-type", "text/event-stream")], sse).into_response();
    }
    if uri.path() == "/v1/messages" {
        Json(json!({
            "id":"msg_test", "type":"message", "role":"assistant", "model":"test-model",
            "content":[{"type":"text","text":"retry-recovered"}], "stop_reason":"end_turn",
            "usage":{"input_tokens":1,"output_tokens":1},
        }))
        .into_response()
    } else if uri.path() == "/v1/chat/completions" {
        Json(json!({"id":"chat_test","choices":[{"message":{"role":"assistant","content":"retry-recovered"},"index":0,"finish_reason":"stop"}]})).into_response()
    } else {
        Json(json!({"id":"resp_test","object":"response","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"retry-recovered"}]}]})).into_response()
    }
}

async fn forward(provider: &ProviderConfig, stream: bool) -> Result<Response, GatewayError> {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let ctx = GatewayContext::extract(&HeaderMap::new(), Some("workbuddy:test-session"));
    let chat = json!({
        "model":"test-model", "stream":stream,
        "messages":[{"role":"user","content":"test request"}],
        "prompt_cache_key":"workbuddy:test-session",
    });
    match provider.provider_type {
        ProviderType::ChatCompletions => {
            workbuddy::proxy_chat_completion(
                &client,
                &ctx,
                chat,
                "test-model",
                "test-model",
                provider,
                None,
            )
            .await
        }
        ProviderType::AnthropicMessages => {
            let request =
                serde_json::from_value(workbuddy::chat_request_to_responses(&chat).unwrap())
                    .unwrap();
            anthropic_messages::handle(&client, &ctx, &request, "test-model", provider, None).await
        }
        _ => {
            openai_responses::passthrough(
                &client,
                &ctx,
                workbuddy::chat_request_to_openai_responses(&chat).unwrap(),
                "test-model",
                provider,
                None,
            )
            .await
        }
    }
}

async fn assert_workbuddy_recovers(provider_type: ProviderType) {
    for stream in [false, true] {
        let upstream = TestUpstream::start(vec![
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::OK,
        ])
        .await;
        let response = forward(
            &upstream.provider("workbuddy", provider_type.clone()),
            stream,
        )
        .await
        .expect("WorkBuddy must recover from HTTP 502/503");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("retry-recovered"));
        let requests = upstream.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0], requests[1]);
        assert_eq!(requests[1], requests[2]);
    }
}

#[tokio::test]
async fn workbuddy_responses_retries_502_and_503() {
    assert_workbuddy_recovers(ProviderType::OpenAiResponses).await;
}

#[tokio::test]
async fn workbuddy_chat_retries_502_and_503() {
    assert_workbuddy_recovers(ProviderType::ChatCompletions).await;
}

#[tokio::test]
async fn workbuddy_anthropic_retries_502_and_503() {
    assert_workbuddy_recovers(ProviderType::AnthropicMessages).await;
}

#[tokio::test]
async fn workbuddy_http_retries_stop_at_limit_and_preserve_last_error() {
    let upstream = TestUpstream::start(vec![
        StatusCode::BAD_GATEWAY,
        StatusCode::SERVICE_UNAVAILABLE,
    ])
    .await;
    let error = forward(
        &upstream.provider("workbuddy", ProviderType::OpenAiResponses),
        true,
    )
    .await
    .unwrap_err();
    assert_eq!(upstream.requests().len(), 3);
    assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(error.upstream_status, Some(503));
    assert_eq!(error.upstream_error_type.as_deref(), Some("api_error"));
    assert_eq!(error.message, "upstream failure 3");
}

#[tokio::test]
async fn workbuddy_does_not_retry_other_http_errors() {
    for code in [400, 401, 403, 404, 408, 422, 429, 500, 504] {
        let status = StatusCode::from_u16(code).unwrap();
        let upstream = TestUpstream::start(vec![status, StatusCode::OK]).await;
        let error = forward(
            &upstream.provider("workbuddy", ProviderType::OpenAiResponses),
            true,
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, status);
        assert_eq!(upstream.requests().len(), 1);
    }
}

#[tokio::test]
async fn codex_http_error_behavior_is_unchanged() {
    for status in [StatusCode::BAD_GATEWAY, StatusCode::SERVICE_UNAVAILABLE] {
        let upstream = TestUpstream::start(vec![status, StatusCode::OK]).await;
        let error = forward(
            &upstream.provider("openai", ProviderType::OpenAiResponses),
            true,
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, status);
        assert_eq!(upstream.requests().len(), 1);
    }
}

#[tokio::test]
async fn workbuddy_success_is_sent_once() {
    for stream in [false, true] {
        let upstream = TestUpstream::start(vec![StatusCode::OK]).await;
        let response = forward(
            &upstream.provider("workbuddy", ProviderType::OpenAiResponses),
            stream,
        )
        .await
        .unwrap();
        to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(upstream.requests().len(), 1);
    }
}

#[tokio::test]
async fn workbuddy_does_not_replay_non_cloneable_request_bodies() {
    let upstream = TestUpstream::start(vec![StatusCode::SERVICE_UNAVAILABLE, StatusCode::OK]).await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let body = reqwest::Body::wrap_stream(futures_util::stream::iter([Ok::<_, std::io::Error>(
        Bytes::from_static(b"{}"),
    )]));
    let request = client
        .post(format!("{}/responses", upstream.base_url))
        .body(body)
        .build()
        .unwrap();
    assert!(request.try_clone().is_none());
    let response = execute_provider_request(
        &client,
        request,
        &upstream.provider("workbuddy", ProviderType::OpenAiResponses),
        "test upstream",
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("upstream failure 1")
    );
    assert_eq!(upstream.requests().len(), 1);
}

#[tokio::test]
async fn workbuddy_does_not_replay_a_successful_stream_that_disconnects() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/stream", listener.local_addr().unwrap());
    let (disconnect, disconnected) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        socket.read(&mut buffer).await.unwrap();
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000\r\n\r\ndata: partial\n\n").await.unwrap();
        disconnected.await.unwrap();
        drop(socket);
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let request = client.get(url).build().unwrap();
    let provider = ProviderConfig {
        name: "workbuddy".to_string(),
        timeout_secs: 5,
        ..ProviderConfig::default()
    };
    let response = execute_provider_request(&client, request, &provider, "test upstream")
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    disconnect.send(()).unwrap();
    assert!(response.bytes().await.is_err());
    server.await.unwrap();
}

#[tokio::test]
async fn workbuddy_transport_and_http_errors_share_retry_budget() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/test", listener.local_addr().unwrap());
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured_attempts = attempts.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            socket.read(&mut buffer).await.unwrap();
            let index = captured_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if index > 0 {
                socket.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbusy").await.unwrap();
            }
        }
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let request = client.post(url).build().unwrap();
    let provider = ProviderConfig {
        name: "workbuddy".to_string(),
        timeout_secs: 5,
        ..ProviderConfig::default()
    };
    let response = execute_provider_request(&client, request, &provider, "test upstream").await;
    server.abort();
    let _ = server.await;
    let response = response.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.text().await.unwrap(), "busy");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
}
