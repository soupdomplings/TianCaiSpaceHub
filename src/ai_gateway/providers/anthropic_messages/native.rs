//! Same-protocol Messages forwarding preserves signatures and provider fields.
use super::*;
use crate::ai_gateway::{
    config::provider_route_id,
    handler::{GatewayRequestEnvelope, insert_initial_log},
};
use crate::app_state::SharedState;
use axum::response::IntoResponse;
use std::time::Instant;

pub async fn handle(
    state: SharedState,
    headers: HeaderMap,
    original: Value,
    provider: ProviderConfig,
) -> Response {
    let route = provider_route_id(&provider);
    let outcome_state = state.clone();
    let result = execute(state, headers, original, provider).await;
    // A streaming response is healthy only after its terminal event, not headers.
    if result.is_err() {
        crate::ai_gateway::handler::record_routing_outcome(
            &outcome_state,
            &route,
            crate::ai_gateway::handler::classify_outcome(&result),
        )
        .await;
    }
    match result {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}
async fn execute(
    state: SharedState,
    headers: HeaderMap,
    original: Value,
    provider: ProviderConfig,
) -> Result<Response, GatewayError> {
    let started = Instant::now();
    let created = request_log::now_ms();
    let model = original["model"]
        .as_str()
        .ok_or_else(|| GatewayError::bad_request("缺少模型名称"))?
        .to_string();
    let streaming = original["stream"].as_bool().unwrap_or(false);
    let ctx = GatewayContext::extract(&headers, None).with_state(&state);
    let route = provider_route_id(&provider);
    ctx.bind_activity(&model, &provider, &route);
    let options = AnthropicProviderOptions::from_provider(&provider)?;
    let mut raw = original.clone();
    raw["model"] = provider
        .resolve_upstream_model(&model)
        .unwrap_or(&model)
        .into();
    let request: GatewayRequest = serde_json::from_value(json!({"model":model,"stream":streaming}))
        .map_err(|_| GatewayError::bad_request("无效模型请求"))?;
    let client = crate::outbound_http::get();
    let upstream =
        build_anthropic_upstream_request(&client, &ctx, &request, &raw, &provider, &options)?;
    let log = {
        let config = state.config.lock().await;
        if config.ai_gateway.request_logging_enabled {
            insert_initial_log(
                &state.ai_gateway_request_logs,
                &ctx,
                &headers,
                &GatewayRequestEnvelope {
                    model: model.clone(),
                    stream: streaming,
                    prompt_cache_key: None,
                },
                Some(&provider),
                &original,
                started,
                created,
                config.ai_gateway.request_log_details_enabled,
            )
        } else {
            None
        }
    };
    // Covers cancellation while waiting for headers as well as body consumption.
    let mut lifetime = NativeLifetime {
        log: log.clone(),
        terminal: false,
        state,
        route,
    };
    if let Some(log) = &log {
        let _ = log.store.update_record(
            log.log_id,
            &RequestLogUpdate {
                upstream_stream: Some(streaming),
                upstream_request_body_bytes: request_log::json_body_size_bytes(&raw),
                upstream_request_json: log.details_enabled.then(|| raw.to_string()),
                upstream_request_headers_json: log
                    .details_enabled
                    .then(|| request_log::headers_to_json(upstream.headers()))
                    .flatten(),
                ..Default::default()
            },
        );
    }
    let response = execute_context_request(
        &client,
        upstream,
        &provider,
        &ctx,
        "Messages upstream request failed",
    )
    .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            fail(&log, &error.message);
            lifetime.terminal = true; // handle() records this explicit error.
            return Err(error);
        }
    };
    request_log::record_upstream_response_headers(log.as_ref(), response.headers());
    let response = match ensure_success_response(&provider.name, response).await {
        Ok(response) => response,
        Err(error) => {
            fail(&log, &error.message);
            lifetime.terminal = true;
            return Err(error);
        }
    };
    if streaming {
        let input = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|e| io::Error::other(e.to_string())))
            .boxed();
        let input = if let Some(context) = log.clone() {
            UpstreamSseCaptureStream::new(input, context).boxed()
        } else {
            input
        };
        let stream = futures_util::stream::unfold(
            (input, lifetime, Vec::<u8>::new(), serde_json::Map::new()),
            |(mut input, mut lifetime, mut pending, mut usage)| async move {
                match input.next().await {
                    Some(Ok(bytes)) => {
                        pending.extend_from_slice(&bytes);
                        while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                            let line = pending.drain(..=end).collect::<Vec<_>>();
                            let text = String::from_utf8_lossy(&line);
                            if let Some(data) = text.trim().strip_prefix("data:") {
                                if let Ok(value) = serde_json::from_str::<Value>(data.trim()) {
                                    if let Some(fields) = value
                                        .get("usage")
                                        .or_else(|| {
                                            value.get("message").and_then(|m| m.get("usage"))
                                        })
                                        .and_then(Value::as_object)
                                    {
                                        usage.extend(fields.clone());
                                    }
                                    if value["type"] == "content_block_delta" {
                                        if let Some(log) = &lifetime.log {
                                            let _ = log.store.record_ttft_once(
                                                log.log_id,
                                                request_log::elapsed_ms(log.started_at),
                                            );
                                        }
                                    }
                                    if value["type"] == "message_stop" && !lifetime.terminal {
                                        lifetime.finish(
                                            crate::ai_gateway::handler::RoutingOutcome::Success,
                                        );
                                        complete(
                                            &lifetime.log,
                                            &Value::Object(usage.clone()),
                                            None,
                                        );
                                    }
                                    if value["type"] == "error" && !lifetime.terminal {
                                        lifetime.finish(crate::ai_gateway::handler::RoutingOutcome::UpstreamFailure);
                                        fail(&lifetime.log, "上游 Messages 返回错误，详见原始事件");
                                    }
                                }
                            }
                        }
                        // Capture is separately bounded. Forwarding stays byte-for-byte.
                        if pending.len() > 4 * 1024 * 1024 {
                            pending.clear();
                        }
                        Some((Ok::<_, io::Error>(bytes), (input, lifetime, pending, usage)))
                    }
                    Some(Err(error)) => {
                        if !lifetime.terminal {
                            lifetime.finish(
                                crate::ai_gateway::handler::RoutingOutcome::UpstreamFailure,
                            );
                            fail(&lifetime.log, "上游流读取失败");
                        }
                        Some((Err(error), (input, lifetime, pending, usage)))
                    }
                    None => {
                        if !lifetime.terminal {
                            terminal(&lifetime.log, "incomplete", "上游流提前结束");
                            lifetime.finish(
                                crate::ai_gateway::handler::RoutingOutcome::UpstreamFailure,
                            );
                        }
                        None
                    }
                }
            },
        );
        let mut result = Response::new(Body::from_stream(stream));
        result.headers_mut().insert(
            "content-type",
            HeaderValue::from_static("text/event-stream"),
        );
        result
            .headers_mut()
            .insert("cache-control", HeaderValue::from_static("no-cache"));
        return Ok(result);
    }
    let body = async {
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ())?;
            if bytes.len().saturating_add(chunk.len()) > 32 * 1024 * 1024 {
                return Err(());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<Value>(&bytes).map_err(|_| ())
    }
    .await;
    let value: Value = match body {
        Ok(value) => value,
        Err(_) => {
            fail(&log, "无效的 Messages 回复");
            lifetime.terminal = true;
            return Err(GatewayError::upstream(
                StatusCode::BAD_GATEWAY,
                "无效的 Messages 回复",
            ));
        }
    };
    if value["type"] != "message" || !value["content"].is_array() {
        fail(&log, "上游未返回有效 Messages 消息");
        lifetime.terminal = true;
        return Err(GatewayError::upstream(
            StatusCode::BAD_GATEWAY,
            "上游未返回有效 Messages 消息",
        ));
    }
    complete(&log, &value["usage"], Some(&value));
    lifetime.finish(crate::ai_gateway::handler::RoutingOutcome::Success);
    Ok(axum::Json(value).into_response())
}
struct NativeLifetime {
    log: Option<RequestLogContext>,
    terminal: bool,
    state: SharedState,
    route: String,
}
impl NativeLifetime {
    fn finish(&mut self, outcome: crate::ai_gateway::handler::RoutingOutcome) {
        if self.terminal {
            return;
        }
        self.terminal = true;
        let (state, route) = (self.state.clone(), self.route.clone());
        tokio::spawn(async move {
            crate::ai_gateway::handler::record_routing_outcome(&state, &route, outcome).await;
        });
    }
}
impl Drop for NativeLifetime {
    fn drop(&mut self) {
        if !self.terminal {
            terminal(&self.log, "cancelled", "客户端停止等待，未自动重复请求");
        }
    }
}
fn terminal(log: &Option<RequestLogContext>, status: &str, message: &str) {
    if let Some(log) = log {
        let _ = log.store.update_record(
            log.log_id,
            &RequestLogUpdate {
                status: Some(status.into()),
                latency_ms: Some(request_log::elapsed_ms(log.started_at)),
                error_message: Some(message.into()),
                ..Default::default()
            },
        );
    }
}

