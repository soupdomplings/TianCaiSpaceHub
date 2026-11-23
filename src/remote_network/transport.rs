//! TLS between paired Hubs; a cookie-protected loopback gateway for browsers.
//! Device credentials are supplied only by the native Hub proxy.
use std::{convert::Infallible, net::Ipv4Addr, time::Duration};

use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    body::Body,
    extract::{OriginalUri, Path, Request, State},
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

async fn browser_listener(
    state: &SharedState,
    preferred: u16,
) -> Result<(tokio::net::TcpListener, String)> {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, preferred))
        .await
        .with_context(|| format!("无法打开本机网页端口 {preferred}，请关闭占用该端口的程序"))?;
    let port = listener.local_addr()?.port();
    let mut inner = state.remote_network.inner.lock().await;
    if inner.config.browser_port != port || inner.secrets.browser_key.is_empty() {
        inner.config.browser_port = port;
        if inner.secrets.browser_key.is_empty() {
            inner.secrets.browser_key = super::random_token();
        }
        super::persist(state, &inner)?;
    }
    Ok((listener, inner.secrets.browser_key.clone()))
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
    let (local_listener, remember_key) = browser_listener(&state, config.browser_port).await?;
    let browser_base = format!("http://127.0.0.1:{}", local_listener.local_addr()?.port());
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
        session: super::random_token(),
        remember_path: remember_path(&remember_key),
        remember_key,
        cookie_name: format!("tch_remote_{}", local_listener.local_addr()?.port()),
    };
    let routes = browser_router(state, guard);
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

fn browser_router(state: SharedState, guard: BrowserGuard) -> Router {
    Router::new()
        .route("/api/remote/v1/devices", get(devices))
        // Strip the static prefix without adding a wildcard to the session
        // handler's Path(client, session, ...) parameters.
        .nest_service("/api/remote/v1", any(proxy).with_state(state.clone()))
        .merge(crate::remote_frontend::router())
        .layer(middleware::from_fn_with_state(guard, local_guard))
        .with_state(state)
}

#[derive(Default)]
pub(super) struct PeerClients(std::collections::HashMap<String, PeerRoute>);

struct PeerRoute {
    socks: u16,
    address: String,
    certificate: String,
    client: std::sync::Arc<once_cell::sync::OnceCell<reqwest::Client>>,
}

impl PeerClients {
    // Obtain the slot while the pairing/runtime snapshot is locked. TLS
    // setup and all I/O run after releasing that lock.
    fn slot(
        &mut self,
        target: &DeviceIdentity,
        socks: u16,
    ) -> std::sync::Arc<once_cell::sync::OnceCell<reqwest::Client>> {
        let route = self.0.entry(target.id.clone());
        let init = || PeerRoute {
            socks,
            address: target.ipv4.clone(),
            certificate: target.certificate_pem.clone(),
            client: Default::default(),
        };
        let route = match route {
            std::collections::hash_map::Entry::Occupied(entry) => {
                let value = entry.into_mut();
                if value.socks != socks
                    || value.address != target.ipv4
                    || value.certificate != target.certificate_pem
                {
                    *value = init();
                }
                value
            }
            std::collections::hash_map::Entry::Vacant(entry) => entry.insert(init()),
        };
        route.client.clone()
    }
    pub(super) fn clear(&mut self) {
        self.0.clear();
    }
    pub(super) fn remove(&mut self, id: &str) {
        self.0.remove(id);
    }
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
        .retry(reqwest::retry::never())
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
            super::wire::compress_json(crate::remote_sessions::router())
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
    session: String,
    remember_key: String,
    remember_path: String,
    cookie_name: String,
}

fn remember_path(key: &str) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "/remote/browser/{}",
        hex::encode(&Sha256::digest(key.as_bytes())[..16])
    )
}

fn has_cookie(request: &Request, name: &str, expected: &str) -> bool {
    request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .filter_map(|item| item.trim().split_once('='))
                .any(|(n, v)| n == name && v == expected)
        })
}

