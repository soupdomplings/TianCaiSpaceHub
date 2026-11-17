use std::collections::HashSet;

use anyhow::{Result, bail, ensure};
use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::Response,
};
use serde_json::{Value, json};

use super::{
    NvwaService,
    config::resolve_mcp_url,
    types::{AuthResult, ClientKind, NvwaProfile},
};

pub(crate) const PROTOCOLS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const MAX_REMOTE_BYTES: usize = 8 * 1024 * 1024;
const RESPONSE_SIZE_ERROR_CODE: i64 = -32010;

#[derive(Debug, thiserror::Error)]
#[error("NVWA response exceeds size limit; result unconfirmed")]
struct ResponseTooLarge;

fn is_size_limit_response(status: StatusCode, request: &Value, reply: &Value) -> bool {
    status == StatusCode::BAD_GATEWAY
        && reply["jsonrpc"] == "2.0"
        && request.get("id").is_some_and(|id| !id.is_null())
        && reply.get("id") == request.get("id")
        && reply.get("result").is_none()
        && reply.pointer("/error/code").and_then(Value::as_i64) == Some(RESPONSE_SIZE_ERROR_CODE)
}

#[cfg(test)]
mod catalog_limit_tests {
    use super::*;

    #[test]
    fn local_size_limit_is_recognized_only_for_its_exact_rpc_response() {
        let request = json!({"jsonrpc":"2.0","id":"fixture-list","method":"tools/list"});
        let reply =
            json!({"jsonrpc":"2.0","id":"fixture-list","error":{"code":RESPONSE_SIZE_ERROR_CODE}});
        assert!(is_size_limit_response(
            StatusCode::BAD_GATEWAY,
            &request,
            &reply
        ));
        for status in [
            StatusCode::OK,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
        ] {
            assert!(!is_size_limit_response(status, &request, &reply));
        }
        let mut changed = reply.clone();
        changed["id"] = json!("other-request");
        assert!(!is_size_limit_response(
            StatusCode::BAD_GATEWAY,
            &request,
            &changed
        ));
        changed = reply.clone();
        changed["error"]["code"] = json!(-32000);
        assert!(!is_size_limit_response(
            StatusCode::BAD_GATEWAY,
            &request,
            &changed
        ));
        changed = reply;
        changed["result"] = json!({});
        assert!(!is_size_limit_response(
            StatusCode::BAD_GATEWAY,
            &request,
            &changed
        ));
    }
}

// Only fixed diagnostics cross the local bridge. Never forward an upstream
// response body or reqwest's URL-bearing error text into the UI or logs.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RemoteFailure {
    #[error("Dumpling MCP 登录已过期，请重新登录当前环境后再连接")]
    LoginExpired,
    #[error("连接 Dumpling MCP 超时，请检查服务地址、网络和代理")]
    Timeout,
    #[error("无法连接 Dumpling MCP，请检查服务地址、DNS、证书和代理")]
    Connect,
    #[error("Dumpling MCP 网络请求失败，请检查网络和代理")]
    Network,
    #[error("无法准备 Dumpling MCP 网络连接，请检查 Hub 的连接与网络设置")]
    NetworkSettings,
    #[error("Dumpling MCP 响应中断，结果尚未确认；未自动重试")]
    Interrupted,
    #[error("Dumpling MCP 服务返回 HTTP {status}：{}", upstream_http_hint(*status))]
    Http { status: u16 },
    #[error("Dumpling MCP 返回了远端会话，当前接入要求服务端使用无状态 JSON 模式")]
    RemoteSession,
    #[error("Dumpling MCP 返回格式不兼容，当前接入要求 application/json，暂不支持 SSE")]
    ContentType,
    #[error("Dumpling MCP 响应不是有效 JSON，请检查 MCP 地址是否指向实际服务接口")]
    InvalidJson,
    #[error("Dumpling MCP 响应不符合 JSON-RPC 协议，请检查服务端 MCP 接口")]
    InvalidRpc,
    #[error("Dumpling MCP 响应的请求编号不一致，请检查服务端或反向代理")]
    ResponseId,
    #[error("Dumpling MCP 初始化响应不兼容，请检查服务端协议版本和工具能力")]
    Initialize,
    #[error("Dumpling MCP 返回了非预期响应，请检查服务端 MCP 接口")]
    Unexpected,
    #[error("Dumpling MCP 请求未能取得确定结果；未自动重试")]
    Unconfirmed,
}

fn upstream_http_hint(status: u16) -> &'static str {
    match status {
        301..=308 => "接口发生重定向，请填写最终 MCP 地址",
        401 => "当前登录凭据未被接受，请重新登录",
        403 => "访问被拒绝，请检查账号的 MCP 权限及网关访问规则",
        404 => "未找到 MCP 接口，请核对服务地址和 MCP 路径",
        407 => "代理要求认证，请检查代理设置",
        429 => "请求受到限流，请稍后重试",
        500..=599 => "上游服务或网关异常，请检查服务端状态和代理",
        _ => "请检查服务地址、接口状态及网关访问规则",
    }
}