fn complete(log: &Option<RequestLogContext>, usage: &Value, response: Option<&Value>) {
    let Some(log) = log else {
        return;
    };
    let normalized = response::convert_usage_value(usage);
    let _ = log.store.update_record(
        log.log_id,
        &RequestLogUpdate {
            status: Some("completed".into()),
            usage: Some(request_log::usage_from_response_value(
                &json!({"usage":normalized}),
            )),
            latency_ms: Some(request_log::elapsed_ms(log.started_at)),
            response_json: log
                .details_enabled
                .then(|| response.map(Value::to_string))
                .flatten(),
            ..Default::default()
        },
    );
}
fn fail(log: &Option<RequestLogContext>, message: &str) {
    if let Some(log) = log {
        let _ = log.store.update_record(
            log.log_id,
            &RequestLogUpdate {
                status: Some("failed".into()),
                latency_ms: Some(request_log::elapsed_ms(log.started_at)),
                error_message: Some(message.into()),
                ..Default::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::to_bytes};
    fn fixture_state(dir: &tempfile::TempDir) -> SharedState {
        let config = crate::config::AppConfig {
            state_path: dir.path().join("state.json"),
            ai_gateway: crate::ai_gateway::config::AiGatewayConfig {
                enabled: true,
                request_logging_enabled: true,
                request_log_details_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::app_state::AppState::new(dir.path().join("hub.toml"), config, None, None)
    }
    #[tokio::test]
    async fn native_messages_keep_raw_signatures_usage_and_cancelled_lifetimes() {
        let wire = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":5}}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"signature_delta\",\"signature\":\"fixture-signature\"},\"future\":{\"keep\":true}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        let app = Router::new()
            .fallback(move || async move { ([("content-type", "text/event-stream")], wire) });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let state = fixture_state(&dir);
        let provider = ProviderConfig {
            name: "fixture".into(),
            provider_type: crate::ai_gateway::config::ProviderType::AnthropicMessages,
            base_url: base,
            api_key: "fixture-key".into(),
            models: vec!["fixture".into()],
            ..Default::default()
        };
        let request = json!({"model":"fixture","stream":true,"max_tokens":1024,"messages":[{"role":"user","content":"fixture"}]});
        let response = execute(
            state.clone(),
            HeaderMap::new(),
            request.clone(),
            provider.clone(),
        )
        .await
        .unwrap();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert_eq!(bytes.as_ref(), wire.as_bytes());
        let logs = state.ai_gateway_request_logs.list_recent(10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status, "completed");
        assert_eq!(logs[0].input_tokens, Some(35));
        assert_eq!(logs[0].output_tokens, Some(7));
        let response = execute(state.clone(), HeaderMap::new(), request, provider)
            .await
            .unwrap();
        drop(response); // A body never polled is still a cancelled request.
        assert_eq!(
            state.ai_gateway_request_logs.list_recent(10).unwrap()[0].status,
            "cancelled"
        );
        server.abort();
    }
    #[tokio::test]
    async fn cancellation_while_waiting_for_headers_does_not_leave_running_logs() {
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let notify = entered.clone();
        let app = Router::new().fallback(move || {
            let notify = notify.clone();
            async move {
                notify.notify_one();
                std::future::pending::<axum::Json<Value>>().await
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let state = fixture_state(&dir);
        let s = state.clone();
        let provider = ProviderConfig {
            name: "fixture".into(),
            provider_type: crate::ai_gateway::config::ProviderType::AnthropicMessages,
            base_url: base,
            api_key: "fixture".into(),
            models: vec!["fixture".into()],
            ..Default::default()
        };
        let task = tokio::spawn(async move {
            execute(
                s,
                HeaderMap::new(),
                json!({"model":"fixture","stream":true,"max_tokens":32}),
                provider,
            )
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified())
            .await
            .unwrap();
        task.abort();
        let _ = task.await;
        assert_eq!(
            state.ai_gateway_request_logs.list_recent(10).unwrap()[0].status,
            "cancelled"
        );
        server.abort();
    }
}
