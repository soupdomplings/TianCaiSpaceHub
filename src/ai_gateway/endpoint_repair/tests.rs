use super::*;
use crate::app_state::SharedState;
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::IntoResponse,
    routing::any,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Captured {
    path: String,
    body: Bytes,
    authorization: String,
}

struct Server {
    url: String,
    captured: Arc<Mutex<Vec<Captured>>>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
struct Replies {
    original: StatusCode,
    alternate: StatusCode,
    body: String,
    content_type: &'static str,
    captured: Arc<Mutex<Vec<Captured>>>,
}

impl Server {
    async fn start(
        original: StatusCode,
        alternate: StatusCode,
        body: &str,
        content_type: &'static str,
    ) -> Self {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let replies = Replies {
            original,
            alternate,
            body: body.into(),
            content_type,
            captured: captured.clone(),
        };
        async fn reply(
            State(state): State<Replies>,
            uri: Uri,
            headers: HeaderMap,
            body: Bytes,
        ) -> impl IntoResponse {
            state.captured.lock().unwrap().push(Captured {
                path: uri.path().into(),
                body,
                authorization: headers
                    .get("authorization")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .into(),
            });
            if state.body == "__timeout__" && uri.path().contains("/v1/") {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }
            let status = if uri.path().contains("/v1/") {
                state.alternate
            } else {
                state.original
            };
            let body = if status.is_success() {
                state.body
            } else if uri.path().contains("/v1/") {
                "alternate-error-must-not-replace-original".into()
            } else {
                "original-error-identity".into()
            };
            (status, [("content-type", state.content_type)], body)
        }
        let app = Router::new().fallback(any(reply)).with_state(replies);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/proxy", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            url,
            captured,
            task,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn fixture(
    server: &Server,
) -> (
    tempfile::TempDir,
    SharedState,
    ProviderConfig,
    GatewayContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let provider = ProviderConfig {
        name: "repair-test".into(),
        base_url: server.url.clone(),
        api_key: "synthetic-key".into(),
        timeout_secs: 2,
        ..Default::default()
    };
    let mut config = AppConfig::default();
    config.state_path = dir.path().join("state.json");
    config.ai_gateway.providers = vec![provider.clone()];
    config.save(&path).unwrap();
    let state = AppState::new(path, config, None, None);
    let context = GatewayContext::extract(&HeaderMap::new(), None).with_state(&state);
    (dir, state, provider, context)
}

async fn forward(
    provider: &ProviderConfig,
    context: &GatewayContext,
    suffix: &str,
) -> reqwest::Response {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let request = client
        .post(provider_endpoint(&provider.base_url, suffix))
        .bearer_auth(&provider.api_key)
        .body("unchanged-synthetic-request")
        .build()
        .unwrap();
    super::super::providers::execute_context_request(
        &client,
        request,
        provider,
        context,
        "synthetic request",
    )
    .await
    .unwrap()
}

#[test]
fn candidate_preserves_prefix_and_skips_explicit_versions_and_special_urls() {
    assert_eq!(
        provider_base_url_with_v1("https://example.invalid/proxy/"),
        Some("https://example.invalid/proxy/v1".into())
    );
    for url in [
        "https://example.invalid/v1",
        "https://example.invalid/v2/",
        "https://example.invalid/v1beta/openai",
        "https://example.invalid/api/v4",
        "https://example.invalid/proxy/v1/responses",
        "https://example.invalid?token=placeholder",
        "https://example.invalid#fragment",
    ] {
        assert!(provider_base_url_with_v1(url).is_none(), "{url}");
    }
    assert_eq!(
        provider_endpoint("https://example.invalid/proxy", "/v1/responses"),
        "https://example.invalid/proxy/responses"
    );
    assert_eq!(
        provider_endpoint("https://example.invalid/proxy/v1/", "/v1/responses"),
        "https://example.invalid/proxy/v1/responses"
    );
    assert_eq!(
        provider_endpoint("https://example.invalid/api/v4?x=1", "/models"),
        "https://example.invalid/api/v4/models?x=1"
    );
}

#[tokio::test]
async fn json_success_corrects_only_after_fallback_and_updates_memory_and_disk() {
    for (suffix, body) in [
        (
            "/responses",
            json!({"id":"response-test","status":"completed","output":[]}),
        ),
        (
            "/chat/completions",
            json!({"id":"chat-test","choices":[{"message":{"content":"hello"},"finish_reason":"stop"}]}),
        ),
        (
            "/messages",
            json!({"id":"message-test","type":"message","content":[],"stop_reason":"end_turn"}),
        ),
    ] {
        let server = Server::start(
            StatusCode::NOT_FOUND,
            StatusCode::OK,
            &body.to_string(),
            "application/json",
        )
        .await;
        let (_dir, state, provider, context) = fixture(&server);
        let response = forward(&provider, &context, suffix).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.json::<Value>().await.unwrap(), body);
        let corrected = format!("{}/v1", server.url);
        assert_eq!(
            AppConfig::load_or_default(&state.config_path)
                .unwrap()
                .ai_gateway
                .providers[0]
                .base_url,
            corrected
        );
        assert_eq!(
            state.config.lock().await.ai_gateway.providers[0].base_url,
            corrected
        );
        let saved = state.config.lock().await.ai_gateway.providers[0].clone();
        assert_eq!(
            saved.base_url_repair_origin.as_deref(),
            Some(provider.base_url.as_str())
        );
        assert_eq!(
            super::super::config::provider_route_id(&saved),
            super::super::config::provider_route_id(&provider)
        );
        assert_eq!(
            super::super::encrypted_content::EncryptedContentScope::for_provider(&saved),
            super::super::encrypted_content::EncryptedContentScope::for_provider(&provider)
        );
        let captured = server.captured.lock().unwrap().clone();
        assert_eq!(
            captured.iter().map(|v| v.path.as_str()).collect::<Vec<_>>(),
            [format!("/proxy{suffix}"), format!("/proxy/v1{suffix}")]
        );
        assert_eq!(captured[0].body, captured[1].body);
        assert_eq!(captured[0].authorization, captured[1].authorization);
    }
}

#[tokio::test]
async fn working_original_is_used_once_and_its_url_is_retained() {
    let server = Server::start(
        StatusCode::OK,
        StatusCode::OK,
        r#"{"output":[]}"#,
        "application/json",
    )
    .await;
    let (_dir, state, provider, context) = fixture(&server);
    forward(&provider, &context, "/responses")
        .await
        .bytes()
        .await
        .unwrap();
    assert_eq!(server.captured.lock().unwrap().len(), 1);
    assert_eq!(
        AppConfig::load_or_default(&state.config_path)
            .unwrap()
            .ai_gateway
            .providers[0]
            .base_url,
        provider.base_url
    );
}

#[tokio::test]
async fn alternate_failures_and_false_success_preserve_original_error_and_config() {
    for (status, body, content_type) in [
        (StatusCode::UNAUTHORIZED, "", "application/json"),
        (StatusCode::OK, "<html>homepage</html>", "text/html"),
        (
            StatusCode::OK,
            r#"{"error":{"message":"rejected"},"output":[]}"#,
            "application/json",
        ),
        (
            StatusCode::OK,
            r#"{"status":"failed","output":[]}"#,
            "application/json",
        ),
        (StatusCode::OK, "{}", "application/json"),
        (
            StatusCode::OK,
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"message\":\"rejected\"}}\n\n",
            "text/event-stream",
        ),
    ] {
        let server = Server::start(StatusCode::NOT_FOUND, status, body, content_type).await;
        let (_dir, state, provider, context) = fixture(&server);
        let original_config = std::fs::read(&state.config_path).unwrap();
        let response = forward(&provider, &context, "/responses").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.text().await.unwrap(), "original-error-identity");
        assert_eq!(std::fs::read(&state.config_path).unwrap(), original_config);
        assert_eq!(server.captured.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn streams_commit_at_valid_terminal_and_preserve_bytes() {
    for (suffix, body) in [
        (
            "/responses",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"r\"}}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n",
        ),
        (
            "/chat/completions",
            "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ),
        (
            "/messages",
            "data: {\"type\":\"message_start\",\"message\":{\"type\":\"message\",\"content\":[]}}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n",
        ),
    ] {
        let server = Server::start(
            StatusCode::NOT_FOUND,
            StatusCode::OK,
            body,
            "text/event-stream",
        )
        .await;
        let (_dir, state, provider, context) = fixture(&server);
        let response = forward(&provider, &context, suffix).await;
        assert_eq!(
            state.config.lock().await.ai_gateway.providers[0].base_url,
            provider.base_url
        );
        assert_eq!(response.text().await.unwrap(), body);
        assert_eq!(
            state.config.lock().await.ai_gateway.providers[0].base_url,
            format!("{}/v1", server.url)
        );
    }
}

#[tokio::test]
async fn incomplete_or_failed_started_stream_is_not_saved_or_replayed() {
    for body in [
        "data: {\"type\":\"response.created\"}\n\n",
        "data: {\"type\":\"response.created\"}\n\ndata: {\"type\":\"response.failed\",\"error\":{}}\n\n",
    ] {
        let server = Server::start(
            StatusCode::NOT_FOUND,
            StatusCode::OK,
            body,
            "text/event-stream",
        )
        .await;
        let (_dir, state, provider, context) = fixture(&server);
        let response = forward(&provider, &context, "/responses").await;
        // A stream that is already known to have failed is rejected before delivery.
        if response.status().is_success() {
            assert_eq!(response.text().await.unwrap(), body);
        } else {
            assert_eq!(response.text().await.unwrap(), "original-error-identity");
        }
        assert_eq!(
            state.config.lock().await.ai_gateway.providers[0].base_url,
            provider.base_url
        );
        assert_eq!(server.captured.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn versioned_urls_do_not_get_another_version_attempt() {
    let server = Server::start(
        StatusCode::NOT_FOUND,
        StatusCode::UNAUTHORIZED,
        "",
        "application/json",
    )
    .await;
    let (_dir, _state, mut provider, context) = fixture(&server);
    provider.base_url.push_str("/v1");
    assert_eq!(
        forward(&provider, &context, "/responses").await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(server.captured.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn alternate_timeout_still_returns_original_status_and_body() {
    let server = Server::start(
        StatusCode::NOT_FOUND,
        StatusCode::OK,
        "__timeout__",
        "application/json",
    )
    .await;
    let (_dir, state, mut provider, context) = fixture(&server);
    provider.timeout_secs = 1;
    let response = forward(&provider, &context, "/responses").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.text().await.unwrap(), "original-error-identity");
    assert_eq!(
        state.config.lock().await.ai_gateway.providers[0].base_url,
        server.url
    );
}

#[tokio::test]
async fn alternate_timeout_keeps_original_error_within_gmclaw_whole_operation_deadline() {
    for responses in [true, false] {
        let server = Server::start(
            StatusCode::NOT_FOUND,
            StatusCode::OK,
            "__timeout__",
            "application/json",
        )
        .await;
        let (_dir, _state, mut provider, context) = fixture(&server);
        provider.name = "gmclaw".into();
        provider.timeout_secs = 1;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let result = if responses {
            super::super::providers::openai_responses::passthrough(
                &client,
                &context,
                json!({"model":"test-model","input":[],"stream":false}),
                "test-model",
                &provider,
                None,
            )
            .await
        } else {
            provider.provider_type = ProviderType::ChatCompletions;
            super::super::workbuddy::proxy_chat_completion(
                &client,
                &context,
                json!({"model":"test-model","messages":[],"stream":false}),
                "test-model",
                "test-model",
                &provider,
                None,
            )
            .await
        };
        let error = result.unwrap_err();
        assert_eq!(error.status, StatusCode::NOT_FOUND);
        assert!(
            error.message.contains("original-error-identity"),
            "{}",
            error.message
        );
        assert_eq!(server.captured.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn non_cloneable_bodies_do_not_attempt_an_alternate_url() {
    let server = Server::start(
        StatusCode::NOT_FOUND,
        StatusCode::OK,
        r#"{"output":[]}"#,
        "application/json",
    )
    .await;
    let (_dir, _state, provider, context) = fixture(&server);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let request = client
        .post(provider_endpoint(&provider.base_url, "/responses"))
        .bearer_auth(&provider.api_key)
        .body(reqwest::Body::wrap_stream(futures_util::stream::iter([
            Ok::<_, std::io::Error>(Bytes::from_static(b"{}")),
        ])))
        .build()
        .unwrap();
    let response = super::super::providers::execute_context_request(
        &client,
        request,
        &provider,
        &context,
        "synthetic request",
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(server.captured.lock().unwrap().len(), 1);
}

#[test]
fn late_success_preserves_edits_deletion_and_unrelated_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = ProviderConfig {
        name: "test".into(),
        base_url: "https://example.invalid".into(),
        ..Default::default()
    };
    let mut config = AppConfig::default();
    config.ai_gateway.providers = vec![original.clone()];
    config.save(&path).unwrap();
    config.bind = "127.0.0.1:4847".into();
    config.save(&path).unwrap();
    let updated = save_correction(&path, &original, "https://example.invalid/v1")
        .unwrap()
        .unwrap();
    assert_eq!(updated.bind, "127.0.0.1:4847");
    let mut config = updated;
    config.ai_gateway.providers[0] = original.clone();
    config.ai_gateway.providers[0].api_key = "new-synthetic-key".into();
    config.save(&path).unwrap();
    assert!(
        save_correction(&path, &original, "https://example.invalid/v1")
            .unwrap()
            .is_none()
    );
    config.ai_gateway.providers.clear();
    config.save(&path).unwrap();
    assert!(
        save_correction(&path, &original, "https://example.invalid/v1")
            .unwrap()
            .is_none()
    );
}

#[test]
fn only_confirmed_url_repairs_preserve_channel_identity() {
    use super::super::config::provider_route_id;
    let original = ProviderConfig {
        name: "test".into(),
        base_url: "https://example.invalid/proxy".into(),
        ..Default::default()
    };
    assert!(
        serde_json::to_value(&original)
            .unwrap()
            .get("baseUrlRepairOrigin")
            .is_none()
    );
    let mut manual = original.clone();
    manual.base_url.push_str("/v1");
    assert_ne!(provider_route_id(&manual), provider_route_id(&original));
    let mut repaired = original.clone();
    assert!(!repaired.confirm_base_url("https://other.invalid/v1"));
    assert!(repaired.confirm_base_url("https://example.invalid/proxy/v1"));
    let roundtrip: ProviderConfig =
        serde_json::from_value(serde_json::to_value(&repaired).unwrap()).unwrap();
    assert_eq!(provider_route_id(&roundtrip), provider_route_id(&original));
    repaired.base_url = "https://example.invalid/other/v1".into();
    assert_ne!(provider_route_id(&repaired), provider_route_id(&original));
}