impl RemoteFailure {
    fn kind(&self) -> &'static str {
        match self {
            Self::LoginExpired => "login_expired",
            Self::Timeout => "timeout",
            Self::Connect => "connect",
            Self::Network => "network",
            Self::NetworkSettings => "network_settings",
            Self::Interrupted => "interrupted",
            Self::Http { .. } => "upstream_http",
            Self::RemoteSession => "remote_session",
            Self::ContentType => "content_type",
            Self::InvalidJson => "invalid_json",
            Self::InvalidRpc => "invalid_rpc",
            Self::ResponseId => "response_id",
            Self::Initialize => "initialize",
            Self::Unexpected => "unexpected",
            Self::Unconfirmed => "unconfirmed",
        }
    }
}

fn request_phase(method: &str) -> &'static str {
    match method {
        "initialize" => "initialize",
        "notifications/initialized" => "initialized",
        "tools/list" => "tools_list",
        "tools/call" => "tools_call",
        _ => "request",
    }
}

fn remote_failure_response(
    id: Option<Value>,
    failure: &RemoteFailure,
    method: &str,
    client: ClientKind,
) -> Response {
    let upstream_status = match failure {
        RemoteFailure::Http { status } => Some(*status),
        RemoteFailure::LoginExpired => Some(401),
        _ => None,
    };
    tracing::warn!(
        target: "codexhub::nvwa",
        event = "mcp_bridge_failure",
        client = client.as_str(),
        phase = request_phase(method),
        failure = failure.kind(),
        upstream_status,
        "Dumpling MCP connection failed"
    );
    let mut data = json!({"bridgeFailure": failure});
    if method == "tools/call" {
        data["outcomeUnknown"] = true.into();
        data["retryable"] = false.into();
    }
    response(
        StatusCode::BAD_GATEWAY,
        json!({"jsonrpc":"2.0","id":id.unwrap_or(Value::Null),"error":{
            "code":-32603,"message":failure.to_string(),"data":data
        }}),
    )
}

async fn record_remote_auth_rejection(
    service: &NvwaService,
    profile_id: &str,
    generation: &str,
    auth: &AuthResult,
    error: &anyhow::Error,
) {
    if !matches!(
        error.downcast_ref::<RemoteFailure>(),
        Some(RemoteFailure::LoginExpired)
    ) {
        return;
    }
    // A late rejection must only expire the token used by this request, never
    // a replacement login or a newer token renewed for the same identity.
    service
        .inner
        .runtime
        .mark_expired(profile_id, generation, &auth.mcp_token.value)
        .await;
    let _ = service.persist_session(profile_id).await;
}

pub(crate) fn client_kind(value: &str) -> Result<ClientKind> {
    match value {
        "codex" => Ok(ClientKind::Codex),
        "workbuddy" => Ok(ClientKind::Workbuddy),
        "tiangong" => Ok(ClientKind::Tiangong),
        _ => bail!("Unknown MCP client"),
    }
}

pub(crate) fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|v| !v.is_empty() && v.len() <= 256)
}

/// TianGong discovery sends the same session under two case variants. HTTP
/// clients can combine them into one comma-separated value. Collapse only
/// identical copies; runtime admission still checks the exact session owner.
fn session_header(headers: &HeaderMap, client: ClientKind) -> Result<Option<&str>> {
    let mut selected = None;
    let mut count = 0;
    for header in headers.get_all("mcp-session-id") {
        let header = header
            .to_str()
            .map_err(|_| anyhow::anyhow!("Invalid MCP session header"))?;
        for value in header.split(',') {
            count += 1;
            let value = value.trim();
            ensure!(
                count <= 4
                    && !value.is_empty()
                    && value.len() <= 128
                    && value.bytes().all(|byte| byte.is_ascii_graphic()),
                "Invalid MCP session header"
            );
            if let Some(previous) = selected {
                ensure!(
                    client == ClientKind::Tiangong && previous == value,
                    "Conflicting MCP session headers"
                );
            } else {
                selected = Some(value);
            }
        }
    }
    Ok(selected)
}

#[cfg(test)]
mod session_header_tests {
    use super::*;

    #[test]
    fn tiangong_case_variants_and_fetch_combined_values_select_the_same_session() {
        let session = "12345678-1234-1234-1234-123456789abc";
        let mut headers = HeaderMap::new();
        headers.append("mcp-session-id", HeaderValue::from_static(session));
        headers.append(
            axum::http::HeaderName::from_bytes(b"Mcp-Session-Id").unwrap(),
            HeaderValue::from_static(session),
        );
        assert_eq!(
            session_header(&headers, ClientKind::Tiangong).unwrap(),
            Some(session)
        );
        headers.insert(
            "mcp-session-id",
            HeaderValue::from_str(&format!("{session}, {session}")).unwrap(),
        );
        assert_eq!(
            session_header(&headers, ClientKind::Tiangong).unwrap(),
            Some(session)
        );
        assert!(session_header(&headers, ClientKind::Codex).is_err());
        assert!(session_header(&headers, ClientKind::Workbuddy).is_err());
    }

