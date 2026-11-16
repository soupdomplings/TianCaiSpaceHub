//! TLS between paired Hubs; a cookie-protected loopback gateway for browsers.
//! Device credentials are supplied only by the native Hub proxy.
use std::{convert::Infallible, net::Ipv4Addr, time::Duration};

use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};
use tokio::{sync::oneshot, task::JoinHandle};
use tower::ServiceExt;

use super::{
    DeviceIdentity, NetworkConfig, NetworkSecrets, PairDecision, PairRequest, PendingPair,
    SERVICE_PORT, device_rows, now_ms,
};
use crate::app_state::SharedState;

pub(super) struct Transport {
    pub browser_base: String,
    pub browser_ticket: String,
    target_handle: axum_server::Handle,
    target_task: JoinHandle<()>,
    local_task: JoinHandle<()>,
    local_shutdown: Option<oneshot::Sender<()>>,
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.target_handle.shutdown();
        if let Some(tx) = self.local_shutdown.take() {
            let _ = tx.send(());
        }
        self.target_task.abort();
        self.local_task.abort();
    }
}

pub(super) async fn start(
    state: SharedState,
    config: &NetworkConfig,
    secrets: &NetworkSecrets,
) -> Result<Transport> {
    ensure!(
        !config.certificate_pem.is_empty() && !secrets.tls_private_key.is_empty(),
        "远程连接设备证书未准备"
    );
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(
        config.certificate_pem.as_bytes().to_vec(),
        secrets.tls_private_key.as_bytes().to_vec(),
    )
    .await
    .context("无法准备远程连接 HTTPS")?;
    let target_listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, SERVICE_PORT))
        .context("远程会话端口 3861 被占用，请关闭占用程序后重新启用")?;
    target_listener.set_nonblocking(true)?;
    let local_listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .context("无法打开本机网页入口")?;
    let browser_base = format!("http://127.0.0.1:{}", local_listener.local_addr()?.port());
    let browser_ticket = super::random_token();
    let target_handle = axum_server::Handle::new();
    let server = axum_server::from_tcp_rustls(target_listener, tls).handle(target_handle.clone());
    let target_routes = target_router(state.clone()).with_state(state.clone());
    let target_state = state.clone();
    let target_task = tokio::spawn(async move {
        if server
            .serve(target_routes.into_make_service())
            .await
            .is_err()
        {
            target_state.remote_network.inner.lock().await.last_error =
                Some("远程 HTTPS 服务已停止，请重新启用远程连接".into());
        }
    });
    let guard = BrowserGuard {
        state: state.clone(),
        base: browser_base.clone(),
        ticket: browser_ticket.clone(),
        cookie_name: format!("tch_remote_{}", local_listener.local_addr()?.port()),
    };
    let routes = Router::new()
        .route("/api/remote/v1/devices", get(devices))
        .route("/api/remote/v1/{*path}", any(proxy))
        .merge(crate::remote_frontend::router())
        .layer(middleware::from_fn_with_state(guard, local_guard))
        .with_state(state);
    let (tx, rx) = oneshot::channel();
    let local_task = tokio::spawn(async move {
        let _ = axum::serve(local_listener, routes)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    Ok(Transport {
        browser_base,
        browser_ticket,
        target_handle,
        target_task,
        local_task,
        local_shutdown: Some(tx),
    })
}

pub(super) async fn stop(mut transport: Transport) {
    transport.target_handle.shutdown();
    if let Some(tx) = transport.local_shutdown.take() {
        let _ = tx.send(());
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), &mut transport.target_task).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), &mut transport.local_task).await;
}

pub(super) fn peer_client(device: &DeviceIdentity, socks: u16) -> Result<reqwest::Client> {
    let certificate = reqwest::Certificate::from_pem(device.certificate_pem.as_bytes())
        .context("配对设备证书不可用，请重新配对")?;
    reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(format!("socks5://127.0.0.1:{socks}"))?)
        .tls_built_in_root_certs(false)
        .add_root_certificate(certificate)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .build()
        .context("无法创建配对设备的加密连接")
}

