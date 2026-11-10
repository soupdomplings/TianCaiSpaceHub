//! OpenAI-compatible / legacy DeepSeek Chat Completions 出站 provider。
//! 参考 AxonHub `deepseek/outbound.go`。

use axum::{
    body::Body,
    http::{HeaderName, HeaderValue, StatusCode},
    response::Response,
};
use serde_json::Value;
use tracing::{debug, error};

use crate::ai_gateway::config::{ProviderConfig, provider_endpoint};
use crate::ai_gateway::context::{GatewayContext, apply_upstream_headers};
use crate::ai_gateway::error::GatewayError;
use crate::ai_gateway::gmclaw_stream;
use crate::ai_gateway::model::GatewayRequest;
use crate::ai_gateway::request_log::{
    self, RequestLogContext, RequestLogUpdate, ResponsesSseLogStream, UpstreamSseCaptureStream,
};
use crate::ai_gateway::tool_names::ToolNameMap;
use crate::ai_gateway::transform::chat_to_responses::convert_chat_response_with_tool_names;
use crate::ai_gateway::transform::responses_stream::ChatSseToResponsesSse;
use crate::ai_gateway::transform::responses_to_chat::build_chat_request_with_tool_names;

use super::{apply_total_request_timeout, ensure_success_response, execute_context_request};

