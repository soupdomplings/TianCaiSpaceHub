//! New clients have explicit routes and per-entry local credentials.
use super::{error::GatewayError, handler, workbuddy};
use crate::app_state::SharedState;
use axum::{
    Json,
    body::{Body, Bytes, to_bytes},
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};

pub async fn chat(
    State(state): State<SharedState>,
    Path((key, entry)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (key, entry, headers) = match native_scope(&state, key, entry, headers, Some(&body)).await {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    match crate::desktop::models::client(&key) {
        Ok(client) => {
            handler::handle_client_chat_completions(
                state,
                headers,
                body,
                handler::ChatClient::Desktop(client),
                Some(entry),
            )
            .await
        }
        Err(_) => GatewayError::bad_request("未知客户端").into_response(),
    }
}
pub async fn responses(
    State(state): State<SharedState>,
    Path((key, entry)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (key, entry, headers) = match native_scope(&state, key, entry, headers, Some(&body)).await {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    match crate::desktop::models::client(&key) {
        Ok(client) => {
            handler::handle_responses_scoped(state, headers, body, Some((client, entry)), None)
                .await
        }
        Err(_) => GatewayError::bad_request("未知客户端").into_response(),
    }
}
pub async fn models(
    State(state): State<SharedState>,
    Path((key, entry)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if key == "webstorm-native" {
        let config = state.config.lock().await;
        let selected = config
            .ai_gateway
            .providers
            .iter()
            .filter(|p| {
                p.enabled
                    && p.desktop_binding.as_ref().is_some_and(|b| {
                        b.client == crate::desktop::clients::ClientId::WebStorm
                            && b.mode == "native"
                            && b.target_id == entry
                    })
            })
            .collect::<Vec<_>>();
        if !selected.first().is_some_and(|p| {
            native_authorized(&headers, &p.desktop_binding.as_ref().unwrap().local_token)
        }) {
            return GatewayError::bad_request("WebStorm 本机授权无效").into_response();
        }
        return Json(json!({"object":"list","data":selected.iter().filter_map(|p|p.desktop_binding.as_ref()).map(|b|json!({"id":b.model,"object":"model","owned_by":"TianCaiSpaceHub"})).collect::<Vec<_>>()})).into_response();
    }
    let Ok(client) = crate::desktop::models::client(&key) else {
        return GatewayError::bad_request("未知客户端").into_response();
    };
    let config = state.config.lock().await;
    let Ok(name) = crate::desktop::models::name(client, &entry) else {
        return GatewayError::bad_request("无效条目").into_response();
    };
    let Some(provider) = config
        .ai_gateway
        .providers
        .iter()
        .find(|p| p.name == name && p.enabled)
    else {
        return GatewayError::invalid_model("").into_response();
    };
    let model = provider
        .desktop_binding
        .as_ref()
        .map(|b| b.model.as_str())
        .unwrap_or("");
    let mut routing = state.ai_gateway_routing.lock().await;
    match super::router::resolve_desktop_provider_with_state(client,&entry,model,&headers,None,&config.ai_gateway,&mut routing,std::time::Instant::now()){
        Ok(_)=>Json(json!({"object":"list","data":[{"id":model,"object":"model","owned_by":"TianCaiSpaceHub"}]})).into_response(),Err(e)=>e.into_response()
    }
}
pub async fn messages(
    State(state): State<SharedState>,
    Path((key, entry)): Path<(String, String)>,
    mut headers: HeaderMap,
    body: Bytes,
) -> Response {
    let body = match handler::decode_request_body(&headers, body) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    headers.remove("content-encoding");
    headers.remove("content-length");
    let (key, entry, resolved_headers) =
        match native_scope(&state, key, entry, headers, Some(&body)).await {
            Ok(v) => v,
            Err(e) => return e.into_response(),
        };
    headers = resolved_headers;
    let Ok(client) = crate::desktop::models::client(&key) else {
        return GatewayError::bad_request("未知客户端").into_response();
    };
    let raw: Value = match serde_json::from_slice(&body) {
        Ok(raw) => raw,
        Err(_) => return GatewayError::bad_request("Messages 请求不是有效 JSON").into_response(),
    };
    let model = raw
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let streaming = raw.get("stream").and_then(Value::as_bool) == Some(true);
    let provider = {
        let config = state.config.lock().await;
        let mut routing = state.ai_gateway_routing.lock().await;
        match super::router::resolve_desktop_provider_with_state(
            client,
            &entry,
            &model,
            &headers,
            None,
            &config.ai_gateway,
            &mut routing,
            std::time::Instant::now(),
        ) {
            Ok((p, _)) => p.clone(),
            Err(e) => return e.into_response(),
        }
    };
    if provider.provider_type == super::config::ProviderType::AnthropicMessages {
        return super::providers::anthropic_messages::native::handle(state, headers, raw, provider)
            .await;
    }
    let converted = match messages_to_responses(&raw) {
        Ok(v) => v,
        Err(e) => return GatewayError::bad_request(e).into_response(),
    };
    headers.remove("content-encoding");
    headers.remove("content-length");
    let response = handler::handle_responses_scoped(
        state,
        headers,
        Bytes::from(converted.to_string()),
        Some((client, entry)),
        Some(raw),
    )
    .await;
    if !response.status().is_success() {
        return response;
    }
    if streaming {
        let mut response = Response::new(stream(response.into_body(), model));
        response.headers_mut().insert(
            "content-type",
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        response.headers_mut().insert(
            "cache-control",
            axum::http::HeaderValue::from_static("no-cache"),
        );
        return response;
    }
    match to_bytes(response.into_body(), 32 * 1024 * 1024).await {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => match to_messages(&value, &model) {
                Ok(value) => Json(value).into_response(),
                Err(error) => GatewayError::upstream(axum::http::StatusCode::BAD_GATEWAY, error)
                    .into_response(),
            },
            Err(_) => GatewayError::upstream(
                axum::http::StatusCode::BAD_GATEWAY,
                "模型返回了无效的 Responses 数据",
            )
            .into_response(),
        },
        Err(_) => GatewayError::upstream(axum::http::StatusCode::BAD_GATEWAY, "读取模型回复失败")
            .into_response(),
    }
}

fn native_authorized(headers: &HeaderMap, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    let actual = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    actual.len() == expected.len()
        && actual
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |n, (a, b)| n | (a ^ b))
            == 0
}
async fn native_scope(
    state: &SharedState,
    key: String,
    entry: String,
    headers: HeaderMap,
    body: Option<&Bytes>,
) -> Result<(String, String, HeaderMap), GatewayError> {
    if key != "webstorm-native" {
        return Ok((key, entry, headers));
    }
    let decoded = handler::decode_request_body(&headers, body.cloned().unwrap_or_default())?;
    let value: Value = serde_json::from_slice(&decoded)
        .map_err(|_| GatewayError::bad_request("模型请求格式无效"))?;
    let model = value["model"]
        .as_str()
        .ok_or_else(|| GatewayError::bad_request("请选择模型"))?;
    let config = state.config.lock().await;
    let binding = config
        .ai_gateway
        .providers
        .iter()
        .filter(|p| p.enabled)
        .filter_map(|p| p.desktop_binding.as_ref())
        .find(|b| {
            b.client == crate::desktop::clients::ClientId::WebStorm
                && b.mode == "native"
                && b.target_id == entry
                && b.model == model
        })
        .ok_or_else(|| GatewayError::invalid_model(model))?;
    if !native_authorized(&headers, &binding.local_token) {
        return Err(GatewayError::bad_request("WebStorm 本机授权无效"));
    }
    if let Some(activity) = headers
        .get(crate::desktop::activity::INTERNAL_HEADER)
        .and_then(|v| v.to_str().ok())
    {
        state
            .desktop_activity
            .identify(activity, binding.client, &binding.id);
    }
    Ok(("webstorm".into(), binding.id.clone(), headers))
}

pub fn messages_to_responses(raw: &Value) -> Result<Value, String> {
    let mut messages = Vec::new();
    let mut tool_outputs = BTreeMap::new();
    if let Some(system) = raw.get("system") {
        let content = if system.is_string() {
            system.clone()
        } else {
            json!(
                system
                    .as_array()
                    .ok_or("system 格式无效")?
                    .iter()
                    .map(|b| b.get("text").and_then(Value::as_str).unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        messages.push(json!({"role":"system","content":content}));
    }
    for item in raw
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("Messages 请求缺少 messages")?
    {
        let role = item
            .get("role")
            .and_then(Value::as_str)
            .ok_or("消息缺少 role")?;
        let content = &item["content"];
        if content.is_string() {
            messages.push(json!({"role":role,"content":content}));
            continue;
        }
        let mut parts = Vec::new();
        let mut calls = Vec::new();
        let mut results = Vec::new();
        for block in content.as_array().ok_or("消息 content 格式无效")? {
            match block.get("type").and_then(Value::as_str).unwrap_or(""){
                "text"=>parts.push(json!({"type":"text","text":block["text"]})),
                "image"=>{let source=&block["source"];let url=if source["type"]=="base64"{format!("data:{};base64,{}",source["media_type"].as_str().ok_or("图片缺少媒体类型")?,source["data"].as_str().ok_or("图片缺少内容")?)}else{source["url"].as_str().ok_or("图片来源格式无效")?.into()};parts.push(json!({"type":"image_url","image_url":{"url":url}}));},
                "tool_use"=>calls.push(json!({"id":block["id"],"type":"function","function":{"name":block["name"],"arguments":block["input"].to_string()}})),
                "tool_result"=>{let id=block["tool_use_id"].as_str().ok_or("工具结果缺少调用身份")?;let output=tool_output(&block["content"],block["is_error"]==true)?;tool_outputs.insert(id.to_string(),output);results.push(json!({"role":"tool","tool_call_id":id,"content":""}));},
                "thinking"|"redacted_thinking"=>return Err("当前来源使用其他协议，无法安全复用该思考签名；请新建会话或为此客户端选择 Messages 渠道".into()),
                other=>return Err(format!("当前跨协议接入不支持 {other} 内容；请选择原生 Messages 渠道")),
            }
        }
        if !parts.is_empty() || !calls.is_empty() {
            let mut item = json!({"role":role,"content":parts});
            if !calls.is_empty() {
                item["tool_calls"] = calls.into();
            }
            messages.push(item);
        }
        messages.extend(results);
    }
    let mut chat = json!({"model":raw["model"],"messages":messages,"stream":raw.get("stream").cloned().unwrap_or(false.into())});
    for (source, destination) in [
        ("max_tokens", "max_tokens"),
        ("temperature", "temperature"),
        ("top_p", "top_p"),
        ("stop_sequences", "stop"),
    ] {
        if let Some(value) = raw.get(source) {
            chat[destination] = value.clone();
        }
    }
    if let Some(tools) = raw.get("tools").and_then(Value::as_array) {
        let mut functions = Vec::new();
        for tool in tools {
            if tool
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t != "custom")
            {
                return Err("此工具依赖 Messages 原生服务，请选择对应来源".into());
            }
            functions.push(json!({"type":"function","function":{"name":tool["name"],"description":tool.get("description").cloned().unwrap_or("".into()),"parameters":tool["input_schema"]}}));
        }
        chat["tools"] = functions.into();
    }
    if let Some(choice) = raw.get("tool_choice") {
        chat["tool_choice"] = match choice["type"].as_str() {
            Some("auto") => "auto".into(),
            Some("any") => "required".into(),
            Some("none") => "none".into(),
            Some("tool") => json!({"type":"function","function":{"name":choice["name"]}}),
            _ => return Err("不支持的 tool_choice".into()),
        };
    }
    let mut response = workbuddy::chat_request_to_responses(&chat)?;
    if let Some(items) = response["input"].as_array_mut() {
        for item in items {
            if item["type"] == "function_call_output" {
                if let Some(output) = item["call_id"].as_str().and_then(|id| tool_outputs.get(id)) {
                    item["output"] = output.clone();
                }
            }
        }
    }
    if let Some(max) = raw.get("max_tokens") {
        response["max_output_tokens"] = max.clone();
    }
    Ok(response)
}
fn tool_output(output: &Value, error: bool) -> Result<Value, String> {
    if let Some(text) = output.as_str() {
        return Ok(if error {
            format!("工具执行失败：{text}").into()
        } else {
            text.into()
        });
    }
    if output.is_null() {
        return Ok("".into());
    }
    let mut parts = Vec::new();
    if error {
        parts.push(json!({"type":"input_text","text":"工具执行失败"}));
    }
    for part in output.as_array().ok_or("工具结果格式无效")? {
        match part["type"].as_str() {
            Some("text") => parts.push(json!({"type":"input_text","text":part["text"]})),
            Some("image") => {
                let source = &part["source"];
                let url = if source["type"] == "base64" {
                    format!(
                        "data:{};base64,{}",
                        source["media_type"].as_str().ok_or("图片缺少媒体类型")?,
                        source["data"].as_str().ok_or("图片缺少内容")?
                    )
                } else {
                    source["url"].as_str().ok_or("图片地址无效")?.into()
                };
                parts.push(json!({"type":"input_image","image_url":url}));
            }
            _ => return Err("此工具结果需要原生 Messages 渠道，未将内容压成文字".into()),
        }
    }
    Ok(parts.into())
}
fn usage(value: &Value) -> Value {
    let input = value["input_tokens"].as_i64().unwrap_or(0);
    let cached = value["input_tokens_details"]["cached_tokens"]
        .as_i64()
        .unwrap_or(0);
    json!({"input_tokens":input.saturating_sub(cached).max(0),"cache_read_input_tokens":cached,"output_tokens":value["output_tokens"].as_i64().unwrap_or(0)})
}
fn stop(value: &Value, tools: bool) -> &'static str {
    if value["status"] == "incomplete" {
        if value["incomplete_details"]["reason"] == "max_output_tokens"
            || value["incomplete_details"]["reason"].is_null()
        {
            "max_tokens"
        } else {
            "end_turn"
        }
    } else if tools {
        "tool_use"
    } else {
        "end_turn"
    }
}
pub fn to_messages(value: &Value, model: &str) -> Result<Value, String> {
    let mut content = Vec::new();
    let mut tools = false;
    for item in value["output"].as_array().into_iter().flatten() {
        match item["type"].as_str() {
            Some("message") => {
                for part in item["content"].as_array().into_iter().flatten() {
                    if part["type"] == "output_text" {
                        content.push(json!({"type":"text","text":part["text"]}));
                    }
                }
            }
            Some("function_call") => {
                tools = true;
                let args = serde_json::from_str::<Value>(
                    item["arguments"].as_str().ok_or("工具回复缺少参数")?,
                )
                .map_err(|_| "工具回复参数不是有效 JSON，未发起工具执行")?;
                if !args.is_object() {
                    return Err("工具回复参数需要为 JSON 对象，未发起工具执行".into());
                }
                content.push(json!({"type":"tool_use","id":item["call_id"],"name":item["name"],"input":args}));
            }
            _ => {}
        }
    }
    Ok(
        json!({"id":value.get("id").cloned().unwrap_or(format!("msg_{}",uuid::Uuid::new_v4()).into()),"type":"message","role":"assistant","model":model,"content":content,"stop_reason":stop(value,tools),"stop_sequence":null,"usage":usage(&value["usage"])}),
    )
}
fn event(kind: &str, value: Value) -> Bytes {
    Bytes::from(format!("event: {kind}\ndata: {value}\n\n"))
}
#[derive(Default)]
struct StateMachine {
    pending: Vec<u8>,
    queue: VecDeque<Bytes>,
    open: BTreeMap<String, (usize, bool)>,
    closed: std::collections::BTreeSet<usize>,
    arguments: BTreeMap<String, String>,
    text: BTreeMap<String, String>,
    next: usize,
    done: bool,
    tools: bool,
}
impl StateMachine {
    fn fail(&mut self, message: &str) {
        if self.done {
            return;
        }
        self.queue.push_back(event(
            "error",
            json!({"type":"error","error":{"type":"api_error","message":message}}),
        ));
        self.done = true;
    }
    fn text(&mut self, key: &str, text: &str, final_value: bool) {
        let sent = self.text.entry(key.into()).or_default();
        let delta = if final_value {
            let Some(tail) = text.strip_prefix(sent.as_str()) else {
                self.fail("上游文本终值与流式内容不一致");
                return;
            };
            tail.to_string()
        } else {
            text.to_string()
        };
        if delta.is_empty() {
            return;
        }
        if self
            .open
            .get(key)
            .is_some_and(|(i, _)| self.closed.contains(i))
        {
            self.fail("上游在文本结束后继续发送内容");
            return;
        }
        sent.push_str(&delta);
        let index = self.open(key, None);
        self.queue.push_back(event("content_block_delta", json!({"type":"content_block_delta","index":index,"delta":{"type":"text_delta","text":delta}})));
    }
    fn tool_done(&mut self, key: &str, full: &Value) {
        let Some(full) = full.as_str() else {
            self.fail("上游工具参数缺少 JSON 内容，未完成工具调用");
            return;
        };
        if !serde_json::from_str::<Value>(full).is_ok_and(|v| v.is_object()) {
            self.fail("上游工具参数不是有效 JSON 对象，未完成工具调用");
            return;
        }
        let Some((index, _)) = self.open.get(key).copied() else {
            self.fail("上游工具参数缺少对应调用，未完成工具调用");
            return;
        };
        let sent = self.arguments.entry(key.into()).or_default();
        let Some(tail) = full.strip_prefix(sent.as_str()) else {
            self.fail("上游工具终值与已发送参数不一致，未完成工具调用");
            return;
        };
        if !tail.is_empty() {
            if self.closed.contains(&index) {
                self.fail("上游在工具结束后改变了参数，未完成工具调用");
                return;
            }
            self.queue.push_back(event("content_block_delta", json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":tail}})));
            sent.push_str(tail);
        }
    }
    fn output(&mut self, item: &Value) {
        if self.done {
            return;
        }
        if item["type"] == "function_call" {
            let Some(key) = item["id"].as_str().filter(|v| !v.is_empty()) else {
                self.fail("上游工具回复缺少身份");
                return;
            };
            if !item["call_id"].as_str().is_some_and(|v| !v.is_empty())
                || !item["name"].as_str().is_some_and(|v| !v.is_empty())
            {
                self.fail("上游工具回复缺少名称或调用身份");
                return;
            }
            let index = self.open(key, Some(item));
            self.tool_done(key, &item["arguments"]);
            if !self.done && self.closed.insert(index) {
                self.queue.push_back(event(
                    "content_block_stop",
                    json!({"type":"content_block_stop","index":index}),
                ));
            }
        } else if item["type"] == "message" {
            for (i, part) in item["content"].as_array().into_iter().flatten().enumerate() {
                if part["type"] == "output_text" {
                    let key = format!("{}:{i}", item["id"].as_str().unwrap_or("text"));
                    self.text(&key, part["text"].as_str().unwrap_or(""), true);
                }
            }
        }
    }
    fn open(&mut self, key: &str, tool: Option<&Value>) -> usize {
        if let Some((index, _)) = self.open.get(key) {
            return *index;
        }
        let index = self.next;
        self.next += 1;
        let block = if let Some(item) = tool {
            self.tools = true;
            json!({"type":"tool_use","id":item["call_id"],"name":item["name"],"input":{}})
        } else {
            json!({"type":"text","text":""})
        };
        self.open.insert(key.into(), (index, tool.is_some()));
        self.queue.push_back(event(
            "content_block_start",
            json!({"type":"content_block_start","index":index,"content_block":block}),
        ));
        index
    }
    fn feed(&mut self, value: Value) {
        if self.done {
            return;
        }
        match value["type"].as_str().unwrap_or("") {
            "response.output_item.added" if value["item"]["type"] == "function_call" => {
                if !value["item"]["id"].as_str().is_some_and(|s| !s.is_empty())
                    || !value["item"]["call_id"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                    || !value["item"]["name"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                {
                    self.fail("上游工具回复缺少身份或名称");
                    return;
                }
                self.open(
                    value["item"]["id"].as_str().unwrap_or("tool"),
                    Some(&value["item"]),
                );
            }
            "response.output_text.delta" => {
                let key = format!(
                    "{}:{}",
                    value["item_id"].as_str().unwrap_or("text"),
                    value["content_index"].as_u64().unwrap_or(0)
                );
                self.text(&key, value["delta"].as_str().unwrap_or(""), false);
            }
            "response.function_call_arguments.delta" => {
                let key = value["item_id"].as_str().unwrap_or("tool");
                if let Some((index, _)) = self.open.get(key) {
                    if self.closed.contains(index) {
                        self.fail("上游在工具结束后继续发送参数");
                        return;
                    }
                    self.arguments
                        .entry(key.into())
                        .or_default()
                        .push_str(value["delta"].as_str().unwrap_or(""));
                    self.queue.push_back(event("content_block_delta",json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":value["delta"]}})));
                } else {
                    self.fail("上游工具参数缺少对应调用");
                }
            }
            "response.function_call_arguments.done" => {
                let key = value["item_id"].as_str().unwrap_or("tool");
                self.tool_done(key, &value["arguments"]);
            }
            "response.content_part.done" => {
                let key = format!(
                    "{}:{}",
                    value["item_id"].as_str().unwrap_or("text"),
                    value["content_index"].as_u64().unwrap_or(0)
                );
                if value["part"]["type"] == "output_text" {
                    self.text(&key, value["part"]["text"].as_str().unwrap_or(""), true);
                    if self.done {
                        return;
                    }
                }
                if let Some((index, _)) = self.open.get(&key) {
                    if self.closed.insert(*index) {
                        self.queue.push_back(event(
                            "content_block_stop",
                            json!({"type":"content_block_stop","index":index}),
                        ));
                    }
                }
            }
            "response.output_item.done" => {
                self.output(&value["item"]);
            }
            "response.completed" | "response.incomplete" => {
                // Some compatible services only emit final output items.
                for item in value["response"]["output"].as_array().into_iter().flatten() {
                    self.output(item);
                    if self.done {
                        return;
                    }
                }
                for (key, (_, tool)) in &self.open {
                    if *tool
                        && !self.arguments.get(key).is_some_and(|s| {
                            serde_json::from_str::<Value>(s).is_ok_and(|v| v.is_object())
                        })
                    {
                        self.fail("上游工具参数未完整返回，未完成工具调用");
                        return;
                    }
                }
                for (index, _) in self.open.values() {
                    if self.closed.insert(*index) {
                        self.queue.push_back(event(
                            "content_block_stop",
                            json!({"type":"content_block_stop","index":index}),
                        ));
                    }
                }
                self.queue.push_back(event("message_delta",json!({"type":"message_delta","delta":{"stop_reason":stop(&value["response"],self.tools),"stop_sequence":null},"usage":usage(&value["response"]["usage"])})));
                self.queue
                    .push_back(event("message_stop", json!({"type":"message_stop"})));
                self.done = true;
            }
            "error" | "response.failed" => {
                self.queue.push_back(event("error",json!({"type":"error","error":{"type":"api_error","message":"上游请求失败，请在 Hub 请求日志查看详情"}})));
                self.done = true;
            }
            _ => {}
        }
    }
}
fn stream(body: Body, model: String) -> Body {
    let mut machine = StateMachine::default();
    machine.queue.push_back(event("message_start",json!({"type":"message_start","message":{"id":format!("msg_{}",uuid::Uuid::new_v4()),"type":"message","role":"assistant","model":model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}}})));
    Body::from_stream(futures_util::stream::unfold(
        (body.into_data_stream(), machine),
        |(mut input, mut state)| async move {
            loop {
                if let Some(event) = state.queue.pop_front() {
                    return Some((Ok::<_, std::io::Error>(event), (input, state)));
                }
                if state.done {
                    return None;
                }
                if let Some(index) = [
                    state.pending.windows(2).position(|b| b == b"\n\n"),
                    state.pending.windows(4).position(|b| b == b"\r\n\r\n"),
                ]
                .into_iter()
                .flatten()
                .min()
                {
                    let end = index
                        + if state.pending.get(index) == Some(&b'\r') {
                            4
                        } else {
                            2
                        };
                    let frame = state.pending.drain(..end).collect::<Vec<_>>();
                    let text = String::from_utf8_lossy(&frame);
                    let data = text
                        .lines()
                        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if let Ok(value) = serde_json::from_str(&data) {
                        state.feed(value);
                    }
                    continue;
                }
                match input.next().await {
                    Some(Ok(bytes)) => {
                        state.pending.extend(bytes);
                        if state.pending.len() > 4 * 1024 * 1024 {
                            state.queue.push_back(event("error",json!({"type":"error","error":{"type":"api_error","message":"上游事件过大"}})));
                            state.done = true;
                        }
                    }
                    Some(Err(error)) => {
                        return Some((Err(std::io::Error::other(error)), (input, state)));
                    }
                    None => {
                        state.queue.push_back(event("error",json!({"type":"error","error":{"type":"api_error","message":"上游回复提前中断，请重试当前任务"}})));
                        state.done = true;
                    }
                }
            }
        },
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inbound_preserves_tools_and_results_and_rejects_untranslatable_signatures() {
        let raw = json!({"model":"fixture","max_tokens":1024,"messages":[{"role":"assistant","content":[{"type":"tool_use","id":"tool-a","name":"read","input":{"path":"relative"}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-a","content":"result"}]}],"tools":[{"name":"read","input_schema":{"type":"object"}}]});
        let value = messages_to_responses(&raw).unwrap();
        assert!(value.to_string().contains("tool-a"));
        assert!(value.to_string().contains("relative"));
        assert_eq!(value["max_output_tokens"], 1024);
        assert!(messages_to_responses(&json!({"messages":[{"role":"assistant","content":[{"type":"thinking","signature":"fixture"}]}]})).is_err());
    }
    #[test]
    fn streaming_tool_blocks_keep_identity_and_stop_reason() {
        let mut state = StateMachine::default();
        state.feed(json!({"type":"response.output_item.added","item":{"type":"function_call","id":"item","call_id":"call","name":"read"}}));
        state.feed(
            json!({"type":"response.function_call_arguments.delta","item_id":"item","delta":"{}"}),
        );
        state.feed(json!({"type":"response.completed","response":{"usage":{"input_tokens":100,"output_tokens":8,"input_tokens_details":{"cached_tokens":60}}}}));
        let output = state
            .queue
            .iter()
            .map(|b| String::from_utf8_lossy(b))
            .collect::<Vec<_>>()
            .join("");
        assert!(
            output.contains("call")
                && output.contains("tool_use")
                && output.contains("cache_read_input_tokens\":60")
        );
        assert!(state.done);
    }
    fn events(state: &StateMachine) -> Vec<Value> {
        state
            .queue
            .iter()
            .map(|b| {
                let s = std::str::from_utf8(b).unwrap();
                serde_json::from_str(s.lines().find_map(|l| l.strip_prefix("data: ")).unwrap())
                    .unwrap()
            })
            .collect()
    }
    #[test]
    fn invalid_or_changed_tool_arguments_cannot_complete_a_tool_turn() {
        for final_args in ["{invalid}", "[]", "{\"path\":\"changed\"}"] {
            let mut s = StateMachine::default();
            s.feed(json!({"type":"response.output_item.added","item":{"type":"function_call","id":"i","call_id":"c","name":"read"}}));
            s.feed(json!({"type":"response.function_call_arguments.delta","item_id":"i","delta":"{\"path\":\"original\"}"}));
            s.feed(json!({"type":"response.function_call_arguments.done","item_id":"i","arguments":final_args}));
            s.feed(json!({"type":"response.completed","response":{"output":[]}}));
            let e = events(&s);
            assert!(e.iter().any(|e| e["type"] == "error"));
            assert!(!e.iter().any(|e| e["type"] == "message_stop"));
        }
    }
    #[tokio::test]
    async fn final_only_sse_and_fragmented_utf8_keep_text_tools_and_usage_once() {
        let done = json!({"type":"response.completed","response":{"output":[{"type":"message","id":"m","content":[{"type":"output_text","text":"你好"}]},{"type":"function_call","id":"i","call_id":"call-a","name":"read","arguments":"{\"path\":\"x\"}"}],"usage":{"input_tokens":100,"output_tokens":9,"input_tokens_details":{"cached_tokens":60}}}});
        let bytes = format!("event: response.completed\r\ndata: {done}\r\n\r\n").into_bytes();
        let chunks = bytes
            .chunks(3)
            .map(|b| Ok::<_, std::io::Error>(Bytes::copy_from_slice(b)))
            .collect::<Vec<_>>();
        let body = stream(
            Body::from_stream(futures_util::stream::iter(chunks)),
            "fixture".into(),
        );
        let output = to_bytes(body, 1024 * 1024).await.unwrap();
        let text = String::from_utf8(output.to_vec()).unwrap();
        assert!(text.contains("你好") && text.contains("call-a"));
        assert_eq!(text.matches("event: message_stop").count(), 1);
        assert!(
            text.contains("\"input_tokens\":40") && text.contains("\"cache_read_input_tokens\":60")
        );
    }
    #[test]
    fn repeated_final_events_do_not_duplicate_blocks_or_allow_post_stop_deltas() {
        let mut s = StateMachine::default();
        let item =
            json!({"type":"function_call","id":"i","call_id":"c","name":"read","arguments":"{}"});
        s.feed(json!({"type":"response.output_item.done","item":item}));
        s.feed(json!({"type":"response.completed","response":{"output":[item]}}));
        s.feed(json!({"type":"response.completed","response":{"output":[]}}));
        let e = events(&s);
        assert_eq!(
            e.iter()
                .filter(|e| e["type"] == "content_block_start")
                .count(),
            1
        );
        assert_eq!(
            e.iter()
                .filter(|e| e["type"] == "content_block_stop")
                .count(),
            1
        );
        let mut s = StateMachine::default();
        s.feed(json!({"type":"response.output_text.delta","item_id":"m","delta":"a"}));
        s.feed(json!({"type":"response.content_part.done","item_id":"m","part":{"type":"output_text","text":"a"}}));
        s.feed(json!({"type":"response.output_text.delta","item_id":"m","delta":"b"}));
        assert!(events(&s).iter().any(|e| e["type"] == "error"));
    }
}
