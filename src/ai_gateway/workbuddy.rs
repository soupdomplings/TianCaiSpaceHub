//! WorkBuddy compatibility helpers.
//!
//! WorkBuddy speaks the OpenAI Chat Completions wire format.  Internally the
//! gateway keeps using the Responses-shaped request model so all existing
//! provider routing, cache-key handling, and Anthropic conversion remain
//! shared with the Codex path.

use std::{collections::HashMap, io, pin::Pin};

use axum::{
    body::{Body, Bytes},
    http::{HeaderName, HeaderValue, StatusCode},
    response::Response,
};
use futures_util::{Stream, StreamExt};
use serde_json::{Value, json};

use super::config::{ProviderConfig, provider_api_root};
use super::context::{GatewayContext, apply_upstream_headers};
use super::error::GatewayError;
use super::model::GatewayRequest;
use super::providers::{
    apply_total_request_timeout, ensure_success_response, execute_provider_request,
};
use super::request_log::{self, RequestLogContext, RequestLogUpdate, UpstreamSseCaptureStream};

/// Converts a WorkBuddy Chat Completions request into the internal Responses
/// request shape.  The conversion intentionally only touches fields with
/// stable OpenAI semantics and leaves provider-specific cache fields intact.
pub fn chat_request_to_responses(raw: &Value) -> Result<Value, String> {
    let object = raw
        .as_object()
        .ok_or_else(|| "request body must be a JSON object".to_string())?;
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "model is required".to_string())?;
    let messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "messages must be an array".to_string())?;

    let mut instructions = Vec::new();
    let mut input = Vec::new();
    for message in messages {
        let Some(message_object) = message.as_object() else {
            return Err("messages entries must be objects".to_string());
        };
        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if role == "system" {
            if let Some(text) = chat_content_text(message_object.get("content")) {
                if !text.is_empty() {
                    instructions.push(text);
                }
            }
            continue;
        }

        if role == "tool" {
            let call_id = message_object
                .get("tool_call_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            input.push(json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": chat_content_text(message_object.get("content")).unwrap_or_default(),
            }));
            continue;
        }

        let content = chat_content_parts(message_object.get("content"), role == "assistant")?;
        let mut item = json!({
            "type": "message",
            "role": role,
            "content": content,
        });
        if let Some(name) = message_object.get("name").and_then(Value::as_str) {
            item["name"] = json!(name);
        }
        input.push(item);

        if let Some(tool_calls) = message_object.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                let function = tool_call.get("function").unwrap_or(&Value::Null);
                let name = function.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = function
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!("{}"));
                input.push(json!({
                    "type": "function_call",
                    "call_id": tool_call.get("id").cloned().unwrap_or(Value::Null),
                    "name": name,
                    "arguments": arguments,
                    "status": "completed",
                }));
            }
        }
    }

    let mut converted = json!({
        "model": model,
        "input": input,
        "stream": object.get("stream").and_then(Value::as_bool).unwrap_or(false),
    });
    if !instructions.is_empty() {
        converted["instructions"] = json!(instructions.join("\n\n"));
    }
    if let Some(tools) = object.get("tools").and_then(Value::as_array) {
        converted["tools"] = json!(convert_tools(tools));
    }
    if let Some(tool_choice) = object.get("tool_choice") {
        converted["tool_choice"] = convert_tool_choice(tool_choice);
    }
    if let Some(value) = object.get("temperature") {
        converted["temperature"] = value.clone();
    }
    if let Some(value) = object.get("top_p") {
        converted["top_p"] = value.clone();
    }
    if let Some(value) = object
        .get("max_output_tokens")
        .or_else(|| object.get("max_completion_tokens"))
        .or_else(|| object.get("max_tokens"))
    {
        converted["max_output_tokens"] = value.clone();
    }
    if let Some(effort) = object.get("reasoning_effort").and_then(Value::as_str) {
        converted["reasoning"] = json!({"effort": effort});
    }
    if let Some(response_format) = object.get("response_format") {
        converted["text"] = json!({"format": convert_response_format(response_format)});
    }
    for key in [
        "prompt_cache_key",
        "prompt_cache_retention",
        "prompt_cache_options",
        "previous_response_id",
    ] {
        if let Some(value) = object.get(key) {
            converted[key] = value.clone();
        }
    }

    // Validate the generated shape now so callers get a useful field path.
    serde_json::from_value::<GatewayRequest>(converted.clone())
        .map_err(|error| format!("invalid converted request: {error}"))?;
    Ok(converted)
}

