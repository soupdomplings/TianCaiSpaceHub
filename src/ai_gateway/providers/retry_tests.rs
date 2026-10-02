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

#[tokio::test]
async fn workbuddy_model_settings_forward_efforts_and_protocol_specific_cache_controls() {
    for provider_type in [
        ProviderType::AnthropicMessages,
        ProviderType::OpenAiResponses,
        ProviderType::ChatCompletions,
    ] {
        let efforts = if provider_type == ProviderType::AnthropicMessages {
            vec!["low", "medium", "high", "xhigh", "max"]
        } else {
            vec!["high", "xhigh"]
        };
        for stream in [false, true] {
            for effort in &efforts {
                for key in [
                    None,
                    Some(Value::Null),
                    Some(json!("")),
                    Some(json!(" \t ")),
                    Some(json!("explicit-session")),
                ] {
                    let upstream = TestUpstream::start(vec![StatusCode::OK]).await;
                    let mut provider = upstream.provider("workbuddy", provider_type.clone());
                    provider.models = vec!["workbuddy-settings-test-model".into()];
                    provider.model_aliases.insert(
                        "workbuddy-settings-test-alias".into(),
                        "workbuddy-settings-test-model".into(),
                    );
                    provider.prompt_cache_retention = Some("24h".into());
                    let directory = tempfile::tempdir().unwrap();
                    let mut config = crate::config::AppConfig::default();
                    config.state_path = directory.path().join("state.json");
                    config.ai_gateway.enabled = true;
                    config.ai_gateway.providers = vec![provider];
                    let state = crate::app_state::AppState::new(
                        directory.path().join("config.toml"),
                        config,
                        None,
                        None,
                    );
                    let mut request = json!({
                        "model":"workbuddy-settings-test-alias", "stream":stream, "reasoning_effort":effort,
                        "messages":[{"role":"system","content":"Be helpful"},{"role":"user","content":"hello"}],
                        "tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}],
                    });
                    let explicit = key
                        .as_ref()
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.trim().is_empty());
                    if let Some(key) = key {
                        request["prompt_cache_key"] = key;
                    }
                    // Exercise the header fallback as well as the generated model fallback.
                    let mut headers = HeaderMap::new();
                    if stream {
                        headers.insert("x-workbuddy-session-id", "header-session".parse().unwrap());
                    }
                    let response = crate::ai_gateway::handler::handle_workbuddy_chat_completions(
                        State(state),
                        headers,
                        Bytes::from(serde_json::to_vec(&request).unwrap()),
                    )
                    .await;
                    assert_eq!(
                        response.status(),
                        StatusCode::OK,
                        "{provider_type:?}, {effort}, {key:?}",
                        key = request.get("prompt_cache_key")
                    );
                    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
                    let output = String::from_utf8(bytes.to_vec()).unwrap();
                    assert!(output.contains("retry-recovered"));
                    if stream {
                        assert!(output.contains("[DONE]"));
                    }
                    let captured = upstream.requests();
                    assert_eq!(captured.len(), 1);
                    let body: Value = serde_json::from_slice(&captured[0].body).unwrap();
                    assert_eq!(body["model"], "workbuddy-settings-test-model");
                    if provider_type == ProviderType::AnthropicMessages {
                        assert_eq!(captured[0].uri.path(), "/v1/messages");
                        assert_eq!(body["thinking"]["type"], "adaptive");
                        assert_eq!(body["output_config"]["effort"], *effort);
                        assert!(body.get("reasoning_effort").is_none());
                        assert!(body.get("prompt_cache_key").is_none());
                        assert!(body.get("prompt_cache_retention").is_none());
                        assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");
                        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
                        assert_eq!(
                            body["messages"][0]["content"][0]["cache_control"]["type"],
                            "ephemeral"
                        );
                    } else {
                        let expected_key = if explicit {
                            "explicit-session"
                        } else if stream {
                            "header-session"
                        } else {
                            "workbuddy:workbuddy-settings-test-alias"
                        };
                        assert_eq!(body["prompt_cache_key"], expected_key);
                        assert_eq!(body["prompt_cache_retention"], "24h");
                        if provider_type == ProviderType::OpenAiResponses {
                            assert_eq!(body["reasoning"]["effort"], *effort);
                        } else {
                            assert_eq!(body["reasoning_effort"], *effort);
                        }
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn workbuddy_kimi_handler_preserves_mapping_reasoning_and_http_retries() {
    for stream in [false, true] {
        let upstream =
            TestUpstream::start(vec![StatusCode::SERVICE_UNAVAILABLE, StatusCode::OK]).await;
        let mut provider = upstream.provider("workbuddy", ProviderType::KimiResponses);
        provider.models = vec!["k3".into()];
        provider.model_aliases.insert("kimi-k3".into(), "k3".into());
        // An old Chat-specific setting must not affect the Kimi channel.
        provider.chat_disable_reasoning = true;
        let directory = tempfile::tempdir().unwrap();
        let mut config = crate::config::AppConfig::default();
        config.state_path = directory.path().join("state.json");
        config.ai_gateway.enabled = true;
        config.ai_gateway.providers = vec![provider];
        let state = crate::app_state::AppState::new(
            directory.path().join("config.toml"),
            config,
            None,
            None,
        );
        let request = json!({
            "model":"kimi-k3", "stream":stream, "reasoning_effort":"high",
            "messages":[{"role":"user","content":"hello"}],
            "tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}],
        });
        let response = crate::ai_gateway::handler::handle_workbuddy_chat_completions(
            State(state),
            HeaderMap::new(),
            Bytes::from(serde_json::to_vec(&request).unwrap()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let output = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(output.contains("retry-recovered"));
        if stream {
            assert!(output.contains("[DONE]"));
        } else {
            let output: Value = serde_json::from_str(&output).unwrap();
            assert_eq!(output["object"], "chat.completion");
            assert_eq!(output["model"], "kimi-k3");
        }
        let captured = upstream.requests();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].body, captured[1].body);
        assert_eq!(captured[0].uri.path(), "/v1/responses");
        let body: Value = serde_json::from_slice(&captured[0].body).unwrap();
        assert_eq!(body["model"], "k3");
        assert_eq!(body["reasoning"]["effort"], "high");
        assert_eq!(body["tools"][0]["name"], "lookup");
        assert!(body.get("prompt_cache_key").is_none());
        assert!(body.get("prompt_cache_retention").is_none());
    }
}

#[tokio::test]
async fn openai_chat_converts_responses_and_uses_standard_parameters() {
    for (stream, disable_reasoning) in [(false, false), (true, false), (false, true), (true, true)]
    {
        let upstream = TestUpstream::start(vec![StatusCode::OK]).await;
        let mut provider = upstream.provider("openai-chat", ProviderType::ChatCompletions);
        provider.compatibility = Some("openai_chat".to_string());
        provider.chat_disable_reasoning = disable_reasoning;
        let request = serde_json::from_value(json!({
            "model":"upstream-model", "stream":stream,
            "instructions":"Be helpful",
            "input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}],
            "reasoning":{"effort":"high"}, "max_output_tokens":512,
            "tools":[{"type":"function","name":"lookup","parameters":{"type":"object","properties":{}}}],
        })).unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let context = GatewayContext::extract(&HeaderMap::new(), Some("chat-session"));
        let response = super::deepseek_chat::handle(
            &client,
            &context,
            &request,
            "visible-model",
            &provider,
            None,
        )
        .await
        .unwrap();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let output = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(output.contains("retry-recovered"));
        if stream {
            assert!(output.contains("response.completed"));
        } else {
            let output: Value = serde_json::from_str(&output).unwrap();
            assert_eq!(output["object"], "response");
            assert_eq!(output["model"], "visible-model");
        }
        let captured = upstream.requests();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].uri.path(), "/v1/chat/completions");
        assert_eq!(captured[0].headers["authorization"], "Bearer test-key");
        let body: Value = serde_json::from_slice(&captured[0].body).unwrap();
        assert_eq!(body["model"], "upstream-model");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(
            body["reasoning_effort"],
            if disable_reasoning { "none" } else { "high" }
        );
        assert_eq!(body["max_completion_tokens"], 512);
        assert_eq!(body["prompt_cache_key"], "chat-session");
        assert_eq!(body["tools"][0]["function"]["name"], "lookup");
        assert!(body.get("thinking").is_none());
        assert!(body.get("max_tokens").is_none());
    }
}

#[tokio::test]
async fn legacy_chat_channel_keeps_deepseek_parameters() {
    let upstream = TestUpstream::start(vec![StatusCode::OK]).await;
    let provider = upstream.provider("legacy", ProviderType::ChatCompletions);
    let request = serde_json::from_value(json!({
        "model":"test-model", "input":[], "reasoning":{"effort":"high"}, "max_output_tokens":512,
    }))
    .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let context = GatewayContext::extract(&HeaderMap::new(), None);
    super::deepseek_chat::handle(&client, &context, &request, "test-model", &provider, None)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&upstream.requests()[0].body).unwrap();
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["max_tokens"], 512);
    assert!(body.get("max_completion_tokens").is_none());
}
use crate::ai_gateway::{
    config::{ProviderConfig, ProviderType},
    context::GatewayContext,
    error::GatewayError,
    workbuddy,
};

#[test]
fn chat_reasoning_setting_defaults_off_and_is_channel_scoped() {
    let old: ProviderConfig = serde_json::from_value(json!({"name":"old"})).unwrap();
    assert!(!old.chat_disable_reasoning);
    for provider_type in [
        ProviderType::ChatCompletions,
        ProviderType::OpenAiResponses,
        ProviderType::ChatGptResponses,
        ProviderType::AnthropicMessages,
        ProviderType::DeepSeekResponses,
        ProviderType::KimiResponses,
        ProviderType::GrokResponses,
    ] {
        let provider = ProviderConfig {
            provider_type: provider_type.clone(),
            chat_disable_reasoning: true,
            ..Default::default()
        };
        let saved = serde_json::to_value(&provider).unwrap();
        let restored: ProviderConfig = serde_json::from_value(saved).unwrap();
        assert!(restored.chat_disable_reasoning);
        let original = json!({"reasoning_effort":"high", "thinking":{"type":"enabled"}, "reasoning":{"effort":"high"}, "tools":[{"type":"function"}]});
        let mut body = original.clone();
        super::apply_chat_reasoning_override(&mut body, &restored);
        if provider_type == ProviderType::ChatCompletions {
            assert_eq!(body["reasoning_effort"], "none");
            assert!(body.get("thinking").is_none());
            assert!(body.get("reasoning").is_none());
            assert_eq!(body["tools"], original["tools"]);
        } else {
            assert_eq!(body, original);
        }
    }
}

#[tokio::test]
async fn workbuddy_chat_reasoning_override_keeps_tools() {
    for disabled in [false, true] {
        let upstream = TestUpstream::start(vec![StatusCode::OK]).await;
        let mut provider = upstream.provider("workbuddy", ProviderType::ChatCompletions);
        provider.chat_disable_reasoning = disabled;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let context = GatewayContext::extract(&HeaderMap::new(), None);
        let tools = json!([{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}]);
        let body = json!({"model":"test", "messages":[{"role":"user","content":"hello"}], "reasoning_effort":"high", "tools":tools});
        let response = workbuddy::proxy_chat_completion(
            &client, &context, body, "test", "test", &provider, None,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let captured: Value = serde_json::from_slice(&upstream.requests()[0].body).unwrap();
        assert_eq!(
            captured["reasoning_effort"],
            if disabled { "none" } else { "high" }
        );
        assert_eq!(captured["tools"], tools);
    }
}

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
    let cache_key = format!("{}:test-session", provider.name);
    let ctx = GatewayContext::extract(&HeaderMap::new(), Some(&cache_key));
    let chat = json!({
        "model":"test-model", "stream":stream,
        "messages":[{"role":"user","content":"test request"}],
        "prompt_cache_key":cache_key,
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

async fn assert_client_recovers(client_name: &str, provider_type: ProviderType) {
    for stream in [false, true] {
        let upstream = TestUpstream::start(vec![
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::OK,
        ])
        .await;
        let response = forward(
            &upstream.provider(client_name, provider_type.clone()),
            stream,
        )
        .await
        .expect("client must recover from HTTP 502/503");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("retry-recovered"));
        let requests = upstream.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0], requests[1]);
        assert_eq!(requests[1], requests[2]);
        let expected_path = match provider_type {
            ProviderType::ChatCompletions => "/v1/chat/completions",
            ProviderType::AnthropicMessages => "/v1/messages",
            _ => "/v1/responses",
        };
        assert_eq!(requests[0].uri.path(), expected_path);
        if matches!(
            provider_type,
            ProviderType::OpenAiResponses | ProviderType::ChatCompletions
        ) {
            let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
            assert_eq!(
                body["prompt_cache_key"],
                format!("{client_name}:test-session")
            );
        }
    }
}

#[tokio::test]
async fn workbuddy_responses_retries_502_and_503() {
    assert_client_recovers("workbuddy", ProviderType::OpenAiResponses).await;
}

#[tokio::test]
async fn workbuddy_chat_retries_502_and_503() {
    assert_client_recovers("workbuddy", ProviderType::ChatCompletions).await;
}

#[tokio::test]
async fn workbuddy_anthropic_retries_502_and_503() {
    assert_client_recovers("workbuddy", ProviderType::AnthropicMessages).await;
}

#[tokio::test]
async fn gmclaw_protocols_retry_502_and_503_without_changing_request() {
    for provider_type in [
        ProviderType::OpenAiResponses,
        ProviderType::KimiResponses,
        ProviderType::ChatCompletions,
        ProviderType::AnthropicMessages,
    ] {
        assert_client_recovers("gmclaw", provider_type).await;
    }
}

#[tokio::test]
async fn workbuddy_and_gmclaw_http_retries_stop_at_limit_and_preserve_last_error() {
    for client_name in ["workbuddy", "gmclaw"] {
        assert_http_retries_stop_at_limit_and_preserve_last_error(client_name).await;
    }
}

async fn assert_http_retries_stop_at_limit_and_preserve_last_error(client_name: &str) {
    let upstream = TestUpstream::start(vec![
        StatusCode::BAD_GATEWAY,
        StatusCode::SERVICE_UNAVAILABLE,
    ])
    .await;
    let error = forward(
        &upstream.provider(client_name, ProviderType::OpenAiResponses),
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
async fn workbuddy_and_gmclaw_do_not_retry_other_http_errors() {
    for client_name in ["workbuddy", "gmclaw"] {
        assert_does_not_retry_other_http_errors(client_name).await;
    }
}

async fn assert_does_not_retry_other_http_errors(client_name: &str) {
    for code in [400, 401, 403, 404, 408, 422, 429, 500, 504] {
        let status = StatusCode::from_u16(code).unwrap();
        let upstream = TestUpstream::start(vec![status, StatusCode::OK]).await;
        let error = forward(
            &upstream.provider(client_name, ProviderType::OpenAiResponses),
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
async fn workbuddy_and_gmclaw_success_is_sent_once() {
    for client_name in ["workbuddy", "gmclaw"] {
        assert_success_is_sent_once(client_name).await;
    }
}

async fn assert_success_is_sent_once(client_name: &str) {
    for stream in [false, true] {
        let upstream = TestUpstream::start(vec![StatusCode::OK]).await;
        let response = forward(
            &upstream.provider(client_name, ProviderType::OpenAiResponses),
            stream,
        )
        .await
        .unwrap();
        to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(upstream.requests().len(), 1);
    }
}

#[tokio::test]
async fn workbuddy_and_gmclaw_do_not_replay_non_cloneable_request_bodies() {
    for client_name in ["workbuddy", "gmclaw"] {
        assert_does_not_replay_non_cloneable_request_bodies(client_name).await;
    }
}

async fn assert_does_not_replay_non_cloneable_request_bodies(client_name: &str) {
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
        &upstream.provider(client_name, ProviderType::OpenAiResponses),
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
async fn workbuddy_and_gmclaw_do_not_replay_a_successful_stream_that_disconnects() {
    for client_name in ["workbuddy", "gmclaw"] {
        assert_does_not_replay_a_successful_stream_that_disconnects(client_name).await;
    }
}

async fn assert_does_not_replay_a_successful_stream_that_disconnects(client_name: &str) {
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
        name: client_name.to_string(),
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
async fn workbuddy_and_gmclaw_transport_and_http_errors_share_retry_budget() {
    for client_name in ["workbuddy", "gmclaw"] {
        assert_transport_and_http_errors_share_retry_budget(client_name).await;
    }
}

async fn assert_transport_and_http_errors_share_retry_budget(client_name: &str) {
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
        name: client_name.to_string(),
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
