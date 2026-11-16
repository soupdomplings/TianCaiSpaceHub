//! Personal device networking and an isolated browser-to-session gateway.
//! The existing Hub management listener is never published through EasyTier.
mod pairing;
mod runtime;
mod transport;

use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};

use crate::{app_state::SharedState, nvwa::secrets::SecretStore, types::now_ms};
use pairing::ConnectionCode;

pub(super) const SERVICE_PORT: u16 = 3861;
// Operator-published community node, not a Hub-owned service. The legacy
// public.easytier.cn hostname no longer resolves and must not be the default.
// Source: https://github.com/orgs/EasyTier/discussions/1848
const DEFAULT_RELAY: &str = "tcp://us01.225284.xyz:11010";
const SECRETS_KEY: &str = "remote-network-v1";

pub(crate) struct NetworkState {
    inner: Mutex<Inner>,
    operation: Mutex<()>,
    revocations: watch::Sender<u64>,
}

impl Default for NetworkState {
    fn default() -> Self {
        let (revocations, _) = watch::channel(0);
        Self {
            inner: Mutex::new(Inner::default()),
            operation: Mutex::new(()),
            revocations,
        }
    }
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    config: NetworkConfig,
    secrets: NetworkSecrets,
    running: Option<runtime::Running>,
    transport: Option<transport::Transport>,
    monitor: Option<JoinHandle<()>>,
    invitations: HashMap<String, u128>,
    pairings: HashMap<String, PendingPair>,
    outgoing: Option<OutgoingPair>,
    discovered: Vec<runtime::DiscoveredPeer>,
    network_connected: bool,
    last_error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct NetworkConfig {
    version: u32,
    enabled: bool,
    device_id: String,
    device_name: String,
    ipv4: String,
    certificate_pem: String,
    network_name: String,
    relay_url: String,
    paired: Vec<DeviceIdentity>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        let id = uuid::Uuid::new_v4();
        let bytes = id.as_bytes();
        Self {
            version: 1,
            enabled: false,
            device_id: id.to_string(),
            device_name: std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    if cfg!(windows) {
                        "我的 Windows".into()
                    } else {
                        "我的 Mac".into()
                    }
                }),
            ipv4: format!("10.203.{}.{}", bytes[0], bytes[1].clamp(1, 254)),
            certificate_pem: String::new(),
            network_name: String::new(),
            relay_url: DEFAULT_RELAY.into(),
            paired: Vec::new(),
        }
    }
}

