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
    runtime::now_ms,
    types::{AuthResult, ClientKind, NvwaProfile},
};

pub(crate) const PROTOCOLS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const MAX_REMOTE_BYTES: usize = 8 * 1024 * 1024;

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
    let session = headers.get("mcp-session-id").and_then(|v| v.to_str().ok());
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
    let task = tokio::spawn(async move {
        let _permit = permit;
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
            return rpc_error(
                StatusCode::BAD_GATEWAY,
                original_id,
                -32603,
                "NVWA request ended without a confirmed result",
                method == "tools/call",
            );
        }
    };
    match result {
        Ok(RemoteResponse::Accepted) if original_id.is_none() => Response::builder()
            .status(StatusCode::ACCEPTED)
            .header("cache-control", "no-store")
            .body(Body::empty())
            .unwrap(),
        Ok(RemoteResponse::Json(mut reply)) if original_id.is_some() => {
            if reply.get("id").and_then(Value::as_str) != lease.upstream_id.as_deref() {
                return rpc_error(
                    StatusCode::BAD_GATEWAY,
                    original_id,
                    -32603,
                    "Upstream response ID mismatch",
                    method == "tools/call",
                );
            }
            reply["id"] = lease.original_id.clone().unwrap_or(Value::Null);
            if method == "initialize" && reply.get("error").is_none() {
                let negotiated = match validate_initialize(&reply) {
                    Ok(version) => version,
                    Err(_) => {
                        return rpc_error(
                            StatusCode::BAD_GATEWAY,
                            original_id,
                            -32603,
                            "Invalid upstream initialize response",
                            false,
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
        Ok(_) => rpc_error(
            StatusCode::BAD_GATEWAY,
            original_id,
            -32603,
            "Unexpected upstream response",
            method == "tools/call",
        ),
        Err(error) => {
            if error.to_string() == "NVWA authorization expired" {
                service
                    .inner
                    .runtime
                    .mark_expired(&profile_id, &lease.generation, &lease.auth.mcp_token.value)
                    .await;
                let _ = service.persist_session(&profile_id).await;
            }
            rpc_error(
                StatusCode::BAD_GATEWAY,
                original_id,
                -32603,
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
    let (Some(credential), Some(session)) = (
        bearer(&headers),
        headers.get("mcp-session-id").and_then(|v| v.to_str().ok()),
    ) else {
        return response(
            StatusCode::UNAUTHORIZED,
            json!({"error":"Client and session authorization required"}),
        );
    };
    let Ok(client) = client_kind(&client) else {
        return response(StatusCode::NOT_FOUND, json!({"error":"Client unavailable"}));
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
    let mut builder = service
        .inner
        .http
        .post(&profile.mcp_url)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .header(&auth.mcp_token.header_name, token)
        .json(request);
    if let Some(version) = protocol {
        builder = builder.header("mcp-protocol-version", version);
    }
    let mut response = builder.send().await.map_err(|_| {
        anyhow::anyhow!("NVWA request interrupted; result unconfirmed; no retry was made")
    })?;
    if response.status() == StatusCode::UNAUTHORIZED {
        bail!("NVWA authorization expired");
    }
    ensure!(
        !response.headers().contains_key("mcp-session-id"),
        "NVWA remote sessions unsupported; this bridge requires stateless JSON"
    );
    if response.status() == StatusCode::ACCEPTED && request.get("id").is_none() {
        return Ok(RemoteResponse::Accepted);
    }
    ensure!(
        response.status() == StatusCode::OK,
        "NVWA returned HTTP {}; no retry was made",
        response.status().as_u16()
    );
    ensure!(
        response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")),
        "NVWA response transport unsupported; this bridge requires stateless JSON"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        anyhow::anyhow!("NVWA response interrupted; result unconfirmed; no retry was made")
    })? {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= MAX_REMOTE_BYTES,
            "NVWA response exceeds size limit; result unconfirmed"
        );
        bytes.extend_from_slice(&chunk);
    }
    let reply: Value = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("NVWA returned invalid JSON; result unconfirmed"))?;
    ensure!(
        reply.is_object()
            && reply.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
            && (reply.get("result").is_some() ^ reply.get("error").is_some()),
        "NVWA returned invalid JSON-RPC response"
    );
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

pub(crate) async fn detect(
    service: &NvwaService,
    profile: &NvwaProfile,
    client: Option<ClientKind>,
) -> Result<Value> {
    let (generation, auth) = service.inner.runtime.auth(&profile.id).await?;
    let mut transport = DetectionTransport::new(service, profile, client).await?;
    let result = detect_catalog(service, profile, &generation, &auth, &mut transport).await;
    transport.close().await;
    if result.is_ok()
        && let Some(client) = client
    {
        service
            .inner
            .runtime
            .record_client_check(&profile.id, client, &generation)
            .await?;
    }
    result
}

struct DetectionTransport {
    local: Option<(reqwest::Client, String, String)>,
    session: Option<String>,
}

impl DetectionTransport {
    async fn new(
        service: &NvwaService,
        profile: &NvwaProfile,
        client: Option<ClientKind>,
    ) -> Result<Self> {
        let local = if let Some(client) = client {
            let credential = service
                .inner
                .runtime
                .existing_capability(&profile.id, client)
                .await
                .ok_or_else(|| {
                    anyhow::anyhow!("Apply this client before checking its local bridge")
                })?;
            let http = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(3))
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .map_err(|_| anyhow::anyhow!("Cannot prepare local MCP bridge detection"))?;
            Some((
                http,
                format!(
                    "{}/mcp/{}/{}",
                    service.inner.management.base_url,
                    profile.id,
                    client.as_str()
                ),
                credential,
            ))
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
        ensure!(
            response.status() == StatusCode::OK,
            "Local MCP bridge check returned HTTP {}",
            response.status().as_u16()
        );
        if request.get("method").and_then(Value::as_str) == Some("initialize") {
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
            ensure!(
                bytes.len().saturating_add(chunk.len()) <= MAX_REMOTE_BYTES,
                "Local MCP response exceeds size limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        let reply: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Local MCP response invalid JSON"))?;
        ensure!(
            reply.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
                && (reply.get("result").is_some() ^ reply.get("error").is_some()),
            "Local MCP response malformed"
        );
        Ok(RemoteResponse::Json(reply))
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
        let RemoteResponse::Json(reply) = transport
            .request(service, profile, auth, &request, &version)
            .await?
        else {
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
                cursors.insert(cursor.clone()) && page < 99,
                "MCP tool catalog cursor cycle or page limit exceeded"
            );
        } else {
            let catalog = Value::Array(tools);
            service
                .inner
                .runtime
                .set_tools(&profile.id, generation, catalog.clone())
                .await?;
            return Ok(
                json!({"state":"tools_discovered","protocolVersion":version,"serverInfo":server_info,
                "toolCount":catalog.as_array().map(Vec::len),"tools":catalog,"checkedAtMs":now_ms(),
                "authGeneration":generation,
                "localBridgeVerified":transport.local.is_some(),
                "detail":"initialize and paginated tools/list verified; no tool invoked"}),
            );
        }
    }
    bail!("MCP tool catalog incomplete")
}