    #[test]
    fn absent_session_is_distinct_from_conflicting_empty_or_excessive_values() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            session_header(&headers, ClientKind::Tiangong).unwrap(),
            None
        );
        for value in ["", "session-a, session-b", "session-a,", "a, a, a, a, a"] {
            headers.insert("mcp-session-id", HeaderValue::from_str(value).unwrap());
            assert!(session_header(&headers, ClientKind::Tiangong).is_err());
        }
    }
}

fn rpc_error(
    status: StatusCode,
    id: Option<Value>,
    code: i64,
    message: &str,
    unknown: bool,
) -> Response {
    let mut error = json!({"code":code,"message":message});
    if unknown {
        error["data"] = json!({"outcomeUnknown":true,"retryable":false});
    }
    response(
        status,
        json!({"jsonrpc":"2.0","id":id.unwrap_or(Value::Null),"error":error}),
    )
}

fn response(status: StatusCode, value: Value) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(Body::from(value.to_string()))
        .unwrap()
}

pub(crate) async fn post(
    State(service): State<NvwaService>,
    Path((profile_id, client)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !service.local_request(&headers).await {
        return rpc_error(
            StatusCode::FORBIDDEN,
            None,
            -32000,
            "Local request rejected",
            false,
        );
    }
    if service.closing() {
        return rpc_error(
            StatusCode::SERVICE_UNAVAILABLE,
            None,
            -32000,
            "Hub is shutting down",
            false,
        );
    }
    let Some(credential) = bearer(&headers) else {
        return rpc_error(
            StatusCode::UNAUTHORIZED,
            None,
            -32000,
            "Client authorization required",
            false,
        );
    };
    let mut request: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return rpc_error(StatusCode::BAD_REQUEST, None, -32700, "Invalid JSON", false),
    };
    let original_id = request.get("id").cloned();
    let Some(method) = request
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return rpc_error(
            StatusCode::BAD_REQUEST,
            original_id,
            -32600,
            "Invalid JSON-RPC request",
            false,
        );
    };
    if !request.is_object()
        || request.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || method.len() > 256
    {
        return rpc_error(
            StatusCode::BAD_REQUEST,
            original_id,
            -32600,
            "Invalid JSON-RPC request",
            false,
        );
    }
    if headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| {
            !v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
    {
        return rpc_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            original_id,
            -32600,
            "JSON content type required",
            false,
        );
    }
    if headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| !v.contains("application/json") || !v.contains("text/event-stream"))
    {
        return rpc_error(
            StatusCode::NOT_ACCEPTABLE,
            original_id,
            -32600,
            "Accept must include application/json and text/event-stream",
            false,
        );
    }
    let profile = match service.profile(&profile_id) {
        Ok(value) => value,
        Err(_) => {
            return rpc_error(
                StatusCode::NOT_FOUND,
                original_id,
                -32000,
                "Profile unavailable",
                false,
            );
        }
    };
    let client = match client_kind(&client) {
        Ok(value) => value,
        Err(_) => {
            return rpc_error(
                StatusCode::NOT_FOUND,
                original_id,
                -32000,
                "Client unavailable",
                false,
            );
        }
    };
    if service
        .inner
        .runtime
        .check_capability(&profile_id, client, credential)
        .await
        .is_err()
    {
        return rpc_error(
            StatusCode::UNAUTHORIZED,
            original_id,
            -32000,
            "Client authorization invalid",
            false,
        );
    }
    if let Err(error) = service.ensure_authenticated(&profile).await {
        return rpc_error(
            StatusCode::UNAUTHORIZED,
            original_id,
            -32000,
            &error.to_string(),
            false,
        );
    }
    let session = match session_header(&headers, client) {
        Ok(session) => session,
        Err(_) => {
            return rpc_error(
                StatusCode::BAD_REQUEST,
                original_id,
                -32600,
                "Invalid or conflicting MCP session headers",
                false,
            );
        }
    };
    if method == "initialize"
        && request
            .pointer("/params/protocolVersion")
            .and_then(Value::as_str)
            .is_none_or(|v| !PROTOCOLS.contains(&v))
    {
        return rpc_error(
            StatusCode::BAD_REQUEST,
            original_id,
            -32602,
            "Only MCP 2025 protocol versions are supported",
            false,
        );
    }
    let permit = if method == "notifications/cancelled" {
        None
    } else {
        match service.inner.inflight.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                return rpc_error(
                    StatusCode::TOO_MANY_REQUESTS,
                    original_id,
                    -32000,
                    "MCP bridge request capacity reached",
                    false,
                );
            }
        }
    };
    let lease = match service
        .inner
        .runtime
        .admit(
            &profile_id,
            client,
            credential,
            session,
            &method,
            original_id.as_ref(),
        )
        .await
    {
        Ok(value) => value,
        Err(error) => {
            let message = error.to_string();
            let status = if message.contains("session expired") {
                StatusCode::NOT_FOUND
            } else if message.contains("Duplicate") || message.contains("request ID") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::UNAUTHORIZED
            };
            return rpc_error(status, original_id, -32000, &message, false);
        }
    };
    if let Some(expected) = &lease.protocol
        && headers
            .get("mcp-protocol-version")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v != expected)
    {
        service.inner.runtime.finish(&lease).await;
        return rpc_error(
            StatusCode::BAD_REQUEST,
            original_id,
            -32602,
            "MCP protocol version differs from session",
            false,
        );
    }
    if method == "notifications/cancelled" {
        if original_id.is_some() {
            service.inner.runtime.finish(&lease).await;
            return rpc_error(
                StatusCode::BAD_REQUEST,
                original_id,
                -32600,
                "Cancellation must be a notification",
                false,
            );
        }
        let mapping = match (session, request.pointer("/params/requestId")) {
            (Some(session), Some(id)) => service.inner.runtime.cancel_id(session, id).await,
            _ => Ok(None),
        };
        match mapping {
            Ok(Some(upstream)) => {
                request["params"]["requestId"] = Value::String(upstream);
            }
            _ => {
                return Response::builder()
                    .status(StatusCode::ACCEPTED)
                    .header("cache-control", "no-store")
                    .body(Body::empty())
                    .unwrap();
            }
        }
        // Reasons can contain private text; only the target ID is needed upstream.
        request["params"]
            .as_object_mut()
            .map(|v| v.remove("reason"));
    }
    if let Some(id) = &lease.upstream_id {
        request["id"] = Value::String(id.clone());
    }
    let protocol = lease.protocol.clone().or_else(|| {
        request
            .pointer("/params/protocolVersion")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let forwarding = service.clone();
    // Keep reading an admitted request to its terminal response even if its
    // downstream HTTP connection disappears. MCP 2025 disconnect is not a
    // cancellation signal; explicit notifications/cancelled owns that mapping.
    let tool_call = method == "tools/call";
    let task = tokio::spawn(async move {
        let _permit = permit;
        let _activity = if tool_call {
            forwarding.inner.hub_state.upgrade().map(|hub| {
                let client = match client {
                    ClientKind::Codex => crate::desktop::clients::ClientId::Codex,
                    ClientKind::Workbuddy => crate::desktop::clients::ClientId::WorkBuddy,
                    ClientKind::Tiangong => crate::desktop::clients::ClientId::Tiangong,
                };
                hub.desktop_mcp_activity
                    .begin(&profile.id, &profile.name, client, true)
            })
        } else {
            None
        };
        let result = remote(
            &forwarding,
            &profile,
            &lease.auth,
            &request,
            protocol.as_deref(),
        )
        .await;
        forwarding.inner.runtime.finish(&lease).await;
        (lease, result)
    });
    let (lease, result) = match task.await {
        Ok(result) => result,
        Err(_) => {
            return remote_failure_response(
                original_id,
                &RemoteFailure::Unconfirmed,
                &method,
                client,
            );
        }
    };
    if !service
        .inner
        .runtime
        .lease_is_current(&profile_id, client, credential, &lease)
        .await
    {
        return rpc_error(
            StatusCode::UNAUTHORIZED,
            original_id,
            -32000,
            "Login or client access changed while request was in flight; result unconfirmed; no retry was made",
            method == "tools/call",
        );
    }
    match result {
        Ok(RemoteResponse::Accepted) if original_id.is_none() => Response::builder()
            .status(StatusCode::ACCEPTED)
            .header("cache-control", "no-store")
            .body(Body::empty())
            .unwrap(),
        Ok(RemoteResponse::Json(mut reply)) if original_id.is_some() => {
            if reply.get("id").and_then(Value::as_str) != lease.upstream_id.as_deref() {
                return remote_failure_response(
                    original_id,
                    &RemoteFailure::ResponseId,
                    &method,
                    client,
                );
            }
            reply["id"] = lease.original_id.clone().unwrap_or(Value::Null);
            if method == "initialize" && reply.get("error").is_none() {
                let negotiated = match validate_initialize(&reply) {
                    Ok(version) => version,
                    Err(_) => {
                        return remote_failure_response(
                            original_id,
                            &RemoteFailure::Initialize,
                            &method,
                            client,
                        );
                    }
                };
                let session_id = match service
                    .inner
                    .runtime
                    .create_session(
                        &profile_id,
                        client,
                        credential,
                        &lease.generation,
                        &negotiated,
                    )
                    .await
                {
                    Ok(value) => value,
                    Err(_) => {
                        return rpc_error(
                            StatusCode::UNAUTHORIZED,
                            original_id,
                            -32000,
                            "Login changed during initialization",
                            false,
                        );
                    }
                };
                let mut answer = response(StatusCode::OK, reply);
                answer.headers_mut().insert(
                    "mcp-session-id",
                    HeaderValue::from_str(&session_id).unwrap(),
                );
                answer.headers_mut().insert(
                    "mcp-protocol-version",
                    HeaderValue::from_str(&negotiated).unwrap(),
                );
                return answer;
            }
            response(StatusCode::OK, reply)
        }
        Ok(_) => remote_failure_response(original_id, &RemoteFailure::Unexpected, &method, client),
        Err(error) => {
            record_remote_auth_rejection(
                &service,
                &profile_id,
                &lease.generation,
                &lease.auth,
                &error,
            )
            .await;
            if let Some(failure) = error.downcast_ref::<RemoteFailure>() {
                return remote_failure_response(original_id, failure, &method, client);
            }
            rpc_error(
                StatusCode::BAD_GATEWAY,
                original_id,
                if error.is::<ResponseTooLarge>() {
                    RESPONSE_SIZE_ERROR_CODE
                } else {
                    -32603
                },
                &error.to_string(),
                method == "tools/call",
            )
        }
    }
}

pub(crate) async fn delete(
    State(service): State<NvwaService>,
    Path((profile_id, client)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if !service.local_request(&headers).await {
        return response(
            StatusCode::FORBIDDEN,
            json!({"error":"Local request rejected"}),
        );
    }
    let Ok(client) = client_kind(&client) else {
        return response(StatusCode::NOT_FOUND, json!({"error":"Client unavailable"}));
    };
    let session = match session_header(&headers, client) {
        Ok(session) => session,
        Err(_) => {
            return response(
                StatusCode::BAD_REQUEST,
                json!({"error":"Invalid or conflicting MCP session headers"}),
            );
        }
    };
    let (Some(credential), Some(session)) = (bearer(&headers), session) else {
        return response(
            StatusCode::UNAUTHORIZED,
            json!({"error":"Client and session authorization required"}),
        );
    };
    match service
        .inner
        .runtime
        .admit(
            &profile_id,
            client,
            credential,
            Some(session),
            "local/session/delete",
            None,
        )
        .await
    {
        Ok(_) => {
            // The stateless NVWA server has no session to DELETE. This only
            // releases local authorization/mappings; remote writes may continue.
            service.inner.runtime.delete_session(session).await;
            Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header("cache-control", "no-store")
                .body(Body::empty())
                .unwrap()
        }
        Err(_) => response(
            StatusCode::NOT_FOUND,
            json!({"error":"Session unavailable"}),
        ),
    }
}

pub(crate) async fn unsupported() -> Response {
    Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .header("allow", "POST, DELETE")
        .header("cache-control", "no-store")
        .body(Body::empty())
        .unwrap()
}

pub(crate) enum RemoteResponse {
    Json(Value),
    Accepted,
}

pub(crate) async fn remote(
    service: &NvwaService,
    profile: &NvwaProfile,
    auth: &AuthResult,
    request: &Value,
    protocol: Option<&str>,
) -> Result<RemoteResponse> {
    let mut token = HeaderValue::from_str(&auth.mcp_token.value)
        .map_err(|_| anyhow::anyhow!("NVWA token format invalid"))?;
    token.set_sensitive(true);
    let http = service
        .http_client()
        .await
        .map_err(|_| RemoteFailure::NetworkSettings)?;
    let mut builder = http
        .post(resolve_mcp_url(profile)?)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .header(&auth.mcp_token.header_name, token)
        .json(request);
    if let Some(version) = protocol {
        builder = builder.header("mcp-protocol-version", version);
    }
    let mut response = builder.send().await.map_err(|error| {
        if error.is_timeout() {
            RemoteFailure::Timeout
        } else if error.is_connect() {
            RemoteFailure::Connect
        } else {
            RemoteFailure::Network
        }
    })?;
    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(RemoteFailure::LoginExpired.into());
    }
    if response.status() == StatusCode::ACCEPTED
        && request.get("id").is_none()
        && !response.headers().contains_key("mcp-session-id")
    {
        return Ok(RemoteResponse::Accepted);
    }
    if !matches!(response.status(), StatusCode::OK | StatusCode::ACCEPTED) {
        return Err(RemoteFailure::Http {
            status: response.status().as_u16(),
        }
        .into());
    }
    if response.headers().contains_key("mcp-session-id") {
        return Err(RemoteFailure::RemoteSession.into());
    }
    if response.status() != StatusCode::OK {
        return Err(RemoteFailure::Unexpected.into());
    }
    if !response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
    {
        return Err(RemoteFailure::ContentType.into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| RemoteFailure::Interrupted)?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_REMOTE_BYTES {
            return Err(ResponseTooLarge.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let reply: Value = serde_json::from_slice(&bytes).map_err(|_| RemoteFailure::InvalidJson)?;
    if !(reply.is_object()
        && reply.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
        && (reply.get("result").is_some() ^ reply.get("error").is_some()))
    {
        return Err(RemoteFailure::InvalidRpc.into());
    }
    Ok(RemoteResponse::Json(reply))
}

pub(crate) fn validate_initialize(reply: &Value) -> Result<String> {
    ensure!(
        reply.get("error").is_none(),
        "MCP initialize returned a protocol error"
    );
    let result = reply
        .get("result")
        .ok_or_else(|| anyhow::anyhow!("MCP initialize result missing"))?;
    let version = result
        .get("protocolVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("MCP protocol version missing"))?;
    ensure!(
        PROTOCOLS.contains(&version),
        "Unsupported MCP protocol version"
    );
    ensure!(
        result.get("serverInfo").is_some_and(|v| v.is_object()
            && v.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| !n.is_empty())
            && v.get("version")
                .and_then(Value::as_str)
                .is_some_and(|n| !n.is_empty())),
        "MCP server identity missing"
    );
    ensure!(
        result
            .pointer("/capabilities/tools")
            .is_some_and(Value::is_object),
        "MCP tools capability missing"
    );
    Ok(version.to_owned())
}

/// Credentials from the selected client's validated configuration. No Debug or
/// serialization: this contains the client's local bearer.
pub(crate) struct LocalBridge {
    pub client: ClientKind,
    pub endpoint: String,
    pub credential: String,
}

pub(crate) async fn detect(
    service: &NvwaService,
    profile: &NvwaProfile,
    local: Option<LocalBridge>,
    allow_catalog_limit: bool,
) -> Result<Value> {
    let (generation, auth) = service.inner.runtime.auth(&profile.id).await?;
    let client = local.as_ref().map(|local| local.client);
    let mut transport = DetectionTransport::new(local)?;
    let result = detect_catalog(
        service,
        profile,
        &generation,
        &auth,
        &mut transport,
        allow_catalog_limit,
    )
    .await;
    transport.close().await;
    if let Err(error) = &result {
        record_remote_auth_rejection(service, &profile.id, &generation, &auth, error).await;
    }
    if let Ok(detected) = &result {
        if detected["catalogComplete"] == false {
            service
                .inner
                .runtime
                .record_catalog_limit(&profile.id, client, &generation)
                .await?;
        } else if let Some(client) = client {
            service
                .inner
                .runtime
                .record_client_check(&profile.id, client, &generation)
                .await?;
        }
    }
    result
}

struct DetectionTransport {
    local: Option<(reqwest::Client, String, String)>,
    session: Option<String>,
}

fn detection_reply(status: StatusCode, request: &Value, reply: Value) -> Result<RemoteResponse> {
    if is_size_limit_response(status, request, &reply) {
        return Err(ResponseTooLarge.into());
    }
    if status != StatusCode::OK {
        // Read only the bridge's typed metadata, not arbitrary error text from
        // the upstream service. It is tied to this exact detection request.
        if reply["jsonrpc"] == "2.0"
            && reply.get("id") == Some(request.get("id").unwrap_or(&Value::Null))
            && reply.get("result").is_none()
            && let Some(failure) = reply
                .pointer("/error/data/bridgeFailure")
                .and_then(|value| serde_json::from_value::<RemoteFailure>(value.clone()).ok())
        {
            return Err(failure.into());
        }
        bail!(
            "Dumpling MCP 连接检测失败（本机桥 HTTP {}），请导出本次连接诊断",
            status.as_u16()
        );
    }
    ensure!(
        reply.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
            && (reply.get("result").is_some() ^ reply.get("error").is_some()),
        "本机 MCP 桥返回了无效协议响应"
    );
    Ok(RemoteResponse::Json(reply))
}

#[cfg(test)]
mod failure_tests {
    use super::*;

    #[tokio::test]
    async fn bridge_failure_preserves_upstream_cause_without_echoing_remote_text() {
        let request = json!({"jsonrpc":"2.0","id":"fixture-detect","method":"initialize"});
        for failure in [
            RemoteFailure::LoginExpired,
            RemoteFailure::Connect,
            RemoteFailure::Timeout,
            RemoteFailure::Http { status: 403 },
            RemoteFailure::Http { status: 502 },
            RemoteFailure::ContentType,
            RemoteFailure::RemoteSession,
            RemoteFailure::ResponseId,
        ] {
            let response = remote_failure_response(
                request.get("id").cloned(),
                &failure,
                "initialize",
                ClientKind::Codex,
            );
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), 8192)
                .await
                .unwrap();
            let mut reply: Value = serde_json::from_slice(&body).unwrap();
            reply["error"]["message"] = "fixture-private-response".into();
            let error = detection_reply(status, &request, reply).err().unwrap();
            assert_eq!(error.downcast_ref::<RemoteFailure>(), Some(&failure));
            assert_eq!(error.to_string(), failure.to_string());
            assert!(!error.to_string().contains("fixture-private-response"));
        }
        let notification = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        let response = remote_failure_response(
            None,
            &RemoteFailure::Http { status: 403 },
            "notifications/initialized",
            ClientKind::Workbuddy,
        );
        let body = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap();
        let reply = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            detection_reply(StatusCode::BAD_GATEWAY, &notification, reply)
                .err()
                .unwrap()
                .downcast_ref::<RemoteFailure>(),
            Some(&RemoteFailure::Http { status: 403 })
        );
    }

    #[test]
    fn unrelated_bridge_errors_cannot_impersonate_a_detection_or_a_catalog_limit() {
        let request = json!({"jsonrpc":"2.0","id":"fixture-detect","method":"tools/list"});
        let reply = json!({"jsonrpc":"2.0","id":"other-request","error":{
            "code":-32603,"message":"fixture-private-response","data":{
                "bridgeFailure":{"kind":"http","status":403}
            }
        }});
        let error = detection_reply(StatusCode::BAD_GATEWAY, &request, reply)
            .err()
            .unwrap();
        assert!(error.downcast_ref::<RemoteFailure>().is_none());
        assert!(!error.is::<ResponseTooLarge>());
        assert!(!error.to_string().contains("fixture-private-response"));

        let limited = json!({"jsonrpc":"2.0","id":"fixture-detect","error":{
            "code":RESPONSE_SIZE_ERROR_CODE,"message":"fixture-private-response"
        }});
        assert!(
            detection_reply(StatusCode::BAD_GATEWAY, &request, limited)
                .err()
                .unwrap()
                .is::<ResponseTooLarge>()
        );
    }

    #[tokio::test]
    async fn tool_failure_keeps_unconfirmed_result_and_no_retry_semantics() {
        let response = remote_failure_response(
            Some(json!(7)),
            &RemoteFailure::Interrupted,
            "tools/call",
            ClientKind::Workbuddy,
        );
        let body = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap();
        let reply: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["error"]["data"]["outcomeUnknown"], true);
        assert_eq!(reply["error"]["data"]["retryable"], false);
        assert_eq!(
            reply["error"]["data"]["bridgeFailure"]["kind"],
            "interrupted"
        );
        assert_eq!(request_phase("fixture-private-method"), "request");
    }
}