/// Builds the wire shape used by the original standalone WorkBuddy adapter.
///
/// Unlike the internal `GatewayRequest` representation, the OpenAI Responses
/// endpoint accepts ordinary `{role, content}` message items. Preserving this
/// shape matters for compatible gateways that hash the serialized prompt when
/// creating a cache entry. Keep this function limited to the WorkBuddy OpenAI
/// route; Codex's native Responses requests continue to pass through unchanged.
pub fn chat_request_to_openai_responses(raw: &Value) -> Result<Value, String> {
    let object = raw
        .as_object()
        .ok_or_else(|| "request body must be a JSON object".to_string())?;
    let model = object
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "model is required".to_string())?;
    let messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "messages must be an array".to_string())?;

    let mut instructions = Vec::new();
    let mut input = Vec::new();
    for message in messages {
        let Some(message_object) = message.as_object() else {
            return Err("messages entries must be objects".to_string());
        };
        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        if matches!(role, "system" | "developer") {
            if let Some(text) = chat_content_text(message_object.get("content")) {
                if !text.is_empty() {
                    instructions.push(text);
                }
            }
            continue;
        }

        if matches!(role, "tool" | "function") {
            let call_id = message_object
                .get("tool_call_id")
                .or_else(|| message_object.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("");
            input.push(json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": chat_content_text(message_object.get("content")).unwrap_or_default(),
            }));
            continue;
        }

        let content = openai_responses_content(message_object.get("content"), role == "assistant")?;
        if role == "assistant" {
            if let Some(tool_calls) = message_object.get("tool_calls").and_then(Value::as_array) {
                if !content_is_empty(&content) {
                    input.push(json!({"role": role, "content": content}));
                }
                for tool_call in tool_calls {
                    let function = tool_call.get("function").unwrap_or(&Value::Null);
                    let name = function.get("name").and_then(Value::as_str).unwrap_or("");
                    let arguments = function
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!("{}"));
                    input.push(json!({
                        "type": "function_call",
                        "id": tool_call.get("id").cloned().unwrap_or(Value::Null),
                        "call_id": tool_call.get("id").cloned().unwrap_or(Value::Null),
                        "name": name,
                        "arguments": arguments,
                    }));
                }
                continue;
            }
        }

        let output_role = if role == "assistant" || role == "user" {
            role
        } else {
            "user"
        };
        input.push(json!({"role": output_role, "content": content}));
    }

    let mut converted = json!({
        "model": model,
        "input": input,
        "stream": object.get("stream").and_then(Value::as_bool).unwrap_or(false),
        // Match the standalone adapter. This controls response persistence and
        // is independent from prompt-cache key matching.
        "store": object.get("store").and_then(Value::as_bool).unwrap_or(false),
    });
    if !instructions.is_empty() {
        converted["instructions"] = json!(instructions.join("\n\n"));
    }
    if let Some(tools) = object.get("tools").and_then(Value::as_array) {
        converted["tools"] = json!(convert_tools(tools));
    }
    if let Some(tool_choice) = object.get("tool_choice") {
        converted["tool_choice"] = convert_tool_choice(tool_choice);
    }
    for key in [
        "temperature",
        "top_p",
        "parallel_tool_calls",
        "metadata",
        "user",
        "service_tier",
    ] {
        if let Some(value) = object.get(key) {
            converted[key] = value.clone();
        }
    }
    if let Some(value) = object
        .get("max_output_tokens")
        .or_else(|| object.get("max_completion_tokens"))
        .or_else(|| object.get("max_tokens"))
    {
        converted["max_output_tokens"] = value.clone();
    }
    if let Some(effort) = object.get("reasoning_effort").and_then(Value::as_str) {
        converted["reasoning"] = json!({"effort": effort});
    }
    if let Some(response_format) = object.get("response_format") {
        converted["text"] = json!({"format": convert_response_format(response_format)});
    }
    for key in [
        "prompt_cache_key",
        "prompt_cache_retention",
        "prompt_cache_options",
        "previous_response_id",
    ] {
        if let Some(value) = object.get(key) {
            converted[key] = value.clone();
        }
    }
    Ok(converted)
}

fn openai_responses_content(value: Option<&Value>, assistant: bool) -> Result<Value, String> {
    match value {
        None | Some(Value::Null) => Ok(Value::String(String::new())),
        Some(Value::String(text)) => Ok(json!(text)),
        Some(Value::Array(_)) => openai_responses_content_parts(value, assistant),
        Some(value) => Err(format!(
            "message content must be string or array, got {value}"
        )),
    }
}