impl NetworkConfig {
    fn identity(&self) -> DeviceIdentity {
        DeviceIdentity {
            id: self.device_id.clone(),
            name: self.device_name.clone(),
            ipv4: self.ipv4.clone(),
            certificate_pem: self.certificate_pem.clone(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceIdentity {
    id: String,
    name: String,
    ipv4: String,
    certificate_pem: String,
}

impl DeviceIdentity {
    fn validate(&self) -> Result<()> {
        ensure!(
            uuid::Uuid::parse_str(&self.id).is_ok(),
            "设备身份无效，请重新生成连接码"
        );
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 160
                && !self.name.chars().any(char::is_control),
            "设备名称无效"
        );
        let ip: Ipv4Addr = self.ipv4.parse().context("设备虚拟地址无效")?;
        ensure!(
            ip.octets()[..2] == [10, 203],
            "设备虚拟地址超出 Hub 网络范围"
        );
        ensure!(self.certificate_pem.len() < 16 * 1024, "设备证书无效");
        reqwest::Certificate::from_pem(self.certificate_pem.as_bytes())
            .context("设备证书无法读取，请重新生成连接码")?;
        Ok(())
    }
}

// Only serialized into the platform secret store, never ordinary config/status.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct NetworkSecrets {
    network_secret: String,
    tls_private_key: String,
    peers: HashMap<String, PeerTokens>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeerTokens {
    inbound: String,
    outbound: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairRequest {
    request_id: String,
    invitation_token: String,
    device: DeviceIdentity,
    return_token: String,
    poll_token: String,
}

#[derive(Clone)]
struct PendingPair {
    request: PairRequest,
    expires_at: u128,
    decision: PairDecision,
}

#[derive(Clone, PartialEq, Eq)]
enum PairDecision {
    Pending,
    Approved,
    Declined,
}

#[derive(Clone)]
struct OutgoingPair {
    target: DeviceIdentity,
    request: PairRequest,
    expires_at: u128,
    acknowledged: bool,
}

fn root(config: &Path) -> PathBuf {
    crate::desktop::state_root(config).join("remote-network")
}
fn config_file(config: &Path) -> PathBuf {
    root(config).join("network.json")
}
fn secret_store(config: &Path) -> Result<SecretStore> {
    SecretStore::new(root(config).join("protected"))
}
fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

async fn ensure_loaded(state: &SharedState) -> Result<()> {
    let mut inner = state.remote_network.inner.lock().await;
    if inner.loaded {
        return Ok(());
    }
    if let Some(bytes) = crate::desktop::transaction::read(&config_file(&state.config_path))? {
        let config: NetworkConfig =
            serde_json::from_slice(&bytes).context("远程连接设置无法读取，请检查配置文件")?;
        ensure!(config.version == 1, "远程连接设置版本不兼容，请更新 Hub");
        ensure!(
            uuid::Uuid::parse_str(&config.device_id).is_ok(),
            "远程连接设备身份损坏"
        );
        pairing::validate_relay(&config.relay_url)?;
        inner.config = config;
        if let Some(bytes) = secret_store(&state.config_path)?.get(SECRETS_KEY)? {
            inner.secrets =
                serde_json::from_slice(&bytes).context("远程连接凭据无法读取，请重新配对设备")?;
        }
    }
    inner.loaded = true;
    Ok(())
}

fn persist(state: &SharedState, inner: &Inner) -> Result<()> {
    secret_store(&state.config_path)?.set(SECRETS_KEY, &serde_json::to_vec(&inner.secrets)?)?;
    crate::desktop::transaction::atomic_write(
        &config_file(&state.config_path),
        &serde_json::to_vec_pretty(&inner.config)?,
    )
}

fn ensure_identity(inner: &mut Inner) -> Result<bool> {
    if !inner.config.certificate_pem.is_empty() && !inner.secrets.tls_private_key.is_empty() {
        return Ok(false);
    }
    let rcgen::CertifiedKey { cert, key_pair } =
        rcgen::generate_simple_self_signed(vec![inner.config.ipv4.clone(), "127.0.0.1".into()])?;
    inner.config.certificate_pem = cert.pem();
    inner.secrets.tls_private_key = key_pair.serialize_pem();
    // Only explicit enable/create/join operations call this helper. If local
    // credentials were lost, a new certificate requires fresh device pairing.
    inner.config.paired.clear();
    inner.secrets.peers.clear();
    Ok(true)
}

pub(crate) fn management_router() -> Router<SharedState> {
    Router::new().nest(
        "/api/remote-network",
        Router::new()
            .route("/status", get(status_api))
            .route("/settings", post(settings_api))
            .route("/prepare", post(prepare_api))
            .route("/uninstall", post(uninstall_api))
            .route("/create", post(create_api))
            .route("/join", post(join_api))
            .route("/pair/approve", post(approve_api))
            .route("/pair/reject", post(reject_api))
            .route("/revoke", post(revoke_api))
            .route("/open", post(open_api)),
    )
}

pub(crate) async fn startup(state: SharedState) -> Result<()> {
    ensure_loaded(&state).await?;
    let _operation = state.remote_network.operation.lock().await;
    runtime::recover_inactive(&root(&state.config_path))?;
    if let Err(error) = restart(&state).await {
        state.remote_network.inner.lock().await.last_error = Some(error.to_string());
    }
    Ok(())
}

pub(crate) async fn shutdown(state: &SharedState) {
    let _operation = state.remote_network.operation.lock().await;
    if let Err(error) = stop(state).await {
        tracing::warn!("remote network shutdown: {error:#}");
    }
}

async fn stop(state: &SharedState) -> Result<()> {
    let (running, transport, monitor) = {
        let mut inner = state.remote_network.inner.lock().await;
        inner.discovered.clear();
        inner.network_connected = false;
        inner.outgoing = None;
        inner.invitations.clear();
        inner.pairings.clear();
        (
            inner.running.take(),
            inner.transport.take(),
            inner.monitor.take(),
        )
    };
    state
        .remote_network
        .revocations
        .send_modify(|v| *v = v.wrapping_add(1));
    if let Some(monitor) = monitor {
        monitor.abort();
        let _ = monitor.await;
    }
    if let Some(transport) = transport {
        transport::stop(transport).await;
    }
    if let Some(running) = running {
        runtime::stop(running).await?;
    }
    Ok(())
}

async fn restart(state: &SharedState) -> Result<()> {
    stop(state).await?;
    let (config, secrets) = {
        let inner = state.remote_network.inner.lock().await;
        if !inner.config.enabled || inner.config.network_name.is_empty() {
            return Ok(());
        }
        ensure!(
            runtime::ready(&root(&state.config_path)),
            "远程连接组件未准备，请先确认安装"
        );
        ensure!(
            !inner.secrets.network_secret.is_empty(),
            "个人网络凭据不可用，请重新加入网络"
        );
        (inner.config.clone(), inner.secrets.clone())
    };
    let transport = transport::start(state.clone(), &config, &secrets).await?;
    // Publish the transport before await so cancellation is always cleanable.
    state.remote_network.inner.lock().await.transport = Some(transport);
    match runtime::launch(&root(&state.config_path), &config, &secrets).await {
        Ok(running) => state.remote_network.inner.lock().await.running = Some(running),
        Err(error) => {
            stop(state).await?;
            return Err(error);
        }
    }
    let weak = std::sync::Arc::downgrade(state);
    let monitor = tokio::spawn(async move {
        loop {
            let Some(state) = weak.upgrade() else {
                break;
            };
            refresh(&state).await;
            drop(state);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
    let mut inner = state.remote_network.inner.lock().await;
    inner.monitor = Some(monitor);
    inner.last_error = None;
    Ok(())
}

async fn refresh(state: &SharedState) {
    let rpc = {
        let mut inner = state.remote_network.inner.lock().await;
        inner.invitations.retain(|_, expires| *expires > now_ms());
        inner
            .pairings
            .retain(|_, pending| pending.expires_at > now_ms());
        let Some(running) = inner.running.as_mut() else {
            return;
        };
        if running.child.try_wait().ok().flatten().is_some() {
            inner.last_error = Some("远程连接组件已退出，请停用后重新启用".into());
            inner.discovered.clear();
            inner.network_connected = false;
            return;
        }
        running.rpc_port
    };
    match runtime::peers(&root(&state.config_path), rpc).await {
        Ok(snapshot) => {
            let mut inner = state.remote_network.inner.lock().await;
            inner.discovered = snapshot.peers;
            inner.network_connected = snapshot.connected;
            if inner.outgoing.is_none() {
                inner.last_error = if snapshot.connected {
                    None
                } else {
                    Some("尚未连接组网节点，请检查网络或更换节点地址".into())
                };
            }
        }
        Err(error) => {
            let mut inner = state.remote_network.inner.lock().await;
            inner.last_error = Some(error.to_string());
            inner.network_connected = false;
            inner.discovered.clear();
        }
    }
    poll_outgoing(state).await;
}

async fn poll_outgoing(state: &SharedState) {
    let (outgoing, socks) = {
        let inner = state.remote_network.inner.lock().await;
        let Some(outgoing) = inner.outgoing.clone() else {
            return;
        };
        let Some(running) = inner.running.as_ref() else {
            return;
        };
        (outgoing, running.socks_port)
    };
    if outgoing.expires_at <= now_ms() {
        let mut inner = state.remote_network.inner.lock().await;
        if inner
            .outgoing
            .as_ref()
            .is_some_and(|v| v.request.request_id == outgoing.request.request_id)
        {
            inner.outgoing = None;
            inner.last_error = Some("设备配对等待超时，请从目标 Hub 重新复制连接码".into());
        }
        return;
    }
    let result: Result<Value> = async {
        let client = transport::peer_client(&outgoing.target, socks)?;
        if !outgoing.acknowledged {
            let response = client
                .post(format!(
                    "https://{}:{SERVICE_PORT}/network/pair",
                    outgoing.target.ipv4
                ))
                .json(&outgoing.request)
                .timeout(Duration::from_secs(5))
                .send()
                .await
                .context("正在连接目标设备，请保持两端 Hub 开启")?;
            ensure!(
                response.status().is_success(),
                "目标设备未接受连接码，请重新复制后加入"
            );
            let mut inner = state.remote_network.inner.lock().await;
            if let Some(current) = inner
                .outgoing
                .as_mut()
                .filter(|v| v.request.request_id == outgoing.request.request_id)
            {
                current.acknowledged = true;
            }
        }
        Ok(client
            .get(format!(
                "https://{}:{SERVICE_PORT}/network/pair/{}",
                outgoing.target.ipv4, outgoing.request.request_id
            ))
            .bearer_auth(&outgoing.request.poll_token)
            .timeout(Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    .await;
    let mut inner = state.remote_network.inner.lock().await;
    if !inner
        .outgoing
        .as_ref()
        .is_some_and(|v| v.request.request_id == outgoing.request.request_id)
    {
        return;
    }
    match result {
        Ok(value) if value["status"] == "approved" => {
            let Some(token) = value["token"]
                .as_str()
                .filter(|v| v.len() == 64 && v.is_ascii())
            else {
                inner.last_error = Some("目标设备配对响应不完整，请更新两端 Hub".into());
                return;
            };
            inner.config.paired.retain(|v| v.id != outgoing.target.id);
            inner.config.paired.push(outgoing.target.clone());
            inner.secrets.peers.insert(
                outgoing.target.id.clone(),
                PeerTokens {
                    inbound: outgoing.request.return_token,
                    outbound: token.into(),
                },
            );
            if let Err(error) = persist(state, &inner) {
                inner.last_error = Some(error.to_string());
                return;
            }
            inner.outgoing = None;
            inner.last_error = None;
            state
                .remote_network
                .revocations
                .send_modify(|v| *v = v.wrapping_add(1));
        }
        Ok(value) if value["status"] == "declined" => {
            inner.outgoing = None;
            inner.last_error = Some("目标设备已拒绝配对".into());
        }
        Ok(_) => inner.last_error = None,
        Err(error) => inner.last_error = Some(error.to_string()),
    }
}

fn device_rows(inner: &Inner) -> Vec<Value> {
    let mut devices = vec![
        json!({"id":inner.config.device_id,"name":format!("{}（本机）",inner.config.device_name),"online":inner.transport.is_some(),"paired":true,"connectionType":"local"}),
    ];
    for device in &inner.config.paired {
        let peer = inner.discovered.iter().find(|p| p.ipv4 == device.ipv4);
        devices.push(json!({"id":device.id,"name":device.name,"online":peer.is_some(),"paired":true,"connectionType":peer.map(|v| if v.direct {"direct"} else {"relay"})}));
    }
    if let Some(outgoing) = &inner.outgoing {
        if !inner
            .config
            .paired
            .iter()
            .any(|v| v.id == outgoing.target.id)
        {
            devices.push(json!({"id":outgoing.target.id,"name":outgoing.target.name,"online":inner.discovered.iter().any(|v|v.ipv4==outgoing.target.ipv4),"paired":false,"connectionType":"pairing"}));
        }
    }
    for peer in &inner.discovered {
        if !inner.config.paired.iter().any(|v| v.ipv4 == peer.ipv4)
            && !inner
                .outgoing
                .as_ref()
                .is_some_and(|v| v.target.ipv4 == peer.ipv4)
        {
            devices.push(json!({"id":format!("unpaired:{}",peer.ipv4),"name":peer.hostname,"online":true,"paired":false,"connectionType":if peer.direct {"direct"} else {"relay"}}));
        }
    }
    devices
}

fn network_running(inner: &mut Inner) -> bool {
    inner
        .running
        .as_mut()
        .is_some_and(|running| matches!(running.child.try_wait(), Ok(None)))
        && inner.transport.is_some()
}

async fn status(state: &SharedState) -> Result<Value> {
    ensure_loaded(state).await?;
    let mut inner = state.remote_network.inner.lock().await;
    let ready = runtime::ready(&root(&state.config_path));
    let running = network_running(&mut inner);
    Ok(
        json!({"enabled":inner.config.enabled,"runtimeReady":ready,"runtimeInstalled":runtime::package_dir(&root(&state.config_path)).is_ok_and(|v|v.join("hub-owner.json").is_file()),"runtimeVersion":runtime::VERSION,"networkName":inner.config.network_name,"deviceName":inner.config.device_name,"deviceId":inner.config.device_id,"relayUrl":inner.config.relay_url,"networkReady":running,"connected":running && inner.network_connected,"devices":device_rows(&inner),"pendingPairings":inner.pairings.values().filter(|v|v.decision==PairDecision::Pending).map(|v|json!({"requestId":v.request.request_id,"deviceName":v.request.device.name,"expiresAt":v.expires_at})).collect::<Vec<_>>(),"pairing":inner.outgoing.is_some(),"browserUrl":inner.transport.as_ref().map(|v|v.browser_base.clone()),"lastError":inner.last_error}),
    )
}

#[derive(Debug)]
struct ApiError(anyhow::Error);
impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        Self(value)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::CONFLICT,
            Json(json!({"error":{"code":"remote_network_error","message":self.0.to_string()}})),
        )
            .into_response()
    }
}
type ApiResult = std::result::Result<Json<Value>, ApiError>;

fn check(condition: bool, message: &str) -> Result<()> {
    ensure!(condition, "{message}");
    Ok(())
}

async fn status_api(State(state): State<SharedState>) -> ApiResult {
    Ok(Json(status(&state).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    enabled: bool,
    device_name: Option<String>,
    relay_url: Option<String>,
}
async fn settings_api(State(state): State<SharedState>, Json(input): Json<Settings>) -> ApiResult {
    ensure_loaded(&state).await?;
    let _operation = state.remote_network.operation.lock().await;
    let needs_restart = {
        let mut inner = state.remote_network.inner.lock().await;
        let was_enabled = inner.config.enabled;
        let previous_relay = inner.config.relay_url.clone();
        if let Some(name) = input.device_name {
            check(
                !name.trim().is_empty() && name.len() <= 160 && !name.chars().any(char::is_control),
                "请输入有效设备名称",
            )?;
            inner.config.device_name = name.trim().into();
        }
        if let Some(relay) = input.relay_url {
            let relay = if relay.trim().is_empty() {
                DEFAULT_RELAY
            } else {
                relay.trim()
            };
            pairing::validate_relay(relay)?;
            inner.config.relay_url = relay.into();
        }
        inner.config.enabled = input.enabled;
        let identity_changed = input.enabled && ensure_identity(&mut inner)?;
        let running = network_running(&mut inner);
        let needs_restart = was_enabled != input.enabled
            || previous_relay != inner.config.relay_url
            || identity_changed
            || (input.enabled && !inner.config.network_name.is_empty() && !running);
        persist(&state, &inner)?;
        needs_restart
    };
    if needs_restart {
        if let Err(error) = restart(&state).await {
            state.remote_network.inner.lock().await.last_error = Some(error.to_string());
        }
    }
    Ok(Json(status(&state).await?))
}

#[derive(Deserialize)]
struct Preparation {
    confirmed: bool,
}
async fn prepare_api(
    State(state): State<SharedState>,
    Json(input): Json<Preparation>,
) -> ApiResult {
    ensure_loaded(&state).await?;
    let _operation = state.remote_network.operation.lock().await;
    check(input.confirmed, "仅在用户确认后准备远程连接组件")?;
    check(
        state.remote_network.inner.lock().await.config.enabled,
        "请先启用远程连接",
    )?;
    runtime::prepare(&root(&state.config_path)).await?;
    if let Err(error) = restart(&state).await {
        state.remote_network.inner.lock().await.last_error = Some(error.to_string());
    }
    Ok(Json(status(&state).await?))
}

async fn uninstall_api(State(state): State<SharedState>) -> ApiResult {
    ensure_loaded(&state).await?;
    let _operation = state.remote_network.operation.lock().await;
    check(
        !state.remote_network.inner.lock().await.config.enabled,
        "请先停用远程连接，再移除 Hub 安装的组件",
    )?;
    stop(&state).await?;
    runtime::uninstall(&root(&state.config_path))?;
    Ok(Json(status(&state).await?))
}

async fn create_api(State(state): State<SharedState>) -> ApiResult {
    ensure_loaded(&state).await?;
    let _operation = state.remote_network.operation.lock().await;
    let needs_restart = {
        let mut inner = state.remote_network.inner.lock().await;
        check(inner.config.enabled, "请先启用远程连接")?;
        check(
            runtime::ready(&root(&state.config_path)),
            "远程连接组件未准备，请先确认安装",
        )?;
        let identity_changed = ensure_identity(&mut inner)?;
        let network_changed =
            inner.config.network_name.is_empty() || inner.secrets.network_secret.is_empty();
        if network_changed {
            inner.config.network_name = format!("hub-{}", uuid::Uuid::new_v4().simple());
            inner.secrets.network_secret = random_token();
            inner.config.paired.clear();
            inner.secrets.peers.clear();
        }
        persist(&state, &inner)?;
        identity_changed || network_changed || !network_running(&mut inner)
    };
    if needs_restart {
        restart(&state).await?;
    }
    let code = {
        let mut inner = state.remote_network.inner.lock().await;
        let code = ConnectionCode::issue(&inner.config, &inner.secrets)?;
        inner.invitations.retain(|_, expiry| *expiry > now_ms());
        inner
            .invitations
            .insert(code.invitation_token.clone(), code.expires_at);
        code.encode()?
    };
    let mut value = status(&state).await?;
    value["code"] = Value::String(code);
    Ok(Json(value))
}

#[derive(Deserialize)]
struct JoinInput {
    code: String,
}
async fn join_api(State(state): State<SharedState>, Json(input): Json<JoinInput>) -> ApiResult {
    let code = ConnectionCode::decode(&input.code, now_ms())?;
    ensure_loaded(&state).await?;
    let _operation = state.remote_network.operation.lock().await;
    let needs_restart = {
        let mut inner = state.remote_network.inner.lock().await;
        check(inner.config.enabled, "请先启用远程连接")?;
        check(
            runtime::ready(&root(&state.config_path)),
            "远程连接组件未准备，请先确认安装",
        )?;
        check(
            inner.config.device_id != code.target.id,
            "这是本机的连接码，请在另一台设备输入",
        )?;
        let address_changed = avoid_address_conflict(&mut inner, &code.target.ipv4);
        let identity_changed = ensure_identity(&mut inner)?;
        let changed = inner.config.network_name != code.network_name
            || inner.secrets.network_secret != code.network_secret;
        if changed {
            inner.config.paired.clear();
            inner.secrets.peers.clear();
        }
        let relay_changed = inner.config.relay_url != code.relay_url;
        inner.config.network_name = code.network_name.clone();
        inner.config.relay_url = code.relay_url.clone();
        inner.secrets.network_secret = code.network_secret.clone();
        persist(&state, &inner)?;
        changed
            || relay_changed
            || address_changed
            || identity_changed
            || !network_running(&mut inner)
    };
    if needs_restart {
        restart(&state).await?;
    }
    {
        let mut inner = state.remote_network.inner.lock().await;
        inner.outgoing = Some(OutgoingPair {
            target: code.target,
            request: PairRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                invitation_token: code.invitation_token,
                device: inner.config.identity(),
                return_token: random_token(),
                poll_token: random_token(),
            },
            expires_at: code.expires_at,
            acknowledged: false,
        });
        inner.last_error = None;
    }
    // A background worker retries the network handshake, never a conversation.
    Ok(Json(status(&state).await?))
}

fn avoid_address_conflict(inner: &mut Inner, target: &str) -> bool {
    if inner.config.ipv4 != target {
        return false;
    }
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    let mut address = Ipv4Addr::new(10, 203, bytes[0], bytes[1].clamp(1, 254));
    if address.to_string() == target {
        address = Ipv4Addr::new(10, 203, bytes[0], if bytes[1] == 1 { 2 } else { 1 });
    }
    inner.config.ipv4 = address.to_string();
    inner.config.certificate_pem.clear();
    inner.secrets.tls_private_key.clear();
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairInput {
    request_id: String,
}
async fn decide_pair(state: &SharedState, request_id: &str, approve: bool) -> Result<()> {
    ensure_loaded(state).await?;
    let _operation = state.remote_network.operation.lock().await;
    let mut inner = state.remote_network.inner.lock().await;
    ensure!(inner.config.enabled, "远程连接已停用");
    let request = {
        let pending = inner
            .pairings
            .get(request_id)
            .context("配对请求已失效，请让另一台设备重试")?;
        ensure!(pending.expires_at > now_ms(), "配对请求已过期");
        ensure!(
            pending.decision == PairDecision::Pending,
            "此配对请求已处理"
        );
        pending.request.clone()
    };
    if approve {
        inner.config.paired.retain(|v| v.id != request.device.id);
        inner.config.paired.push(request.device.clone());
        inner.secrets.peers.insert(
            request.device.id,
            PeerTokens {
                inbound: random_token(),
                outbound: request.return_token,
            },
        );
        persist(state, &inner)?;
        state
            .remote_network
            .revocations
            .send_modify(|v| *v = v.wrapping_add(1));
    }
    inner
        .pairings
        .get_mut(request_id)
        .context("配对请求已失效")?
        .decision = if approve {
        PairDecision::Approved
    } else {
        PairDecision::Declined
    };
    Ok(())
}

async fn approve_api(State(state): State<SharedState>, Json(input): Json<PairInput>) -> ApiResult {
    decide_pair(&state, &input.request_id, true).await?;
    Ok(Json(status(&state).await?))
}
async fn reject_api(State(state): State<SharedState>, Json(input): Json<PairInput>) -> ApiResult {
    decide_pair(&state, &input.request_id, false).await?;
    Ok(Json(status(&state).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceInput {
    device_id: Option<String>,
}
async fn revoke_api(State(state): State<SharedState>, Json(input): Json<DeviceInput>) -> ApiResult {
    ensure_loaded(&state).await?;
    let _operation = state.remote_network.operation.lock().await;
    {
        let mut inner = state.remote_network.inner.lock().await;
        let id = input.device_id.context("请选择要撤销的设备")?;
        inner.config.paired.retain(|v| v.id != id);
        inner.secrets.peers.remove(&id);
        inner.pairings.retain(|_, v| v.request.device.id != id);
        if inner.outgoing.as_ref().is_some_and(|v| v.target.id == id) {
            inner.outgoing = None;
        }
        persist(&state, &inner)?;
    }
    state
        .remote_network
        .revocations
        .send_modify(|v| *v = v.wrapping_add(1));
    Ok(Json(status(&state).await?))
}

async fn open_api(State(state): State<SharedState>, Json(input): Json<DeviceInput>) -> ApiResult {
    ensure_loaded(&state).await?;
    let inner = state.remote_network.inner.lock().await;
    check(inner.config.enabled, "请先启用远程连接")?;
    let transport = inner
        .transport
        .as_ref()
        .context("个人网络尚未就绪，请先创建或加入网络")?;
    if let Some(id) = &input.device_id {
        check(
            id == &inner.config.device_id || inner.config.paired.iter().any(|v| &v.id == id),
            "请先完成设备配对",
        )?;
    }
    let mut url = url::Url::parse(&format!("{}/remote/", transport.browser_base))
        .context("本机远程网页地址无效，请重新启用远程连接")?;
    url.query_pairs_mut()
        .append_pair("ticket", &transport.browser_ticket);
    if let Some(id) = input.device_id {
        url.query_pairs_mut().append_pair("device", &id);
    }
    Ok(Json(json!({"url":url.as_str()})))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn fixture() -> (tempfile::TempDir, SharedState) {
        let directory = tempfile::tempdir().unwrap();
        let mut config = crate::config::AppConfig::default();
        config.state_path = directory.path().join("state.json");
        let state = crate::app_state::AppState::new(
            directory.path().join("config.toml"),
            config,
            None,
            None,
        );
        (directory, state)
    }

    #[tokio::test]
    async fn disabled_startup_creates_no_runtime_or_network_files() {
        let (_directory, state) = fixture();
        startup(state.clone()).await.unwrap();
        let value = status(&state).await.unwrap();
        assert_eq!(value["enabled"], false);
        assert_eq!(value["runtimeReady"], false);
        assert_eq!(value["connected"], false);
        assert!(!root(&state.config_path).exists());
        assert!(state.remote_network.inner.lock().await.running.is_none());
        shutdown(&state).await;
    }

    #[tokio::test]
    async fn cancelled_preparation_and_disabled_preparation_do_no_download_work() {
        let (_directory, state) = fixture();
        let cancelled =
            prepare_api(State(state.clone()), Json(Preparation { confirmed: false })).await;
        assert!(cancelled.err().unwrap().0.to_string().contains("确认"));
        let disabled =
            prepare_api(State(state.clone()), Json(Preparation { confirmed: true })).await;
        assert!(disabled.err().unwrap().0.to_string().contains("启用"));
        assert!(!root(&state.config_path).exists());
    }

    #[tokio::test]
    async fn status_does_not_serialize_network_or_device_secrets() {
        let (_directory, state) = fixture();
        ensure_loaded(&state).await.unwrap();
        {
            let mut inner = state.remote_network.inner.lock().await;
            inner.secrets.network_secret = "fixture-network-private-value".into();
            inner.secrets.tls_private_key = "fixture-tls-private-value".into();
            inner.secrets.peers.insert(
                "fixture-peer".into(),
                PeerTokens {
                    inbound: "fixture-inbound-secret".into(),
                    outbound: "fixture-outbound-secret".into(),
                },
            );
        }
        let serialized = serde_json::to_string(&status(&state).await.unwrap()).unwrap();
        for secret in [
            "fixture-network-private-value",
            "fixture-tls-private-value",
            "fixture-inbound-secret",
            "fixture-outbound-secret",
        ] {
            assert!(!serialized.contains(secret));
        }
    }

    #[test]
    fn missing_credentials_and_address_conflicts_require_fresh_pairing() {
        let mut inner = Inner::default();
        assert!(ensure_identity(&mut inner).unwrap());
        assert!(!ensure_identity(&mut inner).unwrap());
        inner.config.paired.push(inner.config.identity());
        inner.secrets.peers.insert(
            "fixture".into(),
            PeerTokens {
                inbound: random_token(),
                outbound: random_token(),
            },
        );
        let old_certificate = inner.config.certificate_pem.clone();
        inner.secrets.tls_private_key.clear();
        assert!(ensure_identity(&mut inner).unwrap());
        assert_ne!(inner.config.certificate_pem, old_certificate);
        assert!(inner.config.paired.is_empty());
        assert!(inner.secrets.peers.is_empty());
        let old_address = inner.config.ipv4.clone();
        assert!(avoid_address_conflict(&mut inner, &old_address));
        assert_ne!(inner.config.ipv4, old_address);
        assert!(ensure_identity(&mut inner).unwrap());
        assert!(!avoid_address_conflict(&mut inner, &old_address));
        inner.config.identity().validate().unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn pairing_is_confirmed_once_persists_and_revocation_notifies_streams() {
        let (_directory, state) = fixture();
        ensure_loaded(&state).await.unwrap();
        let peer_id = uuid::Uuid::new_v4().to_string();
        let request_id = uuid::Uuid::new_v4().to_string();
        {
            let mut inner = state.remote_network.inner.lock().await;
            inner.config.enabled = true;
            ensure_identity(&mut inner).unwrap();
            inner.pairings.insert(
                request_id.clone(),
                PendingPair {
                    request: PairRequest {
                        request_id: request_id.clone(),
                        invitation_token: random_token(),
                        device: DeviceIdentity {
                            id: peer_id.clone(),
                            name: "合成设备".into(),
                            ipv4: "10.203.0.17".into(),
                            certificate_pem: "synthetic-public-certificate".into(),
                        },
                        return_token: random_token(),
                        poll_token: random_token(),
                    },
                    expires_at: now_ms() + 60_000,
                    decision: PairDecision::Pending,
                },
            );
            assert!(inner.secrets.peers.is_empty());
        }
        let approval_receiver = state.remote_network.revocations.subscribe();
        decide_pair(&state, &request_id, true).await.unwrap();
        assert!(approval_receiver.has_changed().unwrap());
        assert!(decide_pair(&state, &request_id, true).await.is_err());
        let bytes = std::fs::read(config_file(&state.config_path)).unwrap();
        let saved: NetworkConfig = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved.paired[0].id, peer_id);
        let private: NetworkSecrets = serde_json::from_slice(
            &secret_store(&state.config_path)
                .unwrap()
                .get(SECRETS_KEY)
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(private.peers.contains_key(&peer_id));
        let receiver = state.remote_network.revocations.subscribe();
        let _ = revoke_api(
            State(state.clone()),
            Json(DeviceInput {
                device_id: Some(peer_id.clone()),
            }),
        )
        .await
        .unwrap();
        assert!(receiver.has_changed().unwrap());
        assert!(
            !state
                .remote_network
                .inner
                .lock()
                .await
                .secrets
                .peers
                .contains_key(&peer_id)
        );
        secret_store(&state.config_path)
            .unwrap()
            .delete(SECRETS_KEY)
            .unwrap();
    }
}
