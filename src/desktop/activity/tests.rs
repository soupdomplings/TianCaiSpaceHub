use super::*;
use axum::{Json, Router, middleware, routing::post};
use serde_json::json;
use tokio::sync::Notify;

async fn server(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (address, task)
}

/// Exercise real nested routes, middleware, protocol handler, log identity
/// and body lifetime against a held-open synthetic upstream. No Hub startup.
#[tokio::test]
async fn nested_gateway_exposes_live_routes_and_client_identity_until_body_finishes() {
    use crate::ai_gateway::{
        config::{AiGatewayConfig, ProviderConfig, ProviderType},
        request_log::desktop::LogFilter,
    };
    let release = Arc::new(Notify::new());
    let gate = release.clone();
    let upstream=Router::new().route("/v1/responses",post(move |Json(body):Json<serde_json::Value>| {
        let release=gate.clone();
        async move {
            let model=body["model"].as_str().unwrap().to_string();
            let stream=futures_util::stream::unfold((0,release,model),|(step,release,model)|async move {
                let data=match step {
                    0=>format!("data: {}\n\n",json!({"type":"response.created","response":{"id":"resp_synthetic","status":"in_progress"}})),
                    1=>format!("data: {}\n\n",json!({"type":"response.output_text.delta","delta":"synthetic"})),
                    2=>{release.notified().await;format!("data: {}\n\n",json!({"type":"response.completed","response":{"id":"resp_synthetic","status":"completed","model":model,"output":[],"usage":{"input_tokens":10,"output_tokens":2}}}))},
                    _=>return None,
                };
                Some((Ok::<_,std::io::Error>(data),(step+1,release,model)))
            });
            Response::builder().header("content-type","text/event-stream").body(Body::from_stream(stream)).unwrap()
        }
    }));
    let (base, upstream_task) = server(upstream).await;
    let temp = tempfile::tempdir().unwrap();
    let providers = [
        ("ordinary", "codex-model"),
        ("workbuddy:entry-a", "wb-model"),
        ("ordinary-unknown", "external-model"),
    ]
    .into_iter()
    .map(|(name, model)| ProviderConfig {
        name: name.into(),
        models: vec![model.into()],
        base_url: format!("{base}/v1"),
        api_key: "synthetic".into(),
        provider_type: ProviderType::OpenAiResponses,
        ..Default::default()
    })
    .collect();
    let config = crate::config::AppConfig {
        state_path: temp.path().join("state.json"),
        ai_gateway: AiGatewayConfig {
            enabled: true,
            providers,
            request_logging_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let state = crate::app_state::AppState::new(temp.path().join("hub.toml"), config, None, None);
    let app = Router::new()
        .nest(
            "/ai-gateway",
            crate::ai_gateway::router().layer(middleware::from_fn_with_state(state.clone(), track)),
        )
        .with_state(state.clone());
    let (hub, hub_task) = server(app).await;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    for (path, model, agent, expected) in [
        (
            "/v1/responses",
            "codex-model",
            "codex_desktop_rs/0.1",
            Some(ClientId::Codex),
        ),
        (
            "/workbuddy/entry-a/v1/chat/completions",
            "wb-model",
            "generic-http",
            Some(ClientId::WorkBuddy),
        ),
        ("/v1/responses", "external-model", "local-assistance-mcp", None),
    ] {
        let body = if path.ends_with("/completions") {
            json!({"model":model,"messages":[{"role":"user","content":"synthetic"}],"stream":true})
        } else {
            json!({"model":model,"input":"synthetic","stream":true})
        };
        let response = http
            .post(format!("{hub}/ai-gateway{path}"))
            .header("user-agent", agent)
            .header(INTERNAL_HEADER, "forged")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "{path}: {}",
            response.status()
        );
        let rows = state.desktop_activity.snapshot();
        assert_eq!(rows.len(), 1, "{path}");
        let activity = &rows[0];
        assert_ne!(activity.id, "forged");
        assert_eq!(activity.client, expected, "{path}");
        assert!(activity.source.is_none(), "public caller headers cannot claim a managed source");
        assert_eq!(activity.model.as_deref(), Some(model));
        assert!(activity.channel.is_some() && activity.route.is_some());
        let logs = state
            .ai_gateway_request_logs
            .desktop_page(&LogFilter {
                model: Some(model.into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(logs["total"], 1);
        assert_eq!(
            logs["rows"][0]["clientId"],
            expected
                .map(|c| json!(c.key()))
                .unwrap_or(serde_json::Value::Null)
        );
        assert_eq!(
            logs["rows"][0]["routeId"].as_str(),
            activity.route.as_deref()
        );
        release.notify_one();
        let output = response.text().await.unwrap();
        assert!(output.contains("synthetic") || output.contains("completed"));
        tokio::time::timeout(Duration::from_secs(2), async {
            while !state.desktop_activity.snapshot().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    hub_task.abort();
    upstream_task.abort();
}

#[tokio::test]
async fn local_assistance_response_lifetime_preserves_source_for_logs_and_usage() {
    use crate::ai_gateway::{
        config::{AiGatewayConfig, ProviderConfig, ProviderType},
        request_log::desktop::LogFilter,
    };
    use crate::desktop::request_source::LOCAL_ASSISTANCE_MCP;
    use axum::{body::{Bytes, to_bytes}, http::HeaderMap, response::IntoResponse};

    let release = Arc::new(Notify::new());
    let gate = release.clone();
    let upstream = Router::new().route("/v1/responses", post(
        move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
            let release = gate.clone();
            async move {
                assert!(!headers.contains_key(INTERNAL_HEADER));
                let completed = json!({"id":"resp_assistance_fixture","status":"completed",
                    "model":"assistance-fixture","output":[],"usage":{"input_tokens":10,"output_tokens":2}});
                if body["stream"] != true {
                    return Json(completed).into_response();
                }
                let stream = futures_util::stream::unfold(
                    (0, release, completed),
                    |(step, release, completed)| async move {
                        let event = match step {
                            0 => json!({"type":"response.created","response":{"id":"resp_assistance_fixture","status":"in_progress"}}),
                            1 => {
                                release.notified().await;
                                json!({"type":"response.completed","response":completed.clone()})
                            }
                            _ => return None,
                        };
                        Some((Ok::<_, std::io::Error>(format!("data: {event}\n\n")), (step + 1, release, completed)))
                    },
                );
                Response::builder().header("content-type", "text/event-stream")
                    .body(Body::from_stream(stream)).unwrap()
            }
        }
    ));
    let (base, upstream_task) = server(upstream).await;
    let provider = ProviderConfig {
        name: "ordinary-assistance-fixture".into(),
        models: vec!["assistance-fixture".into()],
        base_url: format!("{base}/v1"),
        api_key: "synthetic".into(),
        provider_type: ProviderType::OpenAiResponses,
        ..Default::default()
    };
    let route = crate::ai_gateway::config::provider_route_id(&provider);
    let fingerprint = crate::local_assistance::settings::source_fingerprint(&provider).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let state = crate::app_state::AppState::new(temp.path().join("hub.toml"), crate::config::AppConfig {
        state_path: temp.path().join("state.json"),
        ai_gateway: AiGatewayConfig {
            enabled: true,
            providers: vec![provider],
            request_logging_enabled: true,
            ..Default::default()
        },
        ..Default::default()
    }, None, None);
    let filter = LogFilter { client: Some(LOCAL_ASSISTANCE_MCP.into()), ..Default::default() };
    for (index, (stream, cancel)) in [(true, false), (false, false), (true, true)].into_iter().enumerate() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", "codex_cli_rs/fixture".parse().unwrap());
        headers.insert(INTERNAL_HEADER, "forged".parse().unwrap());
        let body = Bytes::from(serde_json::to_vec(&json!({"model":"assistance-fixture","input":"fixture","stream":stream})).unwrap());
        let response = crate::ai_gateway::handler::handle_local_assistance_responses(
            state.clone(), headers, body, (route.clone(), fingerprint.clone()),
        ).await;
        assert!(response.status().is_success());
        let active = state.desktop_activity.snapshot();
        assert_eq!(active.len(), 1);
        assert_ne!(active[0].id, "forged");
        assert_eq!(active[0].client, Some(ClientId::Tiangong));
        assert_eq!(active[0].source, Some(LOCAL_ASSISTANCE_MCP));
        assert_eq!(active[0].model.as_deref(), Some("assistance-fixture"));
        assert_eq!(active[0].route.as_deref(), Some(route.as_str()));
        let page = state.ai_gateway_request_logs.desktop_page(&filter).unwrap();
        assert_eq!(page["total"], index + 1);
        assert_eq!(page["rows"][0]["clientId"], LOCAL_ASSISTANCE_MCP);
        let detail = state.ai_gateway_request_logs.get_detail(page["rows"][0]["id"].as_i64().unwrap()).unwrap().unwrap();
        assert_eq!(detail.client_id.as_deref(), Some(LOCAL_ASSISTANCE_MCP));
        if cancel {
            drop(response);
        } else {
            if stream { release.notify_one(); }
            let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            assert!(String::from_utf8_lossy(&body).contains("completed"));
        }
        assert!(state.desktop_activity.snapshot().is_empty());
        assert!(state.desktop_activity.recently_completed().iter().all(|r| r.phase == "ended" && r.source == Some(LOCAL_ASSISTANCE_MCP)));
    }
    let usage = state.ai_gateway_request_logs.desktop_usage(&filter).unwrap();
    assert_eq!(usage["summary"]["requests"], 3);
    assert_eq!(usage["summary"]["input"], 20);
    assert_eq!(usage["summary"]["output"], 4);
    assert_eq!(usage["breakdown"]["clients"][0]["name"], LOCAL_ASSISTANCE_MCP);
    assert_eq!(state.ai_gateway_request_logs.desktop_page(&LogFilter {client: Some("codex".into()), ..Default::default()}).unwrap()["total"], 0);
    upstream_task.abort();
}

#[test]
fn caller_evidence_does_not_guess_by_model_or_override_scoped_routes() {
    let store = ActivityStore::default();
    let ordinary = store.begin("/v1/responses");
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("originator", "codex_desktop_rs".parse().unwrap());
    store.identify_headers(&ordinary.id, &headers);
    assert_eq!(
        store.get(&ordinary.id).unwrap().client,
        Some(ClientId::Codex)
    );
    let workbuddy = store.begin("/workbuddy/entry-b/v1/chat/completions");
    store.identify_headers(&workbuddy.id, &headers);
    assert_eq!(
        store.get(&workbuddy.id).unwrap().client,
        Some(ClientId::WorkBuddy)
    );
    let external = store.begin("/v1/responses");
    store.annotate(&external.id, "codex-model", "ordinary", "separate-route");
    store.identify_headers(&external.id, &axum::http::HeaderMap::new());
    assert!(store.get(&external.id).unwrap().client.is_none());
}