fn openai_responses_content_parts(value: Option<&Value>, assistant: bool) -> Result<Value, String> {
    let Some(Value::Array(parts)) = value else {
        return Ok(json!(""));
    };
    let mut converted = Vec::new();
    for part in parts {
        let Some(object) = part.as_object() else {
            return Err("message content parts must be objects".to_string());
        };
        match object.get("type").and_then(Value::as_str).unwrap_or("text") {
            "text" => converted.push(json!({
                "type": if assistant { "output_text" } else { "input_text" },
                "text": object.get("text").and_then(Value::as_str).unwrap_or(""),
            })),
            "image_url" => {
                let image = object.get("image_url").unwrap_or(&Value::Null);
                let (url, detail) = if let Some(url) = image.as_str() {
                    (url.to_string(), "auto")
                } else {
                    (
                        image
                            .get("url")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        image
                            .get("detail")
                            .and_then(Value::as_str)
                            .unwrap_or("auto"),
                    )
                };
                converted.push(json!({
                    "type": "input_image",
                    "image_url": url,
                    "detail": detail,
                }));
            }
            "input_text" | "output_text" => converted.push(json!({
                "type": if assistant { "output_text" } else { "input_text" },
                "text": object.get("text").and_then(Value::as_str).unwrap_or(""),
            })),
            "input_image" | "input_file" => converted.push(part.clone()),
            _other => converted.push(json!({
                "type": if assistant { "output_text" } else { "input_text" },
                "text": chat_content_text(Some(&Value::Object(object.clone()))).unwrap_or_default(),
            })),
        }
    }
    Ok(Value::Array(converted))
}

fn content_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(parts) => parts.is_empty(),
        _ => false,
    }
}

pub fn chat_request_model(raw: &Value) -> Result<String, String> {
    raw.get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| "model is required".to_string())
}

pub fn chat_request_stream(raw: &Value) -> bool {
    raw.get("stream").and_then(Value::as_bool).unwrap_or(false)
}

pub fn chat_request_cache_key(raw: &Value) -> Option<String> {
    raw.get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn apply_default_reasoning_effort(raw: &mut Value, default_effort: Option<&str>) {
    let requested = raw
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            raw.get("reasoning")
                .and_then(|reasoning| reasoning.get("effort"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        });
    let effort = requested.or_else(|| {
        default_effort
            .map(str::trim)
            .filter(|value| !value.is_empty())
    });
    if let Some(effort) = effort {
        raw["reasoning_effort"] = json!(effort);
    }
}

/// Proxies a WorkBuddy Chat request directly to a Chat Completions provider.
/// This path is used for providers that natively speak Chat Completions and
/// avoids an unnecessary Responses round trip.
pub async fn proxy_chat_completion(
    client: &reqwest::Client,
    ctx: &GatewayContext,
    mut raw_body: Value,
    _request_model: &str,
    upstream_model: &str,
    provider: &ProviderConfig,
    log_context: Option<RequestLogContext>,
) -> Result<Response<Body>, GatewayError> {
    raw_body["model"] = json!(upstream_model);
    apply_chat_cache_controls(&mut raw_body, ctx, provider);
    super::providers::apply_chat_reasoning_override(&mut raw_body, provider);
    let stream = raw_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let url = format!(
        "{}/v1/chat/completions",
        provider_api_root(&provider.base_url)
    );
    let builder = client
        .post(&url)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", provider.api_key));
    let builder =
        apply_total_request_timeout(builder, provider.timeout_secs, stream).json(&raw_body);
    let request = apply_upstream_headers(builder, &ctx.upstream_headers)
        .build()
        .map_err(|error| {
            GatewayError::upstream(
                StatusCode::BAD_GATEWAY,
                format!("build upstream request: {error}"),
            )
        })?;

    if let Some(log_context) = &log_context {
        let update = RequestLogUpdate {
            upstream_request_headers_json: log_context
                .details_enabled
                .then(|| request_log::headers_to_json(request.headers()))
                .flatten(),
            upstream_request_body_bytes: request_log::json_body_size_bytes(&raw_body),
            upstream_request_json: log_context
                .details_enabled
                .then(|| serde_json::to_string(&raw_body).ok())
                .flatten(),
            ..RequestLogUpdate::default()
        };
        if let Err(error) = log_context.store.update_record(log_context.log_id, &update) {
            request_log::log_update_error(error);
        }
    }

    let upstream =
        execute_provider_request(client, request, provider, "chat upstream request failed").await?;
    let upstream = ensure_success_response(&provider.name, upstream).await?;
    if stream {
        let bytes = upstream.bytes_stream();
        let body = if let Some(log_context) = log_context {
            let captured = UpstreamSseCaptureStream::new(bytes, log_context);
            Body::from_stream(captured)
        } else {
            Body::from_stream(bytes)
        };
        let mut response = Response::new(body);
        *response.status_mut() = StatusCode::OK;
        response.headers_mut().insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("text/event-stream"),
        );
        response.headers_mut().insert(
            HeaderName::from_static("cache-control"),
            HeaderValue::from_static("no-cache"),
        );
        response.headers_mut().insert(
            HeaderName::from_static("connection"),
            HeaderValue::from_static("keep-alive"),
        );
        return Ok(response);
    }

    let headers = upstream.headers().clone();
    let bytes = upstream.bytes().await.map_err(|error| {
        GatewayError::upstream(
            StatusCode::BAD_GATEWAY,
            format!("read upstream response: {error}"),
        )
    })?;
    if let Some(log_context) = &log_context {
        let response_json = serde_json::from_slice::<Value>(&bytes).ok();
        let update = RequestLogUpdate {
            status: Some("completed".to_string()),
            usage: response_json
                .as_ref()
                .map(request_log::usage_from_response_value),
            latency_ms: Some(request_log::elapsed_ms(log_context.started_at)),
            response_json: log_context
                .details_enabled
                .then(|| {
                    response_json
                        .as_ref()
                        .and_then(|value| serde_json::to_string(value).ok())
                })
                .flatten(),
            ..RequestLogUpdate::default()
        };
        if let Err(error) = log_context.store.update_record(log_context.log_id, &update) {
            request_log::log_update_error(error);
        }
    }
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/json"),
    );
    for name in ["x-request-id", "openai-model"] {
        let header = HeaderName::from_static(name);
        if let Some(value) = headers.get(&header) {
            response.headers_mut().insert(header, value.clone());
        }
    }
    Ok(response)
}

