use super::*;
use crate::ai_gateway::{
    tool_names::ToolNameMap,
    transform::{
        chat_to_responses::convert_chat_response_with_tool_names,
        responses_stream::ChatSseToResponsesSse,
        responses_to_chat::build_chat_request_with_tool_names,
    },
};
use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use tokio::sync::watch;

#[derive(Clone)]
pub(super) struct Bridge {
    id: String,
    token: String,
    squad: Arc<Squad>,
    account: ExpectedAccount,
    model: String,
    cancel: watch::Sender<u64>,
}
impl Bridge {
    pub(super) fn new(
        id: String,
        token: String,
        squad: Arc<Squad>,
        account: ExpectedAccount,
        model: String,
    ) -> Self {
        let (cancel, _) = watch::channel(0);
        Self {
            id,
            token,
            squad,
            account,
            model,
            cancel,
        }
    }
    pub(super) fn cancel(&self) {
        self.cancel
            .send_modify(|epoch| *epoch = epoch.wrapping_add(1));
    }
    fn authorize(&self, id: &str, headers: &HeaderMap) -> Result<()> {
        ensure!(
            id == self.id
                && headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.strip_prefix("Bearer "))
                    == Some(self.token.as_str()),
            "天才搭档本机会话已结束"
        );
        ensure!(
            self.squad.assistant_identity()?.as_ref() == Some(&self.account),
            "小分队账户已切换"
        );
        Ok(())
    }
    pub(super) fn ensure_account(&self, epoch: &str, account: &ExpectedAccount) -> Result<()> {
        ensure!(
            self.id == epoch
                && &self.account == account
                && self.squad.assistant_identity()?.as_ref() == Some(account),
            "小分队账户或搭档会话已切换，操作已取消"
        );
        Ok(())
    }
    pub(super) fn tool_squad(&self, epoch: &str, account: &ExpectedAccount) -> Result<Arc<Squad>> {
        self.ensure_account(epoch, account)?;
        Ok(self.squad.clone())
    }
    #[cfg(test)]
    pub(super) fn scope(&self) -> (String, ExpectedAccount) {
        (self.id.clone(), self.account.clone())
    }
}
pub(super) async fn catalog(state: &SharedState, id: &str, headers: &HeaderMap) -> Result<Value> {
    let inner = state.assistant.inner.lock().await;
    let bridge = inner.bridge.as_ref().context("天才搭档会话未启动")?;
    bridge.authorize(id, headers)?;
    inner
        .config
        .as_ref()
        .context("管理员模型尚未就绪")?
        .catalog()
}
pub(super) fn filter_tools(body: &mut Value) {
    let names = tools::definitions()
        .into_iter()
        .filter_map(|d| d["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if let Some(list) = body.get_mut("tools").and_then(Value::as_array_mut) {
        list.retain(|tool| {
            tool["type"] == "function"
                && tool["name"]
                    .as_str()
                    .is_some_and(|name| names.iter().any(|n| n == name))
        });
    }
    if body["tool_choice"].is_object()
        && body["tool_choice"]["name"]
            .as_str()
            .is_none_or(|name| !names.iter().any(|n| n == name))
    {
        body["tool_choice"] = "auto".into();
    }
}
pub(super) async fn forward(
    state: &SharedState,
    id: &str,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Response> {
    let (bridge, config) = {
        let inner = state.assistant.inner.lock().await;
        (
            inner.bridge.clone().context("天才搭档会话未启动")?,
            inner.config.clone().context("管理员模型尚未就绪")?,
        )
    };
    if let Err(error) = bridge.authorize(id, &headers) {
        return Err(error);
    }
    let mut cancel = bridge.cancel.subscribe();
    let mut body: Value = serde_json::from_slice(&bytes).context("模型请求格式无效")?;
    ensure!(body.is_object(), "模型请求应为对象");
    body["model"] = bridge.model.clone().into();
    filter_tools(&mut body);
    let stream = body["stream"].as_bool().unwrap_or(false);
    let (mut body, names) = if config.api.protocol == model::Protocol::ChatCompletions {
        let request = serde_json::from_value(body).context("模型请求无法转换为兼容格式")?;
        build_chat_request_with_tool_names(&request, false).map_err(anyhow::Error::msg)?
    } else {
        (body, ToolNameMap::default())
    };
    body["stream"] = config.api.stream.into();
    if config.api.protocol == model::Protocol::ChatCompletions {
        if config.api.stream {
            body["stream_options"] = json!({"include_usage":true});
        } else {
            body.as_object_mut().unwrap().remove("stream_options");
        }
    }
    let activity = state.desktop_activity.begin_assistant();
    state.desktop_activity.annotate(
        &activity.id,
        &bridge.model,
        "天才搭档管理员服务",
        "squad-assistant",
    );
    let logging = state.config.lock().await.ai_gateway.request_logging_enabled;
    let log = telemetry::begin(state, &bridge.model, stream, logging);
    if let Some(log) = &log {
        state.desktop_activity.log(&activity.id, log.log_id);
    }
    let mut protocol = reqwest::header::HeaderMap::new();
    for name in [
        "openai-beta",
        "x-codex-turn-state",
        "x-codex-turn-metadata",
        "x-codex-session-id",
        "x-client-request-id",
    ] {
        if let Some(value) = headers.get(name) {
            protocol.insert(
                reqwest::header::HeaderName::from_static(name),
                value.clone(),
            );
        }
    }
    protocol.insert(
        "x-hub-client",
        reqwest::header::HeaderValue::from_static("genius-buddy"),
    );
    protocol.insert(
        "x-hub-version",
        reqwest::header::HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
    );
    let result = tokio::select! {
        response=bridge.squad.assistant_model(&bridge.account,&config.api,body,protocol)=>response,
        _=cancel.changed()=>Err(anyhow::anyhow!("天才搭档本轮请求已停止")),
    };
    let response = match result {
        Ok(response) => response,
        Err(error) => {
            telemetry::finish(log.as_ref(), "failed", None, Some(error.to_string()));
            return Err(error);
        }
    };
    if let Err(error) = bridge.authorize(id, &headers) {
        telemetry::finish(log.as_ref(), "cancelled", None, Some(error.to_string()));
        return Err(error);
    }
    let mut builder = Response::builder().status(response.status());
    for name in ["x-request-id", "x-codex-turn-state", "retry-after"] {
        if let Some(value) = response.headers().get(name) {
            builder = builder.header(name, value);
        }
    }
    builder = builder.header("cache-control", "no-store");
    if !response.status().is_success() {
        if let Some(content_type) = response.headers().get("content-type") {
            builder = builder.header("content-type", content_type);
        }
        let status = response.status();
        let mut response = response;
        let mut bytes = Vec::new();
        loop {
            let chunk = tokio::select! {chunk=response.chunk()=>match chunk{
                Ok(chunk)=>chunk,Err(error)=>{telemetry::finish(log.as_ref(),"failed",None,Some("模型 JSON 响应中断".into()));return Err(error.into());}},_=cancel.changed()=>{
            telemetry::finish(log.as_ref(),"cancelled",None,None);anyhow::bail!("本轮请求已停止");}};
            let Some(chunk) = chunk else {
                break;
            };
            if bytes.len() + chunk.len() > 32 * 1024 * 1024 {
                telemetry::finish(
                    log.as_ref(),
                    "failed",
                    None,
                    Some("模型 JSON 响应过大".into()),
                );
                anyhow::bail!("模型 JSON 响应过大");
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = serde_json::from_slice::<Value>(&bytes).ok();
        telemetry::finish(
            log.as_ref(),
            "failed",
            value.as_ref(),
            (!status.is_success()).then(|| format!("管理员模型服务返回 HTTP {}", status.as_u16())),
        );
        return Ok(activity.response(builder.body(Body::from(bytes))?));
    }
    let upstream_headers = response.headers().clone();
    let upstream = futures_util::stream::try_unfold(
        (response, cancel),
        |(mut response, mut cancel)| async move {
            let chunk = tokio::select! {
                chunk=response.chunk()=>chunk.map_err(|_|std::io::Error::other("天才搭档模型流已中断"))?,
                _=cancel.changed()=>return Err(std::io::Error::other("天才搭档本轮请求已停止")),
            };
            Ok(chunk.map(|chunk| (chunk, (response, cancel))))
        },
    );
    if !stream || !config.api.stream {
        use crate::ai_gateway::gmclaw_stream::{Protocol, collect};
        let protocol = match config.api.protocol {
            model::Protocol::Responses => Protocol::Responses,
            model::Protocol::ChatCompletions => Protocol::Chat,
        };
        let result = collect(
            upstream,
            &upstream_headers,
            protocol,
            "天才搭档模型 API",
            tokio::time::Instant::now() + Duration::from_secs(1800),
        )
        .await;
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                telemetry::finish(
                    log.as_ref(),
                    "failed",
                    None,
                    Some("模型 API 响应未完成".into()),
                );
                return Err(anyhow::anyhow!("{}", error.message));
            }
        };
        let value = (|| -> Result<Value> {
            let value = if config.api.protocol == model::Protocol::ChatCompletions {
                serde_json::to_value(
                    convert_chat_response_with_tool_names(&value, &bridge.model, &names)
                        .map_err(anyhow::Error::msg)?,
                )?
            } else {
                value
            };
            ensure!(
                value["output"].is_array(),
                "模型 API 未返回有效的 Responses 结果"
            );
            Ok(value)
        })();
        let value = match value {
            Ok(value) => value,
            Err(error) => {
                telemetry::finish(log.as_ref(), "failed", None, Some(error.to_string()));
                return Err(error);
            }
        };
        telemetry::finish(
            log.as_ref(),
            value["status"].as_str().unwrap_or("completed"),
            Some(&value),
            None,
        );
        let body = if stream {
            builder = builder.header("content-type", "text/event-stream");
            Body::from(response_events(&value)?)
        } else {
            builder = builder.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&value)?)
        };
        return Ok(activity.response(builder.body(body)?));
    }
    let stream: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = std::result::Result<Bytes, std::io::Error>> + Send>,
    > = if config.api.protocol == model::Protocol::ChatCompletions {
        Box::pin(ChatSseToResponsesSse::new_with_tool_names(
            Box::pin(upstream),
            bridge.model.clone(),
            names,
        ))
    } else {
        Box::pin(upstream)
    };
    builder = builder.header("content-type", "text/event-stream");
    let body = if let Some(log) = log {
        Body::from_stream(crate::ai_gateway::request_log::ResponsesSseLogStream::new(
            Box::pin(stream),
            log,
        ))
    } else {
        Body::from_stream(stream)
    };
    Ok(activity.response(builder.body(body)?))
}
/// A non-streaming source still delivers complete items and real usage to Server.
fn response_events(value: &Value) -> Result<String> {
    ensure!(
        value["output"].is_array(),
        "模型 API 未返回有效的 Responses 结果"
    );
    let mut events = vec![
        json!({"type":"response.created","response":{"id":value["id"],"status":"in_progress"}}),
    ];
    for (index, item) in value["output"].as_array().unwrap().iter().enumerate() {
        events.push(json!({"type":"response.output_item.added","output_index":index,"item":item}));
        if let Some(content) = item["content"].as_array() {
            for (content_index, part) in content.iter().enumerate() {
                if part["type"] == "output_text" {
                    events.push(
                        json!({"type":"response.output_text.delta","item_id":item["id"],
                        "output_index":index,"content_index":content_index,"delta":part["text"]}),
                    );
                }
            }
        }
        events.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    let terminal = match value["status"].as_str() {
        Some("failed") => "response.failed",
        Some("incomplete") => "response.incomplete",
        _ => "response.completed",
    };
    events.push(json!({"type":terminal,"response":value}));
    Ok(events
        .into_iter()
        .enumerate()
        .map(|(sequence, mut event)| {
            event["sequence_number"] = json!(sequence);
            format!("data: {event}\n\n")
        })
        .collect())
}
pub(super) fn rejection(error: anyhow::Error) -> Response {
    (
        StatusCode::CONFLICT,
        axum::Json(json!({"error":{"type":"assistant_unavailable","message":error.to_string()}})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::post};
    use futures_util::StreamExt;

    #[tokio::test]
    async fn ordinary_api_adapts_both_protocols_stream_modes_and_tool_continuations_without_extra_site_posts()
     {
        for chat in [false, true] {
            for stream in [false, true] {
                let (seen, mut requests) = tokio::sync::mpsc::unbounded_channel();
                let endpoint = if chat {
                    "/v1/chat/completions"
                } else {
                    "/v1/responses"
                };
                let app=Router::new().route(endpoint,post(move |headers:HeaderMap,axum::Json(body):axum::Json<Value>| {
                let seen=seen.clone(); async move {
                    assert_eq!(headers["authorization"],"Bearer fixture-admin-api-key");
                    assert_eq!(body["model"],"fixture-model");
                    assert_eq!(body["stream"],stream);
                    assert!(!body.to_string().contains("fixture-admin-api-key"));
                    let cancelling=body.to_string().contains("cancel-fixture");
                    seen.send(body).unwrap();
                    if chat && stream && cancelling {
                        let first=json!({"id":"chat-cancel","choices":[{"index":0,"delta":{"role":"assistant","content":"未完成"},"finish_reason":null}]});
                        let chunks=futures_util::stream::once(async move {Ok::<_,std::io::Error>(Bytes::from(format!("data: {first}\n\n")))})
                            .chain(futures_util::stream::pending());
                        return ([("content-type","text/event-stream")],Body::from_stream(chunks)).into_response();
                    }
                    if chat {
                        if stream {
                            let chunk=json!({"id":"chat-fixture","choices":[{"index":0,"delta":{"role":"assistant","content":"配置入口",
                                "tool_calls":[{"index":0,"id":"call-help","type":"function","function":{"name":"hub_help","arguments":"{}"}}]},"finish_reason":"tool_calls"}],
                                "usage":{"prompt_tokens":11,"completion_tokens":3,"total_tokens":14}});
                            ([("content-type","text/event-stream")],format!("data: {chunk}\n\ndata: [DONE]\n\n")).into_response()
                        } else { axum::Json(json!({"id":"chat-fixture","choices":[{"message":{"role":"assistant","content":"配置入口",
                            "tool_calls":[{"id":"call-help","type":"function","function":{"name":"hub_help","arguments":"{}"}}]},"finish_reason":"tool_calls"}],
                            "usage":{"prompt_tokens":11,"completion_tokens":3,"total_tokens":14}})).into_response() }
                    } else {
                        let result=json!({"id":"responses-fixture","object":"response","status":"completed","model":"fixture-model",
                            "output":[{"type":"message","id":"message-1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"配置入口"}]},
                                {"type":"function_call","id":"tool-1","call_id":"call-help","name":"hub_help","arguments":"{}"}],
                            "usage":{"input_tokens":11,"output_tokens":3,"total_tokens":14}});
                        if stream { ([("content-type","text/event-stream")],response_events(&result).unwrap()).into_response() }
                        else { axum::Json(result).into_response() }
                    }
                }
            }));
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let origin = format!("http://{}", listener.local_addr().unwrap());
                let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
                let temp = tempfile::tempdir().unwrap();
                let state = crate::app_state::AppState::new(
                    temp.path().join("config.toml"),
                    crate::config::AppConfig {
                        state_path: temp.path().join("state.json"),
                        ..Default::default()
                    },
                    None,
                    None,
                );
                let squad = Arc::new(
                    Squad::synthetic(
                        temp.path().join("squad"),
                        reqwest::Client::builder().no_proxy().build().unwrap(),
                        origin.clone(),
                    )
                    .unwrap(),
                );
                let account = squad.assistant_test_identity("one").unwrap();
                let config=model::Configuration::parse(json!({"enabled":true,"name":"天才搭档",
                "model":{"id":"fixture-model","contextWindow":128000,"reasoningEfforts":["medium"],"defaultReasoningEffort":"medium","supportsReasoning":true},
                "api":{"baseUrl":format!("{origin}/v1"),"apiKey":"fixture-admin-api-key","protocol":if chat {"chat_completions"}else{"responses"},"stream":stream}})).unwrap();
                {
                    let mut inner = state.assistant.inner.lock().await;
                    inner.config = Some(config);
                    inner.bridge = Some(Bridge::new(
                        "fixture".into(),
                        "local-token".into(),
                        squad.clone(),
                        account,
                        "fixture-model".into(),
                    ));
                }
                let mut headers = HeaderMap::new();
                headers.insert("authorization", "Bearer local-token".parse().unwrap());
                for continuation in [false, true] {
                    let input = if continuation {
                        json!([
                    {"type":"function_call","call_id":"call-help","name":"hub_help","arguments":"{}"},
                    {"type":"function_call_output","call_id":"call-help","output":"本机帮助"}])
                    } else {
                        json!([{"type":"message","role":"user","content":"查看配置帮助"}])
                    };
                    let body = json!({"model":"caller-model","stream":true,"input":input,
                    "tools":[{"type":"function","name":"hub_help","parameters":{"type":"object","properties":{}}},{"type":"function","name":"shell"}]});
                    let response = forward(
                        &state,
                        "fixture",
                        headers.clone(),
                        Bytes::from(serde_json::to_vec(&body).unwrap()),
                    )
                    .await
                    .unwrap();
                    assert_eq!(response.headers()["content-type"], "text/event-stream");
                    let text = String::from_utf8(
                        axum::body::to_bytes(response.into_body(), 1024 * 1024)
                            .await
                            .unwrap()
                            .to_vec(),
                    )
                    .unwrap();
                    assert!(
                        text.contains("response.completed")
                            && text.contains("call-help")
                            && text.contains("配置入口"),
                        "{text}"
                    );
                    assert!(!text.contains("fixture-admin-api-key"));
                    let request = requests.recv().await.unwrap();
                    assert!(!request.to_string().contains("shell"));
                    if continuation {
                        if chat {
                            assert!(
                                request["messages"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .any(
                                        |m| m["role"] == "tool" && m["tool_call_id"] == "call-help"
                                    )
                            );
                        } else {
                            assert_eq!(request["input"][1]["call_id"], "call-help");
                        }
                    }
                }
                if chat && stream {
                    let response=forward(&state,"fixture",headers.clone(),Bytes::from(serde_json::to_vec(
                        &json!({"model":"fixture-model","stream":true,"input":"cancel-fixture"})).unwrap())).await.unwrap();
                    let mut chunks = response.into_body().into_data_stream();
                    chunks.next().await.unwrap().unwrap();
                    state
                        .assistant
                        .inner
                        .lock()
                        .await
                        .bridge
                        .as_ref()
                        .unwrap()
                        .cancel();
                    let remaining =
                        tokio::time::timeout(Duration::from_secs(2), chunks.collect::<Vec<_>>())
                            .await
                            .unwrap();
                    assert!(remaining.iter().any(|chunk| chunk.is_err()));
                    assert!(
                        remaining
                            .iter()
                            .filter_map(|chunk| chunk.as_ref().ok())
                            .all(|chunk| !String::from_utf8_lossy(chunk)
                                .contains("response.completed"))
                    );
                }
                let status = super::super::status(&state).await.unwrap();
                assert!(!status.to_string().contains("fixture-admin-api-key"));
                assert!(status.get("api").is_none());
                server.abort();
            }
        }
    }

    #[test]
    fn model_tools_are_limited_to_hub_dynamic_allowlist() {
        let mut body = json!({"tools":[{"type":"function","name":"hub_help"},
            {"type":"function","name":"shell"},{"type":"web_search"},
            {"type":"custom","name":"apply_patch"},{"type":"function","name":"read_file"}],
            "tool_choice":{"type":"function","name":"shell"}});
        filter_tools(&mut body);
        assert_eq!(
            body["tools"],
            json!([{"type":"function","name":"hub_help"}])
        );
        assert_eq!(body["tool_choice"], "auto");
    }
}