fn target_router(state: SharedState) -> Router<SharedState> {
    Router::new()
        .route("/network/info", get(info))
        .route("/network/pair", post(receive_pair))
        .route("/network/pair/{request_id}", get(pair_status))
        .nest(
            "/api/remote/v1",
            crate::remote_sessions::router()
                .layer(middleware::from_fn_with_state(state, authorize)),
        )
}

async fn info(State(state): State<SharedState>) -> Json<Value> {
    let inner = state.remote_network.inner.lock().await;
    Json(json!({"version":1,"device":inner.config.identity()}))
}

async fn receive_pair(
    State(state): State<SharedState>,
    Json(request): Json<PairRequest>,
) -> Response {
    let result: Result<()> = async {
        request.device.validate()?;
        ensure!(
            uuid::Uuid::parse_str(&request.request_id).is_ok()
                && valid_token(&request.return_token)
                && valid_token(&request.poll_token),
            "设备配对请求格式无效"
        );
        let mut inner = state.remote_network.inner.lock().await;
        ensure!(
            inner.config.enabled && inner.running.is_some(),
            "远程连接当前未启用"
        );
        if let Some(previous) = inner.pairings.get(&request.request_id) {
            ensure!(
                previous.request.poll_token == request.poll_token && previous.expires_at > now_ms(),
                "配对请求已失效"
            );
            return Ok(());
        }
        let expiry = *inner
            .invitations
            .get(&request.invitation_token)
            .filter(|v| **v > now_ms())
            .context("连接码已失效，请从目标 Hub 重新复制")?;
        ensure!(
            request.device.id != inner.config.device_id && request.device.ipv4 != inner.config.ipv4,
            "设备身份或虚拟地址冲突"
        );
        ensure!(
            inner.pairings.len() < 16,
            "等待处理的配对请求过多，请先在 Hub 中处理"
        );
        inner.invitations.remove(&request.invitation_token);
        inner.pairings.insert(
            request.request_id.clone(),
            PendingPair {
                request,
                expires_at: expiry,
                decision: PairDecision::Pending,
            },
        );
        Ok(())
    }
    .await;
    match result {
        Ok(()) => Json(json!({"status":"pending"})).into_response(),
        Err(error) => failure(
            StatusCode::FORBIDDEN,
            "pairing_rejected",
            &error.to_string(),
        ),
    }
}

async fn pair_status(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let inner = state.remote_network.inner.lock().await;
    let Some(pending) = inner.pairings.get(&id).filter(|v| v.expires_at > now_ms()) else {
        return failure(StatusCode::NOT_FOUND, "pairing_expired", "配对请求已过期");
    };
    if bearer(&headers) != Some(pending.request.poll_token.as_str()) {
        return failure(
            StatusCode::UNAUTHORIZED,
            "pairing_unauthorized",
            "配对凭据无效",
        );
    }
    match pending.decision {
        PairDecision::Pending => Json(json!({"status":"pending"})).into_response(),
        PairDecision::Declined => Json(json!({"status":"declined"})).into_response(),
        PairDecision::Approved => {
            let Some(credentials) = inner.secrets.peers.get(&pending.request.device.id) else {
                return failure(StatusCode::GONE, "pairing_revoked", "设备授权已撤销");
            };
            Json(json!({"status":"approved","token":credentials.inbound,"device":inner.config.identity()})).into_response()
        }
    }
}

fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|v| v.is_ascii_hexdigit())
}
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

async fn inbound_authorized(state: &SharedState, device: &str, token: &str) -> bool {
    let inner = state.remote_network.inner.lock().await;
    inner.config.enabled
        && inner.transport.is_some()
        && inner
            .secrets
            .peers
            .get(device)
            .is_some_and(|v| v.inbound == token)
}