fn apply_chat_cache_controls(
    raw_body: &mut Value,
    ctx: &GatewayContext,
    provider: &ProviderConfig,
) {
    // OpenAI-compatible Chat providers can use the same cache controls as
    // Responses. Preserve an explicit request value and fill in the stable
    // key selected by the gateway when WorkBuddy omitted one.
    if raw_body.get("prompt_cache_key").is_none() {
        raw_body["prompt_cache_key"] = json!(ctx.prompt_cache_key);
    }
    if raw_body.get("prompt_cache_retention").is_none()
        && let Some(retention) = &provider.prompt_cache_retention
    {
        raw_body["prompt_cache_retention"] = json!(retention);
    }
}

/// Converts a complete Responses response into a Chat Completions response.
pub fn responses_to_chat(value: &Value, request_model: &str) -> Value {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("chatcmpl_workbuddy");
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(request_model);
    let created = value
        .get("created_at")
        .and_then(Value::as_i64)
        .unwrap_or_else(unix_timestamp);

    let mut content = String::new();
    let mut reasoning_content = String::new();
    let mut tool_calls = Vec::new();
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        for item in output {
            match item.get("type").and_then(Value::as_str).unwrap_or("") {
                "message" => append_message_text(&mut content, item),
                "reasoning" => append_reasoning_text(&mut reasoning_content, item),
                "function_call" | "custom_tool_call" => {
                    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                    let arguments = item
                        .get("arguments")
                        .or_else(|| item.get("input"))
                        .map(stringify_json_or_text)
                        .unwrap_or_else(|| "{}".to_string());
                    let id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("call_workbuddy")
                        .to_string();
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {"name": name, "arguments": arguments},
                    }));
                }
                _ => {}
            }
        }
    }

    let finish_reason = if !tool_calls.is_empty() {
        "tool_calls"
    } else if value.get("status").and_then(Value::as_str) == Some("incomplete") {
        "length"
    } else {
        "stop"
    };
    let mut message = json!({
        "role": "assistant",
        "content": if content.is_empty() { Value::Null } else { json!(content) },
    });
    if !reasoning_content.is_empty() {
        message["reasoning_content"] = json!(reasoning_content);
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = json!(tool_calls);
    }

    let mut response = json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}],
    });
    if let Some(usage) = value.get("usage") {
        response["usage"] = responses_usage_to_chat(usage);
    }
    response
}

fn responses_usage_to_chat(usage: &Value) -> Value {
    let input = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(input + output);
    let mut result = json!({
        "prompt_tokens": input,
        "completion_tokens": output,
        "total_tokens": total,
    });
    if let Some(cached) = usage
        .get("input_tokens_details")
        .and_then(|value| value.get("cached_tokens"))
        .and_then(Value::as_i64)
    {
        result["prompt_tokens_details"] = json!({"cached_tokens": cached});
    }
    if let Some(cache_write) = usage
        .get("input_tokens_details")
        .and_then(|value| value.get("cache_write_tokens"))
        .and_then(Value::as_i64)
    {
        if !result["prompt_tokens_details"].is_object() {
            result["prompt_tokens_details"] = json!({});
        }
        result["prompt_tokens_details"]["cache_write_tokens"] = json!(cache_write);
    }
    if let Some(reasoning) = usage
        .get("output_tokens_details")
        .and_then(|value| value.get("reasoning_tokens"))
        .and_then(Value::as_i64)
    {
        result["completion_tokens_details"] = json!({"reasoning_tokens": reasoning});
    }
    result
}

fn append_message_text(output: &mut String, item: &Value) {
    if let Some(content) = item.get("content") {
        match content {
            Value::String(text) => output.push_str(text),
            Value::Array(parts) => {
                for part in parts {
                    if matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("output_text" | "text")
                    ) {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            output.push_str(text);
                        }
                    }
                }
            }
            _ => {}
        }
    } else if let Some(text) = item.get("text").and_then(Value::as_str) {
        output.push_str(text);
    }
}