impl DetectionTransport {
    fn new(local: Option<LocalBridge>) -> Result<Self> {
        let local = if let Some(local) = local {
            let http = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(3))
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .map_err(|_| anyhow::anyhow!("Cannot prepare local MCP bridge detection"))?;
            Some((http, local.endpoint, local.credential))
        } else {
            None
        };
        Ok(Self {
            local,
            session: None,
        })
    }

    async fn request(
        &mut self,
        service: &NvwaService,
        profile: &NvwaProfile,
        auth: &AuthResult,
        request: &Value,
        protocol: &str,
    ) -> Result<RemoteResponse> {
        let Some((http, endpoint, credential)) = &self.local else {
            return remote(service, profile, auth, request, Some(protocol)).await;
        };
        let mut authorization = HeaderValue::from_str(&format!("Bearer {credential}"))
            .map_err(|_| anyhow::anyhow!("Local MCP credential invalid"))?;
        authorization.set_sensitive(true);
        let mut builder = http
            .post(endpoint)
            .header("authorization", authorization)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", protocol)
            .json(request);
        if let Some(session) = &self.session {
            builder = builder.header("mcp-session-id", session);
        }
        let mut response = builder
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("Local MCP bridge unavailable"))?;
        if response.status() == StatusCode::ACCEPTED && request.get("id").is_none() {
            return Ok(RemoteResponse::Accepted);
        }
        let status = response.status();
        ensure!(
            matches!(status, StatusCode::OK | StatusCode::BAD_GATEWAY),
            "Local MCP bridge check returned HTTP {}",
            status.as_u16()
        );
        if status == StatusCode::OK
            && request.get("method").and_then(Value::as_str) == Some("initialize")
        {
            let session = response
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| anyhow::anyhow!("Local MCP bridge session missing"))?;
            ensure!(
                uuid::Uuid::parse_str(session).is_ok(),
                "Local MCP bridge session invalid"
            );
            self.session = Some(session.to_owned());
        }
        ensure!(
            response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with("application/json")),
            "Local MCP response type invalid"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("Local MCP response interrupted"))?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_REMOTE_BYTES {
                ensure!(
                    status == StatusCode::OK,
                    "Local MCP bridge check returned HTTP {}",
                    status.as_u16()
                );
                return Err(ResponseTooLarge.into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let reply: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Local MCP response invalid JSON"))?;
        detection_reply(status, request, reply)
    }

    async fn close(&self) {
        if let (Some((http, endpoint, credential)), Some(session)) = (&self.local, &self.session) {
            if let Ok(mut authorization) = HeaderValue::from_str(&format!("Bearer {credential}")) {
                authorization.set_sensitive(true);
                let _ = http
                    .delete(endpoint)
                    .header("authorization", authorization)
                    .header("mcp-session-id", session)
                    .send()
                    .await;
            }
        }
    }
}