async fn authorize(State(state): State<SharedState>, request: Request, next: Next) -> Response {
    let receiver = state.remote_network.revocations.subscribe();
    let device = request
        .headers()
        .get("x-hub-device")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let token = bearer(request.headers()).unwrap_or("").to_string();
    if !inbound_authorized(&state, &device, &token).await {
        return failure(
            StatusCode::UNAUTHORIZED,
            "device_not_paired",
            "此设备尚未配对或授权已撤销，请在目标 Hub 完成配对",
        );
    }
    let response = next.run(request).await;
    bind_inbound_response(state, device, token, response, receiver)
}

fn bind_inbound_response(
    state: SharedState,
    device: String,
    token: String,
    response: Response,
    receiver: tokio::sync::watch::Receiver<u64>,
) -> Response {
    let (parts, body) = response.into_parts();
    let input = Box::pin(body.into_data_stream());
    let output = stream::unfold(
        (input, receiver, state, device, token),
        |(mut input, mut receiver, state, device, token)| async move {
            loop {
                tokio::select! {
                    value = input.next() => return value.map(|value| (value, (input, receiver, state, device, token))),
                    changed = receiver.changed() => {
                        if changed.is_err() || !inbound_authorized(&state, &device, &token).await { return None; }
                    }
                }
            }
        },
    );
    Response::from_parts(parts, Body::from_stream(output))
}

#[derive(Clone)]
struct BrowserGuard {
    state: SharedState,
    base: String,
    ticket: String,
    cookie_name: String,
}