fn append_reasoning_text(output: &mut String, item: &Value) {
    if let Some(summary) = item.get("summary").and_then(Value::as_array) {
        for part in summary {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                output.push_str(text);
            }
        }
    }
}

fn stringify_json_or_text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()))
}

fn chat_content_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(""),
        ),
        Value::Null => Some(String::new()),
        value => Some(stringify_json_or_text(value)),
    }
}

fn chat_content_parts(value: Option<&Value>, assistant: bool) -> Result<Value, String> {
    let text_type = if assistant {
        "output_text"
    } else {
        "input_text"
    };
    match value {
        None | Some(Value::Null) => Ok(json!([])),
        Some(Value::String(text)) => Ok(json!([{"type": text_type, "text": text}])),
        Some(Value::Array(parts)) => {
            let mut converted = Vec::new();
            for part in parts {
                let Some(object) = part.as_object() else {
                    return Err("message content parts must be objects".to_string());
                };
                match object.get("type").and_then(Value::as_str).unwrap_or("text") {
                    "text" | "input_text" | "output_text" => {
                        let mut text_part = json!({
                            "type": text_type,
                            "text": object.get("text").and_then(Value::as_str).unwrap_or(""),
                        });
                        if let Some(breakpoint) = object.get("prompt_cache_breakpoint") {
                            text_part["prompt_cache_breakpoint"] = breakpoint.clone();
                        }
                        converted.push(text_part);
                    }
                    "image_url" | "input_image" => {
                        let image = object.get("image_url").unwrap_or(&Value::Null);
                        let (url, detail) = if let Some(url) = image.as_str() {
                            (url.to_string(), None)
                        } else {
                            (
                                image
                                    .get("url")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                image.get("detail").and_then(Value::as_str),
                            )
                        };
                        let mut item = json!({"type": "input_image", "image_url": url});
                        if let Some(detail) = detail {
                            item["detail"] = json!(detail);
                        }
                        converted.push(item);
                    }
                    other => return Err(format!("unsupported message content type: {other}")),
                }
            }
            Ok(json!(converted))
        }
        Some(value) => Err(format!(
            "message content must be string or array, got {value}"
        )),
    }
}

fn convert_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let function = tool.get("function").unwrap_or(tool);
            let name = function.get("name")?.as_str()?;
            let mut result = json!({"type": "function", "name": name});
            for key in ["description", "parameters", "strict"] {
                if let Some(value) = function.get(key) {
                    result[key] = value.clone();
                }
            }
            Some(result)
        })
        .collect()
}

fn convert_tool_choice(value: &Value) -> Value {
    if let Some(object) = value.as_object() {
        if object.get("type").and_then(Value::as_str) == Some("function") {
            if let Some(name) = object
                .get("function")
                .and_then(|function| function.get("name"))
            {
                return json!({"type": "function", "name": name});
            }
        }
    }
    value.clone()
}