/// DeepSeek Chat Completions 出站处理。
pub async fn handle(
    client: &reqwest::Client,
    ctx: &GatewayContext,
    request: &GatewayRequest,
    response_model: &str,
    provider: &ProviderConfig,
    log_context: Option<RequestLogContext>,
) -> Result<Response<Body>, GatewayError> {
    // 1. Responses → Chat Completions 请求转换
    // Existing channels retain DeepSeek compatibility; new OpenAI Chat channels
    // opt into standard reasoning_effort and completion-token parameters.
    let openai_chat = provider.compatibility.as_deref() == Some("openai_chat");
    let (mut chat_body, tool_name_map) = build_chat_request_with_tool_names(request, !openai_chat)
        .map_err(|e| GatewayError::bad_request(format!("transform error: {e}")))?;
    if openai_chat {
        if let Some(max_tokens) = chat_body
            .as_object_mut()
            .and_then(|body| body.remove("max_tokens"))
        {
            chat_body["max_completion_tokens"] = max_tokens;
        }
        chat_body["prompt_cache_key"] = serde_json::json!(ctx.prompt_cache_key);
    }

    super::apply_chat_reasoning_override(&mut chat_body, provider);

    let native = crate::desktop::tiangong_models::parameters_for_provider(provider);
    let collect_stream = !request.stream
        && (native.as_ref().is_some_and(|p| p.stream)
            || (native.is_none()
                && openai_chat
                && crate::desktop::models::prefers_upstream_stream(provider)));
    let unary_to_stream = request.stream && native.as_ref().is_some_and(|p| !p.stream);
    if let Some(native) = &native {
        chat_body["stream"] = native.stream.into();
        if !native.stream {
            chat_body.as_object_mut().unwrap().remove("stream_options");
        }
    }
    let deadline = (collect_stream || native.is_some())
        .then(|| gmclaw_stream::deadline(provider.timeout_secs));
    let deadline_context = deadline.map(|d| ctx.clone().with_deadline(d));
    let ctx = deadline_context.as_ref().unwrap_or(ctx);
    if collect_stream {
        chat_body["stream"] = true.into();
        chat_body["stream_options"] = serde_json::json!({"include_usage":true});
    }

    let url = provider_endpoint(&provider.base_url, "/v1/chat/completions");

    debug!(url = %url, stream = request.stream, "proxying to deepseek chat");

    // 2. 发送上游请求
    let req_builder = client
        .post(&url)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", provider.api_key));
    let req_builder = apply_total_request_timeout(
        req_builder,
        provider.timeout_secs,
        request.stream || collect_stream,
    )
    .json(&chat_body);
    let upstream_req = apply_upstream_headers(req_builder, &ctx.upstream_headers)
        .build()
        .map_err(|e| {
            error!(error = %e, "build deepseek upstream request failed");
            GatewayError::upstream(
                StatusCode::BAD_GATEWAY,
                format!("build upstream request: {e}"),
            )
        })?;

    if let Some(log_context) = &log_context {
        let update = RequestLogUpdate {
            upstream_stream: Some(
                chat_body
                    .get("stream")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            upstream_request_headers_json: log_context
                .details_enabled
                .then(|| request_log::headers_to_json(upstream_req.headers()))
                .flatten(),
            upstream_request_body_bytes: request_log::json_body_size_bytes(&chat_body),
            upstream_request_json: log_context
                .details_enabled
                .then(|| serde_json::to_string(&chat_body).ok())
                .flatten(),
            ..RequestLogUpdate::default()
        };
        if let Err(err) = log_context.store.update_record(log_context.log_id, &update) {
            request_log::log_update_error(err);
        }
    }

    let upstream_resp = execute_context_request(
        client,
        upstream_req,
        provider,
        ctx,
        "deepseek upstream request failed",
    )
    .await?;

    request_log::record_upstream_response_headers(log_context.as_ref(), upstream_resp.headers());
    let upstream_resp = ensure_success_response(&provider.name, upstream_resp).await?;

    // 3. 流式 vs 非流式
    if collect_stream {
        let headers = upstream_resp.headers().clone();
        let value = if let Some(log) = log_context.clone() {
            gmclaw_stream::collect(
                UpstreamSseCaptureStream::new(upstream_resp.bytes_stream(), log),
                &headers,
                gmclaw_stream::Protocol::Chat,
                &provider.name,
                deadline.unwrap(),
            )
            .await?
        } else {
            gmclaw_stream::collect(
                upstream_resp.bytes_stream(),
                &headers,
                gmclaw_stream::Protocol::Chat,
                &provider.name,
                deadline.unwrap(),
            )
            .await?
        };
        handle_chat_json(value, response_model, tool_name_map, log_context)
    } else if unary_to_stream {
        let headers = upstream_resp.headers().clone();
        let value = gmclaw_stream::collect(
            upstream_resp.bytes_stream(),
            &headers,
            gmclaw_stream::Protocol::Chat,
            &provider.name,
            deadline.unwrap(),
        )
        .await?;
        let chunks = super::chat_json_stream::chunks(&value)?
            .into_iter()
            .map(Ok::<_, std::io::Error>);
        let stream = ChatSseToResponsesSse::new_with_tool_names(
            futures_util::stream::iter(chunks),
            response_model.to_string(),
            tool_name_map,
        );
        let body = if let Some(log) = log_context {
            Body::from_stream(ResponsesSseLogStream::new(stream, log))
        } else {
            Body::from_stream(stream)
        };
        Ok(Response::builder()
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .body(body)
            .unwrap())
    } else if request.stream {
        handle_stream(upstream_resp, response_model, tool_name_map, log_context).await
    } else {
        handle_non_stream(upstream_resp, response_model, tool_name_map, log_context).await
    }
}

/// 非流式：Chat JSON → Responses JSON。
async fn handle_non_stream(
    resp: reqwest::Response,
    model: &str,
    tool_name_map: ToolNameMap,
    log_context: Option<RequestLogContext>,
) -> Result<Response<Body>, GatewayError> {
    let chat_resp: serde_json::Value = resp.json().await.map_err(|e| {
        GatewayError::upstream(StatusCode::BAD_GATEWAY, format!("parse upstream json: {e}"))
    })?;
    handle_chat_json(chat_resp, model, tool_name_map, log_context)
}
fn handle_chat_json(
    chat_resp: Value,
    model: &str,
    tool_name_map: ToolNameMap,
    log_context: Option<RequestLogContext>,
) -> Result<Response<Body>, GatewayError> {
    let response_obj = convert_chat_response_with_tool_names(&chat_resp, model, &tool_name_map)
        .map_err(|e| {
            GatewayError::upstream(StatusCode::BAD_GATEWAY, format!("transform error: {e}"))
        })?;

    let body_bytes = serde_json::to_vec(&response_obj).unwrap_or_default();
    if let Some(log_context) = &log_context {
        let response_value = serde_json::to_value(&response_obj).unwrap_or_default();
        let update = RequestLogUpdate {
            status: Some(response_obj.status.clone()),
            usage: Some(request_log::usage_from_response_value(&response_value)),
            latency_ms: Some(request_log::elapsed_ms(log_context.started_at)),
            response_json: log_context
                .details_enabled
                .then(|| serde_json::to_string(&response_value).ok())
                .flatten(),
            ..RequestLogUpdate::default()
        };
        if let Err(err) = log_context.store.update_record(log_context.log_id, &update) {
            request_log::log_update_error(err);
        }
    }

    let mut response = Response::new(Body::from(body_bytes));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/json"),
    );
    Ok(response)
}

/// 流式：Chat SSE → Responses SSE（通过状态机转换）。
async fn handle_stream(
    resp: reqwest::Response,
    model: &str,
    tool_name_map: ToolNameMap,
    log_context: Option<RequestLogContext>,
) -> Result<Response<Body>, GatewayError> {
    let model = model.to_string();
    let upstream_bytes = resp.bytes_stream();
    let body = if let Some(log_context) = log_context {
        let captured_upstream = UpstreamSseCaptureStream::new(upstream_bytes, log_context.clone());
        let sse_stream =
            ChatSseToResponsesSse::new_with_tool_names(captured_upstream, model, tool_name_map);
        Body::from_stream(ResponsesSseLogStream::new(sse_stream, log_context))
    } else {
        let sse_stream =
            ChatSseToResponsesSse::new_with_tool_names(upstream_bytes, model, tool_name_map);
        Body::from_stream(sse_stream)
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
    Ok(response)
}