async fn local_guard(State(guard): State<BrowserGuard>, request: Request, next: Next) -> Response {
    let expected_host = guard.base.strip_prefix("http://").unwrap_or_default();
    if request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        != Some(expected_host)
    {
        return failure(
            StatusCode::FORBIDDEN,
            "local_host_required",
            "请从 Hub 打开本机网页入口",
        );
    }
    if request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v != guard.base)
        || request
            .headers()
            .get("sec-fetch-site")
            .and_then(|v| v.to_str().ok())
            == Some("cross-site")
    {
        return failure(
            StatusCode::FORBIDDEN,
            "local_origin_required",
            "请从 Hub 打开本机网页入口",
        );
    }
    let query: Vec<(String, String)> =
        url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
            .into_owned()
            .collect();
    if request.uri().path() == "/remote/"
        && query
            .iter()
            .any(|(key, value)| key == "ticket" && value == &guard.ticket)
    {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in query.iter().filter(|(key, _)| key != "ticket") {
            serializer.append_pair(key, value);
        }
        let query = serializer.finish();
        let location = if query.is_empty() {
            "/remote/".to_string()
        } else {
            format!("/remote/?{query}")
        };
        let cookie = format!(
            "{}={}; HttpOnly; SameSite=Strict; Path=/",
            guard.cookie_name, guard.ticket
        );
        let mut response = StatusCode::SEE_OTHER.into_response();
        if let (Ok(location), Ok(cookie)) = (
            HeaderValue::from_str(&location),
            HeaderValue::from_str(&cookie),
        ) {
            response.headers_mut().insert(header::LOCATION, location);
            response.headers_mut().insert(header::SET_COOKIE, cookie);
        }
        return response;
    }
    let allowed = request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|cookies| {
            cookies.split(';').any(|pair| {
                pair.trim()
                    .split_once('=')
                    .is_some_and(|(name, value)| name == guard.cookie_name && value == guard.ticket)
            })
        });
    if !allowed {
        return failure(
            StatusCode::UNAUTHORIZED,
            "browser_session_required",
            "网页入口已失效，请在 Hub 的远程连接中重新打开",
        );
    }
    if !guard.state.remote_network.inner.lock().await.config.enabled {
        return failure(StatusCode::GONE, "remote_disabled", "远程连接已停用");
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    if !response.headers().contains_key("content-security-policy") {
        response.headers_mut().insert("content-security-policy", HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; frame-src 'self' blob:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"));
    }
    response
}

async fn devices(State(state): State<SharedState>) -> Json<Value> {
    let inner = state.remote_network.inner.lock().await;
    Json(json!({"devices":device_rows(&inner),"localDeviceId":inner.config.device_id}))
}

async fn proxy(State(state): State<SharedState>, mut request: Request) -> Response {
    let receiver = state.remote_network.revocations.subscribe();
    let target_id = request
        .headers()
        .get("x-hub-target")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let local_id = state
        .remote_network
        .inner
        .lock()
        .await
        .config
        .device_id
        .clone();
    if target_id.as_ref().is_none_or(|id| id == &local_id) {
        let Some(path) = request
            .uri()
            .path_and_query()
            .and_then(|v| v.as_str().strip_prefix("/api/remote/v1"))
        else {
            return failure(
                StatusCode::BAD_REQUEST,
                "invalid_remote_path",
                "远程会话地址无效",
            );
        };
        let Ok(uri) = path.parse() else {
            return failure(
                StatusCode::BAD_REQUEST,
                "invalid_remote_path",
                "远程会话地址无效",
            );
        };
        *request.uri_mut() = uri;
        request.headers_mut().remove(header::AUTHORIZATION);
        request.headers_mut().remove("x-hub-device");
        return crate::remote_sessions::router()
            .with_state(state)
            .oneshot(request)
            .await
            .unwrap_or_else(|never: Infallible| match never {});
    }
    let target_id = target_id.unwrap_or_default();
    let snapshot = {
        let inner = state.remote_network.inner.lock().await;
        inner
            .config
            .paired
            .iter()
            .find(|device| device.id == target_id)
            .cloned()
            .zip(inner.secrets.peers.get(&target_id).cloned())
            .zip(inner.running.as_ref().map(|v| v.socks_port))
    };
    let Some(((target, credentials), socks)) = snapshot else {
        return failure(
            StatusCode::UNAUTHORIZED,
            "device_not_paired",
            "目标设备尚未配对或已离线，请在 Hub 查看连接状态",
        );
    };
    let client = match peer_client(&target, socks) {
        Ok(client) => client,
        Err(error) => {
            return failure(
                StatusCode::BAD_GATEWAY,
                "peer_tls_unavailable",
                &error.to_string(),
            );
        }
    };
    let url = format!(
        "https://{}:{SERVICE_PORT}{}",
        target.ipv4,
        request
            .uri()
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or("/")
    );
    let (parts, body) = request.into_parts();
    let mut outbound = client
        .request(parts.method, &url)
        .bearer_auth(&credentials.outbound)
        .header("x-hub-device", &local_id);
    for name in [
        "content-type",
        "x-file-name",
        "last-event-id",
        "range",
        "accept",
    ] {
        if let Some(value) = parts.headers.get(name) {
            outbound = outbound.header(name, value);
        }
    }
    if let Some(length) = parts.headers.get(header::CONTENT_LENGTH) {
        outbound = outbound.header(header::CONTENT_LENGTH, length);
    }
    let response = match outbound
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            return failure(
                StatusCode::BAD_GATEWAY,
                "peer_unreachable",
                "无法连接目标设备，请确认两端 Hub 与客户端仍在运行，并检查网络节点状态",
            );
        }
    };
    let status = response.status();
    let mut headers = HeaderMap::new();
    for name in [
        "content-type",
        "cache-control",
        "content-disposition",
        "content-length",
        "accept-ranges",
        "content-range",
        "content-security-policy",
    ] {
        if let Some(value) = response.headers().get(name) {
            headers.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.clone(),
            );
        }
    }
    let input = Box::pin(response.bytes_stream());
    let token = credentials.outbound;
    let output = stream::unfold(
        (input, receiver, state, target_id, token),
        |(mut input, mut receiver, state, target_id, token)| async move {
            loop {
                tokio::select! {
                    value = input.next() => return value.map(|value|(value,(input,receiver,state,target_id,token))),
                    changed = receiver.changed() => {
                        let inner = state.remote_network.inner.lock().await;
                        if changed.is_err() || !inner.config.enabled || inner.transport.is_none() || !inner.secrets.peers.get(&target_id).is_some_and(|v|v.outbound==token) { return None; }
                    }
                }
            }
        },
    );
    let mut response = Response::new(Body::from_stream(output));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