fn local_redirect(path: &str) -> Response {
    let Ok(location) = HeaderValue::from_str(path) else {
        return failure(StatusCode::BAD_REQUEST, "invalid_entry", "网页入口无效");
    };
    let mut response = StatusCode::SEE_OTHER.into_response();
    response.headers_mut().insert(header::LOCATION, location);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn browser_login(guard: &BrowserGuard, path: &str) -> Response {
    let mut response = local_redirect(path);
    // A cookie on 127.0.0.1 is not isolated by port. Only a per-run session
    // cookie is sent on regular requests; the persistent credential is scoped
    // to this installation's dedicated recovery path.
    for value in [
        format!(
            "{}={}; HttpOnly; SameSite=Strict; Path=/",
            guard.cookie_name, guard.session
        ),
        remembered_cookie(guard),
    ] {
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&value).expect("generated cookie"),
        );
    }
    response
}

fn remembered_cookie(guard: &BrowserGuard) -> String {
    format!(
        "{}_remember={}; HttpOnly; SameSite=Strict; Path={}; Max-Age=34560000",
        guard.cookie_name, guard.remember_key, guard.remember_path
    )
}

fn local_read_path(path: &str) -> bool {
    (path == "/remote" || path.starts_with("/remote/") || path.starts_with("/api/remote/v1/"))
        && !path.starts_with("/remote/browser/")
        && !path.contains(['\\', '#', '\r', '\n'])
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
    if !guard.state.remote_network.inner.lock().await.config.enabled {
        return failure(StatusCode::GONE, "remote_disabled", "远程连接已停用");
    }
    let read = request.method() == axum::http::Method::GET;
    if read
        && request.uri().path() == "/remote/"
        && let Some((_, ticket)) = query.iter().find(|(key, _)| key == "ticket")
    {
        if !guard
            .state
            .remote_network
            .inner
            .lock()
            .await
            .browser_launches
            .remove(ticket)
            .is_some_and(|until| until > now_ms())
        {
            return failure(
                StatusCode::UNAUTHORIZED,
                "launch_expired",
                "打开网页的链接已用过或已过期，请从 Hub 重新打开",
            );
        }
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
        return browser_login(&guard, &location);
    }
    if read && request.uri().path() == guard.remember_path {
        if has_cookie(
            &request,
            &format!("{}_remember", guard.cookie_name),
            &guard.remember_key,
        ) {
            let dest = query
                .iter()
                .find(|(k, v)| k == "next" && local_read_path(v))
                .map(|(_, v)| v.as_str())
                .unwrap_or("/remote/");
            return browser_login(&guard, dest);
        }
        return failure(
            StatusCode::UNAUTHORIZED,
            "browser_session_required",
            "请在本机 Hub 的远程连接中打开一次网页",
        );
    }
    if !has_cookie(&request, &guard.cookie_name, &guard.session) {
        let dest = request
            .uri()
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or("/");
        if read && local_read_path(dest) {
            let query = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("next", dest)
                .finish();
            return local_redirect(&format!("{}?{query}", guard.remember_path));
        }
        return failure(
            StatusCode::UNAUTHORIZED,
            "browser_session_required",
            "网页入口已失效，请在 Hub 的远程连接中重新打开",
        );
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
    Json(json!({"devices":device_rows(&inner),"localDeviceId":inner.config.device_id,
        "webEntry":crate::remote_frontend::entry_script(),"hubVersion":env!("CARGO_PKG_VERSION")}))
}

async fn proxy(
    State(state): State<SharedState>,
    OriginalUri(original_uri): OriginalUri,
    mut request: Request,
) -> Response {
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
        let mut inner = state.remote_network.inner.lock().await;
        let peer = inner
            .config
            .paired
            .iter()
            .find(|device| device.id == target_id)
            .cloned()
            .zip(inner.secrets.peers.get(&target_id).cloned());
        let route = peer.as_ref().and_then(|(target, _)| {
            let socks = inner.running.as_ref()?.socks_port;
            Some((socks, inner.proxy_clients.slot(target, socks)))
        });
        (peer, route)
    };
    let (Some((target, credentials)), route) = snapshot else {
        return failure(
            StatusCode::UNAUTHORIZED,
            "device_not_paired",
            "目标设备尚未配对或授权已删除，请在 Hub 查看连接状态",
        );
    };
    let Some((socks, slot)) = route else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "network_reconnecting",
            "个人网络正在重连，设备授权仍然有效",
        );
    };
    let client = match slot.get_or_try_init(|| peer_client(&target, socks)) {
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
        original_uri
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
    #[tokio::test]
    async fn browser_reads_reuse_one_socks_tls_connection_and_drop_it_after_revocation() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _ = rustls::crypto::ring::default_provider().install_default();
        let rcgen::CertifiedKey { cert, key_pair } =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let pem = cert.pem();
        let tls = axum_server::tls_rustls::RustlsConfig::from_pem(
            pem.clone().into_bytes(),
            key_pair.serialize_pem().into_bytes(),
        )
        .await
        .unwrap();
        let origin = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        origin.set_nonblocking(true).unwrap();
        let target_addr = origin.local_addr().unwrap();
        let target = tokio::spawn(async move {
            axum_server::from_tcp_rustls(origin, tls)
                .serve(
                    Router::new()
                        .route("/read", get(|| async { "ok" }))
                        .into_make_service(),
                )
                .await
        });
        let proxy = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let proxy_port = proxy.local_addr().unwrap().port();
        let tunnels = Arc::new(AtomicUsize::new(0));
        let count = tunnels.clone();
        let socks = tokio::spawn(async move {
            while let Ok((mut stream, _)) = proxy.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut greeting = [0; 2];
                    stream.read_exact(&mut greeting).await.unwrap();
                    let mut methods = vec![0; greeting[1] as usize];
                    stream.read_exact(&mut methods).await.unwrap();
                    stream.write_all(&[5, 0]).await.unwrap();
                    let mut request = [0; 4];
                    stream.read_exact(&mut request).await.unwrap();
                    assert_eq!(request, [5, 1, 0, 1]);
                    let mut addr_port = [0; 6];
                    stream.read_exact(&mut addr_port).await.unwrap();
                    let mut remote = tokio::net::TcpStream::connect(target_addr).await.unwrap();
                    stream
                        .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                        .await
                        .unwrap();
                    let _ = tokio::io::copy_bidirectional(&mut stream, &mut remote).await;
                });
            }
        });
        let device = DeviceIdentity {
            id: "fixture".into(),
            name: "fixture".into(),
            ipv4: "127.0.0.1".into(),
            certificate_pem: pem,
        };
        let mut clients = PeerClients::default();
        tokio::time::timeout(Duration::from_secs(6), async {
            for i in 0..3 {
                if i == 2 {
                    clients.remove(&device.id);
                }
                let slot = clients.slot(&device, proxy_port);
                let http = slot
                    .get_or_try_init(|| peer_client(&device, proxy_port))
                    .unwrap();
                assert_eq!(
                    http.get(format!("https://{target_addr}/read"))
                        .send()
                        .await
                        .unwrap()
                        .text()
                        .await
                        .unwrap(),
                    "ok"
                );
                assert_eq!(tunnels.load(Ordering::SeqCst), if i == 2 { 2 } else { 1 });
            }
        })
        .await
        .unwrap();
        socks.abort();
        target.abort();
    }
    #[test]
    fn a_new_cert_or_network_instance_cannot_reuse_the_old_tls_pool() {
        use std::sync::Arc;
        let mut clients = PeerClients::default();
        let mut device = DeviceIdentity {
            id: "peer".into(),
            name: "same peer".into(),
            ipv4: "10.0.0.2".into(),
            certificate_pem: "old".into(),
        };
        let previous = clients.slot(&device, 9800);
        device.certificate_pem = "new".into();
        let rotated = clients.slot(&device, 9800);
        assert!(!Arc::ptr_eq(&previous, &rotated));
        clients.clear(); // Replacement worker may bind the same SOCKS port.
        assert!(!Arc::ptr_eq(&rotated, &clients.slot(&device, 9800)));
    }
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

    #[tokio::test]
    async fn worker_recovery_does_not_turn_a_paired_browser_into_an_unauthorized_one() {
        let (_directory, state) = super::super::tests::fixture();
        let target = "paired-id";
        {
            let mut inner = state.remote_network.inner.lock().await;
            let mut peer = inner.config.identity();
            peer.id = target.into();
            inner.config.paired.push(peer);
            inner.secrets.peers.insert(
                target.into(),
                crate::remote_network::PeerTokens {
                    inbound: "fixture-in".into(),
                    outbound: "fixture-out".into(),
                },
            );
        }
        let get = || {
            proxy(
                State(state.clone()),
                OriginalUri("/api/remote/v1/clients".parse().unwrap()),
                Request::builder()
                    .uri("/clients")
                    .header("x-hub-target", target)
                    .body(Body::empty())
                    .unwrap(),
            )
        };
        assert_eq!(get().await.status(), StatusCode::SERVICE_UNAVAILABLE);
        state
            .remote_network
            .inner
            .lock()
            .await
            .secrets
            .peers
            .remove(target);
        assert_eq!(get().await.status(), StatusCode::UNAUTHORIZED);
    }

    fn browser_fixture(state: SharedState) -> Router {
        Router::new()
            .route("/remote/", get(|| async { "fixture" }))
            .route("/remote/browser/{scope}", get(|| async { "fixture" }))
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
                    session: "fixture-session".into(),
                    remember_key: "fixture-browser-key".into(),
                    remember_path: "/remote/browser/fixture".into(),
                    cookie_name: "fixture-cookie".into(),
                },
                local_guard,
            ))
    }

    #[tokio::test]
    async fn local_browser_lists_clients_projects_sessions_and_reads_original_history() {
        let (_directory, state) = super::super::tests::fixture();
        // Keep all runtime traffic in-process. The normal browser API should
        // carry encoded client/thread IDs and the pagination query to Codex.
        state.config.lock().await.gmclaw_bridge.enabled = false;
        let local_id = {
            let mut net = state.remote_network.inner.lock().await;
            net.config.enabled = true;
            net.config.device_id.clone()
        };
        let mut desktop = crate::remote_sessions::codex_fixture::Desktop::connect(&state).await;
        let router = browser_router(
            state.clone(),
            BrowserGuard {
                state: state.clone(),
                base: "http://127.0.0.1:12345".into(),
                session: "fixture-session".into(),
                remember_key: "fixture-browser-key".into(),
                remember_path: "/remote/browser/fixture".into(),
                cookie_name: "fixture-cookie".into(),
            },
        );
        let session_id = "original/thread";
        let get = |path: &'static str| {
            let request = Request::builder()
                .uri(path)
                .header(header::HOST, "127.0.0.1:12345")
                .header(header::COOKIE, "fixture-cookie=fixture-session")
                .header("x-hub-target", &local_id)
                .body(Body::empty())
                .unwrap();
            let router = router.clone();
            async move {
                let response = router.oneshot(request).await.unwrap();
                assert_eq!(response.status(), StatusCode::OK, "{path}");
                let bytes = axum::body::to_bytes(response.into_body(), 65536)
                    .await
                    .unwrap();
                serde_json::from_slice::<Value>(&bytes).unwrap()
            }
        };
        let native =
            async {
                for method in [
                    "model/list",
                    "thread/list",
                    "thread/list",
                    "thread/turns/list",
                    "thread/read",
                ] {
                    desktop.reply(method, |params| {
                let thread =
                    json!({"id":session_id,"cwd":"fixture/project","name":"Route fixture"});
                let reply = match method {
                    "model/list" => json!({"data":[]}),
                    "thread/list" => {
                        assert_eq!(params["cursor"], "next+page");
                        assert_eq!(params["limit"], 40);
                        json!({"data":[thread],"nextCursor":null})
                    }
                    _ => {
                        assert_eq!(params["threadId"], session_id);
                        match method {
                            "thread/read" => json!({"thread":thread}),
                            _ => json!({"data":[],"nextCursor":null}),
                        }
                    }
                };
                Ok(reply)
                }).await;
                }
            };
        let browser = async {
            assert_eq!(
                get("/api/remote/v1/devices").await["localDeviceId"],
                local_id
            );
            assert_eq!(
                get("/api/remote/v1/clients").await["clients"][0]["id"],
                "codex:default"
            );
            assert_eq!(
                get("/api/remote/v1/clients/codex%3Adefault/projects?limit=40&cursor=next%2Bpage")
                    .await["projects"][0]["id"],
                "fixture/project"
            );
            assert_eq!(
                get("/api/remote/v1/clients/codex%3Adefault/sessions?limit=40&cursor=next%2Bpage")
                    .await["sessions"][0]["id"],
                session_id
            );
            let history = get("/api/remote/v1/clients/codex%3Adefault/sessions/original%2Fthread/history?limit=100").await;
            assert_eq!(history["items"], json!([]));
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(browser, native);
        })
        .await
        .expect("browser requests must reach the synthetic runtime");
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
        {
            let mut inner = state.remote_network.inner.lock().await;
            inner.config.enabled = true;
            inner
                .browser_launches
                .insert("fixture-ticket".into(), now_ms() + 120_000);
        }
        let router = browser_fixture(state);
        let response = router
            .clone()
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
        let reused = router
            .oneshot(
                Request::builder()
                    .uri("/remote/?ticket=fixture-ticket")
                    .header(header::HOST, "127.0.0.1:12345")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(reused.status(), StatusCode::UNAUTHORIZED);
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
                    .header("cookie", "fixture-cookie=fixture-session")
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
                    .header("cookie", "fixture-cookie=fixture-session")
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
    async fn returning_browser_restores_expired_runtime_cookie_without_replaying_writes() {
        let (_directory, state) = super::super::tests::fixture();
        state.remote_network.inner.lock().await.config.enabled = true;
        let router = browser_fixture(state);
        let read = |uri: &str, cookie: &str| {
            let router = router.clone();
            let request = Request::builder()
                .uri(uri)
                .header("host", "127.0.0.1:12345")
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap();
            async move { router.oneshot(request).await.unwrap() }
        };
        let dest = "/api/remote/v1/fixture?cursor=old%2Bturn";
        let first = read(dest, "fixture-cookie=old-process").await;
        assert_eq!(first.status(), StatusCode::SEE_OTHER);
        let unlock = first.headers()[header::LOCATION].to_str().unwrap();
        assert_eq!(read(unlock, "").await.status(), StatusCode::UNAUTHORIZED);
        let restored = read(unlock, "fixture-cookie_remember=fixture-browser-key").await;
        assert_eq!(restored.status(), StatusCode::SEE_OTHER);
        assert_eq!(restored.headers()[header::LOCATION], dest);
        let cookies: Vec<_> = restored
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert!(
            cookies
                .iter()
                .any(|v| v.contains("fixture-session") && !v.contains("Max-Age"))
        );
        assert!(
            cookies
                .iter()
                .any(|v| v.contains("Path=/remote/browser/fixture; Max-Age="))
        );
        assert_eq!(
            read(dest, "fixture-cookie=fixture-session").await.status(),
            StatusCode::OK
        );
        let write = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(dest)
                    .header("host", "127.0.0.1:12345")
                    .header("cookie", "fixture-cookie=old-process")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(write.status(), StatusCode::UNAUTHORIZED);
        assert!(!write.headers().contains_key(header::LOCATION));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn browser_origin_and_recovery_key_survive_a_fresh_hub_state() {
        let (_directory, state) = super::super::tests::fixture();
        super::super::ensure_loaded(&state).await.unwrap();
        let (first, key) = browser_listener(&state, 0).await.unwrap();
        let expected = first.local_addr().unwrap();
        let config = state.config.lock().await.clone();
        let fresh = crate::app_state::AppState::new(state.config_path.clone(), config, None, None);
        drop(first);
        super::super::ensure_loaded(&fresh).await.unwrap();
        let port = fresh.remote_network.inner.lock().await.config.browser_port;
        let (second, key2) = browser_listener(&fresh, port).await.unwrap();
        assert_eq!(expected, second.local_addr().unwrap());
        assert_eq!(key, key2);
        super::super::secret_store(&fresh.config_path)
            .unwrap()
            .delete(super::super::SECRETS_KEY)
            .unwrap();
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