async fn detect_catalog(
    service: &NvwaService,
    profile: &NvwaProfile,
    generation: &str,
    auth: &AuthResult,
    transport: &mut DetectionTransport,
    allow_catalog_limit: bool,
) -> Result<Value> {
    let id = format!("hub-detect-{}", uuid::Uuid::new_v4());
    let init = json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{
        "protocolVersion":PROTOCOLS[0],"capabilities":{},"clientInfo":{"name":"TianCaiSpaceHub","version":env!("CARGO_PKG_VERSION")}}});
    let RemoteResponse::Json(reply) = transport
        .request(service, profile, auth, &init, PROTOCOLS[0])
        .await?
    else {
        bail!("MCP initialize response missing");
    };
    ensure!(
        reply.get("id") == Some(&Value::String(id)),
        "MCP initialize response ID mismatch"
    );
    let version = validate_initialize(&reply)?;
    let server_info = reply.pointer("/result/serverInfo").cloned();
    let initialized = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    ensure!(
        matches!(
            transport
                .request(service, profile, auth, &initialized, &version)
                .await?,
            RemoteResponse::Accepted
        ),
        "MCP initialized notification not accepted"
    );
    let mut tools = Vec::new();
    let mut names = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor: Option<String> = None;
    for page in 0..100 {
        let id = format!("hub-tools-{}", uuid::Uuid::new_v4());
        let mut request = json!({"jsonrpc":"2.0","id":id,"method":"tools/list","params":{}});
        if let Some(cursor) = &cursor {
            request["params"]["cursor"] = Value::String(cursor.clone());
        }
        let response = transport
            .request(service, profile, auth, &request, &version)
            .await;
        let response = match response {
            Err(error) if allow_catalog_limit && error.is::<ResponseTooLarge>() => {
                return Ok(super::discovery::catalog_result(
                    &version,
                    server_info,
                    generation,
                    transport.local.is_some(),
                    tools,
                    Some("response_size"),
                ));
            }
            result => result?,
        };
        let RemoteResponse::Json(reply) = response else {
            bail!("MCP tools/list response missing");
        };
        ensure!(
            reply.get("id") == Some(&Value::String(id)) && reply.get("error").is_none(),
            "MCP tools/list protocol error or ID mismatch"
        );
        let page_tools = reply
            .pointer("/result/tools")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("MCP tools/list result malformed"))?;
        for tool in page_tools {
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("MCP tool name missing"))?;
            ensure!(
                !name.is_empty()
                    && name.len() <= 256
                    && !name.chars().any(char::is_control)
                    && names.insert(name.to_owned()),
                "MCP tool name invalid or duplicated"
            );
            ensure!(
                tool.get("inputSchema")
                    .is_some_and(|s| s.is_object()
                        && s.get("type").and_then(Value::as_str) == Some("object")),
                "MCP tool input schema invalid"
            );
            if tools.len() == 10_000 && allow_catalog_limit {
                return Ok(super::discovery::catalog_result(
                    &version,
                    server_info,
                    generation,
                    transport.local.is_some(),
                    tools,
                    Some("tool_count"),
                ));
            }
            tools.push(tool.clone());
            ensure!(tools.len() <= 10_000, "MCP tool catalog exceeds size limit");
        }
        cursor = match reply.pointer("/result/nextCursor") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) if !value.is_empty() && value.len() <= 4096 => {
                Some(value.clone())
            }
            _ => bail!("MCP tools/list cursor invalid"),
        };
        if let Some(cursor) = &cursor {
            ensure!(
                cursors.insert(cursor.clone()),
                "MCP tool catalog cursor cycle or page limit exceeded"
            );
            if page == 99 && allow_catalog_limit {
                return Ok(super::discovery::catalog_result(
                    &version,
                    server_info,
                    generation,
                    transport.local.is_some(),
                    tools,
                    Some("page_count"),
                ));
            }
            ensure!(
                page < 99,
                "MCP tool catalog cursor cycle or page limit exceeded"
            );
        } else {
            let catalog = Value::Array(tools);
            service
                .inner
                .runtime
                .set_tools(&profile.id, generation, catalog.clone())
                .await?;
            let Value::Array(tools) = catalog else {
                unreachable!()
            };
            return Ok(super::discovery::catalog_result(
                &version,
                server_info,
                generation,
                transport.local.is_some(),
                tools,
                None,
            ));
        }
    }
    bail!("MCP tool catalog incomplete")
}