fn failure(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"code":code,"message":message}})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_tokens_and_bearer_values_are_exact() {
        assert!(valid_token(&"a".repeat(64)));
        assert!(!valid_token(&"a".repeat(63)));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer fixture"),
        );
        assert_eq!(bearer(&headers), Some("fixture"));
    }

    fn browser_fixture(state: SharedState) -> Router {
        Router::new()
            .route("/remote/", get(|| async { "fixture" }))
            .route(
                "/api/remote/v1/fixture",
                get(|| async {
                    let mut response = "fixture".into_response();
                    response.headers_mut().insert(
                        "content-security-policy",
                        HeaderValue::from_static("sandbox; default-src 'none'"),
                    );
                    response
                }),
            )
            .layer(middleware::from_fn_with_state(
                BrowserGuard {
                    state,
                    base: "http://127.0.0.1:12345".into(),
                    ticket: "fixture-ticket".into(),
                    cookie_name: "fixture-cookie".into(),
                },
                local_guard,
            ))
    }

    #[tokio::test]
    async fn isolated_target_does_not_publish_management_routes() {
        let (_directory, state) = super::super::tests::fixture();
        let router = target_router(state.clone()).with_state(state);
        for path in [
            "/api/status",
            "/api/shutdown",
            "/api/config",
            "/api/remote-network/status",
        ] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/remote/v1/clients")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn browser_ticket_becomes_http_only_cookie_and_is_removed_from_url() {
        let (_directory, state) = super::super::tests::fixture();
        let response = browser_fixture(state)
            .oneshot(
                Request::builder()
                    .uri("/remote/?ticket=fixture-ticket&device=fixture-device")
                    .header("host", "127.0.0.1:12345")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers()[header::LOCATION],
            "/remote/?device=fixture-device"
        );
        assert!(
            response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("HttpOnly; SameSite=Strict")
        );
    }

    #[tokio::test]
    async fn browser_gateway_rejects_cross_origin_and_preserves_artifact_sandbox() {
        let (_directory, state) = super::super::tests::fixture();
        state.remote_network.inner.lock().await.config.enabled = true;
        let router = browser_fixture(state);
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/remote/v1/fixture")
                    .header("host", "127.0.0.1:12345")
                    .header("cookie", "fixture-cookie=fixture-ticket")
                    .header("origin", "https://unrelated.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/remote/v1/fixture")
                    .header("host", "127.0.0.1:12345")
                    .header("cookie", "fixture-cookie=fixture-ticket")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-security-policy"],
            "sandbox; default-src 'none'"
        );
    }

    #[tokio::test]
    async fn revocation_ends_a_waiting_stream_without_a_new_model_event() {
        let (_directory, state) = super::super::tests::fixture();
        let receiver = state.remote_network.revocations.subscribe();
        let response = Response::new(Body::from_stream(stream::pending::<
            Result<axum::body::Bytes, std::io::Error>,
        >()));
        let response = bind_inbound_response(
            state.clone(),
            "fixture".into(),
            "revoked-token".into(),
            response,
            receiver,
        );
        state.remote_network.revocations.send_modify(|v| *v += 1);
        let mut body = response.into_body().into_data_stream();
        assert!(
            tokio::time::timeout(Duration::from_millis(250), body.next())
                .await
                .unwrap()
                .is_none()
        );
    }
}