fn convert_response_format(value: &Value) -> Value {
    match value.get("type").and_then(Value::as_str) {
        Some("json_schema") => {
            // Chat Completions nests the schema under `json_schema`, while
            // Responses expects its name/schema fields at the format level.
            if let Some(schema) = value.get("json_schema") {
                let mut converted = schema.clone();
                if let Some(object) = converted.as_object_mut() {
                    object.insert("type".to_string(), json!("json_schema"));
                }
                converted
            } else {
                value.clone()
            }
        }
        Some("json_object") => json!({"type": "json_object"}),
        _ => value.clone(),
    }
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Converts Responses SSE into the Chat Completions SSE format expected by
/// WorkBuddy.  The converter is deliberately stateful because Responses emits
/// tool arguments in separate events.
pub fn responses_sse_to_chat(body: Body, request_model: String) -> Body {
    let input = body
        .into_data_stream()
        .map(|result| result.map_err(|error| io::Error::other(error.to_string())))
        .boxed();
    let state = ChatSseState::new(request_model);
    let output =
        futures_util::stream::unfold((input, state), |(mut input, mut state)| async move {
            match state.next_item(&mut input).await {
                Some(item) => Some((item, (input, state))),
                None => None,
            }
        });
    Body::from_stream(output)
}

type InputStream = Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>>;

struct ChatSseState {
    request_model: String,
    buffer: String,
    queue: std::collections::VecDeque<Bytes>,
    response_id: String,
    model: String,
    created: i64,
    started: bool,
    finished: bool,
    done_emitted: bool,
    tools: HashMap<String, usize>,
    next_tool_index: usize,
}

impl ChatSseState {
    fn new(request_model: String) -> Self {
        Self {
            request_model,
            buffer: String::new(),
            queue: std::collections::VecDeque::new(),
            response_id: "chatcmpl_workbuddy".to_string(),
            model: String::new(),
            created: unix_timestamp(),
            started: false,
            finished: false,
            done_emitted: false,
            tools: HashMap::new(),
            next_tool_index: 0,
        }
    }

    async fn next_item(&mut self, input: &mut InputStream) -> Option<Result<Bytes, io::Error>> {
        loop {
            if let Some(item) = self.queue.pop_front() {
                return Some(Ok(item));
            }
            if self.finished {
                return None;
            }
            match input.next().await {
                Some(Ok(chunk)) => {
                    self.buffer.push_str(&String::from_utf8_lossy(&chunk));
                    self.process_lines();
                }
                Some(Err(error)) => return Some(Err(error)),
                None => {
                    if !self.buffer.is_empty() {
                        let line = std::mem::take(&mut self.buffer);
                        self.process_line(line.trim_end_matches('\r'));
                    }
                    self.finish_stream();
                }
            }
        }
    }

    fn process_lines(&mut self) {
        while let Some(position) = self.buffer.find('\n') {
            let line = self.buffer[..position].trim_end_matches('\r').to_string();
            self.buffer = self.buffer[position + 1..].to_string();
            self.process_line(&line);
        }
    }

    fn process_line(&mut self, line: &str) {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            return;
        };
        if data == "[DONE]" {
            self.finish_stream();
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return;
        };
        self.process_event(&value);
    }

    fn process_event(&mut self, value: &Value) {
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        if !self.started {
            self.started = true;
            if let Some(response) = value.get("response") {
                self.response_id = response
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.response_id)
                    .to_string();
                self.model = response
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.request_model)
                    .to_string();
                self.created = response
                    .get("created_at")
                    .and_then(Value::as_i64)
                    .unwrap_or(self.created);
            }
            self.emit(json!({
                "id": self.response_id,
                "object": "chat.completion.chunk",
                "created": self.created,
                "model": if self.model.is_empty() { &self.request_model } else { &self.model },
                "choices": [{"index": 0, "delta": {"role": "assistant", "content": null}, "finish_reason": null}],
            }));
        }

        match event_type {
            "response.output_text.delta" | "response.reasoning_summary_text.delta" => {
                if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                    let mut chunk = json!({"index": 0, "delta": {}});
                    if event_type == "response.output_text.delta" {
                        chunk["delta"]["content"] = json!(delta);
                    } else {
                        chunk["delta"]["reasoning_content"] = json!(delta);
                    }
                    let chunk = self.chunk(chunk);
                    self.emit(chunk);
                }
            }
            "response.output_item.added" => {
                let item = value.get("item").unwrap_or(&Value::Null);
                if matches!(
                    item.get("type").and_then(Value::as_str),
                    Some("function_call" | "custom_tool_call")
                ) {
                    let key = item
                        .get("id")
                        .or_else(|| item.get("call_id"))
                        .and_then(Value::as_str)
                        .unwrap_or("call_workbuddy")
                        .to_string();
                    let index = self.next_tool_index;
                    self.next_tool_index += 1;
                    self.tools.insert(key.clone(), index);
                    let chunk = self.chunk(json!({
                        "index": 0,
                        "delta": {"tool_calls": [{"index": index, "id": key, "type": "function", "function": {"name": item.get("name").and_then(Value::as_str).unwrap_or("")}}]},
                    }));
                    self.emit(chunk);
                }
            }
            "response.function_call_arguments.delta" => {
                let key = value
                    .get("item_id")
                    .or_else(|| value.get("call_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("call_workbuddy");
                let index = *self.tools.entry(key.to_string()).or_insert_with(|| {
                    let index = self.next_tool_index;
                    self.next_tool_index += 1;
                    index
                });
                if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                    let chunk = self.chunk(json!({
                        "index": 0,
                        "delta": {"tool_calls": [{"index": index, "function": {"arguments": delta}}]},
                    }));
                    self.emit(chunk);
                }
            }
            "response.completed" => {
                let response = value.get("response").unwrap_or(value);
                let finish_reason = if self.tools.is_empty() {
                    "stop"
                } else {
                    "tool_calls"
                };
                let mut chunk = self.chunk(json!({
                    "index": 0,
                    "delta": {},
                    "finish_reason": finish_reason,
                }));
                if let Some(usage) = response.get("usage") {
                    chunk["usage"] = responses_usage_to_chat(usage);
                }
                self.emit(chunk);
                self.finish_stream();
            }
            _ => {}
        }
    }

    fn chunk(&self, choice: Value) -> Value {
        json!({
            "id": self.response_id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": if self.model.is_empty() {
                &self.request_model
            } else {
                &self.model
            },
            "choices": [choice],
        })
    }

    fn emit(&mut self, value: Value) {
        if let Ok(json) = serde_json::to_string(&value) {
            self.queue
                .push_back(Bytes::from(format!("data: {json}\n\n")));
        }
    }

    fn finish_stream(&mut self) {
        if self.finished {
            return;
        }
        if !self.started {
            self.started = true;
            let chunk = self.chunk(json!({
                "index": 0,
                "delta": {"role": "assistant", "content": null},
                "finish_reason": null,
            }));
            self.emit(chunk);
        }
        if !self.done_emitted {
            self.queue
                .push_back(Bytes::from_static(b"data: [DONE]\n\n"));
            self.done_emitted = true;
        }
        self.finished = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_responses_wire_preserves_standalone_cache_prefix_shape() {
        let raw = json!({
            "model": "gpt-5.6-sol",
            "messages": [
                {"role": "system", "content": "stable system prompt"},
                {"role": "developer", "content": "stable developer prompt"},
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "previous"}
            ]
        });
        let converted = chat_request_to_openai_responses(&raw).unwrap();
        assert_eq!(converted["store"], false);
        assert_eq!(
            converted["instructions"],
            "stable system prompt\n\nstable developer prompt"
        );
        assert_eq!(converted["input"][0]["role"], "user");
        assert_eq!(converted["input"][0]["content"], "hello");
        assert_eq!(converted["input"][1]["role"], "assistant");
        assert_eq!(converted["input"][1]["content"], "previous");
        assert!(converted["input"][0].get("type").is_none());
    }

    #[test]
    fn openai_responses_wire_keeps_tool_history_compatible() {
        let raw = json!({
            "model": "gpt-5.6-sol",
            "messages": [
                {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call-1", "type": "function",
                    "function": {"name": "lookup", "arguments": "{\"q\":\"x\"}"}
                }]},
                {"role": "tool", "tool_call_id": "call-1", "content": "result"}
            ]
        });
        let converted = chat_request_to_openai_responses(&raw).unwrap();
        assert_eq!(converted["input"][0]["type"], "function_call");
        assert_eq!(converted["input"][0]["call_id"], "call-1");
        assert_eq!(converted["input"][1]["type"], "function_call_output");
        assert_eq!(converted["input"][1]["call_id"], "call-1");
    }

    #[test]
    fn openai_responses_wire_uses_output_text_for_assistant_parts() {
        let raw = json!({
            "model": "gpt-5.6-sol",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hello"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "previous"}]},
                {"role": "assistant", "content": [{"type": "input_text", "text": "older"}]}
            ]
        });
        let converted = chat_request_to_openai_responses(&raw).unwrap();
        assert_eq!(converted["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(converted["input"][1]["content"][0]["type"], "output_text");
        assert_eq!(converted["input"][2]["content"][0]["type"], "output_text");
    }

    #[test]
    fn chat_history_uses_output_text_for_assistant() {
        let raw = json!({
            "model": "gpt-5.6-sol",
            "messages": [
                {"role": "system", "content": "system"},
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "previous"}
            ]
        });
        let converted = chat_request_to_responses(&raw).unwrap();
        assert_eq!(converted["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(converted["input"][1]["content"][0]["type"], "output_text");
    }

    #[test]
    fn assistant_content_parts_use_output_text_even_when_already_structured() {
        let raw = json!({
            "model": "gpt-5.6-sol",
            "messages": [
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": [{"type": "input_text", "text": "previous"}]}
            ]
        });
        let converted = chat_request_to_responses(&raw).unwrap();
        assert_eq!(converted["input"][1]["role"], "assistant");
        assert_eq!(converted["input"][1]["content"][0]["type"], "output_text");
    }

    #[test]
    fn response_conversion_preserves_cache_usage() {
        let response = json!({
            "id": "resp_1", "model": "gpt-5.6-sol", "created_at": 1, "status": "completed",
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "ok"}]}],
            "usage": {"input_tokens": 10, "output_tokens": 2, "total_tokens": 12, "input_tokens_details": {"cached_tokens": 8, "cache_write_tokens": 2}}
        });
        let converted = responses_to_chat(&response, "gpt-5.6-sol");
        assert_eq!(converted["choices"][0]["message"]["content"], "ok");
        assert_eq!(
            converted["usage"]["prompt_tokens_details"]["cached_tokens"],
            8
        );
        assert_eq!(
            converted["usage"]["prompt_tokens_details"]["cache_write_tokens"],
            2
        );
    }

    #[test]
    fn chat_request_preserves_reasoning_cache_and_tool_history() {
        let raw = json!({
            "model": "gpt-5.6-sol",
            "reasoning_effort": "high",
            "prompt_cache_key": "session-1",
            "messages": [
                {"role": "assistant", "content": "calling", "tool_calls": [{
                    "id": "call-1", "type": "function",
                    "function": {"name": "lookup", "arguments": "{\"q\":\"x\"}"}
                }]},
                {"role": "tool", "tool_call_id": "call-1", "content": "result"}
            ],
            "response_format": {"type": "json_schema", "json_schema": {
                "name": "answer", "schema": {"type": "object"}
            }}
        });
        let converted = chat_request_to_responses(&raw).unwrap();
        assert_eq!(converted["reasoning"]["effort"], "high");
        assert_eq!(converted["prompt_cache_key"], "session-1");
        assert_eq!(converted["input"][1]["type"], "function_call");
        assert_eq!(converted["input"][2]["type"], "function_call_output");
        assert_eq!(converted["text"]["format"]["type"], "json_schema");
        assert_eq!(converted["text"]["format"]["name"], "answer");
    }

    #[test]
    fn chat_request_preserves_prompt_cache_options() {
        let raw = json!({
            "model": "gpt-5.6-sol",
            "prompt_cache_options": {"mode": "explicit", "ttl": "30m"},
            "messages": [{"role": "user", "content": [{
                "type": "text",
                "text": "hello",
                "prompt_cache_breakpoint": {"mode": "explicit"}
            }]}]
        });
        let converted = chat_request_to_responses(&raw).unwrap();
        assert_eq!(converted["prompt_cache_options"]["mode"], "explicit");
        assert_eq!(converted["prompt_cache_options"]["ttl"], "30m");
        assert_eq!(
            converted["input"][0]["content"][0]["prompt_cache_breakpoint"]["mode"],
            "explicit"
        );
    }

    #[test]
    fn default_reasoning_effort_fills_missing_and_preserves_selection() {
        let mut missing = json!({"model": "gpt-5.6-sol"});
        apply_default_reasoning_effort(&mut missing, Some("high"));
        assert_eq!(missing["reasoning_effort"], "high");

        let mut selected = json!({"reasoning_effort": "xhigh"});
        apply_default_reasoning_effort(&mut selected, Some("high"));
        assert_eq!(selected["reasoning_effort"], "xhigh");

        let mut responses_style = json!({"reasoning": {"effort": "medium"}});
        apply_default_reasoning_effort(&mut responses_style, Some("high"));
        assert_eq!(responses_style["reasoning_effort"], "medium");
    }

    #[test]
    fn responses_stream_emits_standard_chat_completion_chunks() {
        let mut state = ChatSseState::new("gpt-5.6-sol".to_string());
        state.process_event(&json!({
            "type": "response.created",
            "response": {
                "id": "resp_stream_1",
                "model": "gpt-5.6-sol",
                "created_at": 123
            }
        }));
        state.process_event(&json!({
            "type": "response.output_text.delta",
            "delta": "OK"
        }));
        state.process_event(&json!({
            "type": "response.completed",
            "response": {
                "usage": {
                    "input_tokens": 4096,
                    "output_tokens": 1,
                    "total_tokens": 4097,
                    "input_tokens_details": {"cached_tokens": 3072, "cache_write_tokens": 1024}
                }
            }
        }));

        let frames: Vec<Value> = state
            .queue
            .iter()
            .filter_map(|bytes| {
                let text = std::str::from_utf8(bytes).unwrap();
                let data = text.trim().strip_prefix("data: ").unwrap();
                (data != "[DONE]").then(|| serde_json::from_str(data).unwrap())
            })
            .collect();

        assert_eq!(frames.len(), 3);
        for frame in &frames {
            assert_eq!(frame["object"], "chat.completion.chunk");
            assert!(frame["choices"].is_array());
            assert_eq!(frame["choices"].as_array().unwrap().len(), 1);
            assert!(frame["choices"][0]["delta"].is_object());
        }
        assert_eq!(frames[1]["choices"][0]["delta"]["content"], "OK");
        assert_eq!(frames[2]["choices"][0]["finish_reason"], "stop");
        assert_eq!(
            frames[2]["usage"]["prompt_tokens_details"]["cached_tokens"],
            3072
        );
        assert_eq!(
            frames[2]["usage"]["prompt_tokens_details"]["cache_write_tokens"],
            1024
        );
    }

    #[test]
    fn chat_cache_controls_fill_missing_values_and_preserve_explicit_key() {
        let ctx = GatewayContext {
            request_id: "request-1".to_string(),
            session_id: None,
            thread_id: None,
            window_id: None,
            prompt_cache_key: "stable-session".to_string(),
            upstream_headers: Default::default(),
        };
        let provider = ProviderConfig {
            prompt_cache_retention: Some("24h".to_string()),
            ..ProviderConfig::default()
        };
        let mut generated = json!({"model": "gpt-5.6-sol"});
        apply_chat_cache_controls(&mut generated, &ctx, &provider);
        assert_eq!(generated["prompt_cache_key"], "stable-session");
        assert_eq!(generated["prompt_cache_retention"], "24h");

        let mut explicit = json!({"prompt_cache_key": "request-key"});
        apply_chat_cache_controls(&mut explicit, &ctx, &provider);
        assert_eq!(explicit["prompt_cache_key"], "request-key");
    }
}
