//! Personal device networking and an isolated browser-to-session gateway.
//! The existing Hub management listener is never published through EasyTier.
mod display;
mod mobile;
mod pairing;
mod runtime;
mod transport;
mod wire;

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
pub(crate) use display::{DisplayContent, get as display_content};

pub(super) const SERVICE_PORT: u16 = 3861;
// Operator-published community node, not a Hub-owned service. The legacy
// public.easytier.cn hostname no longer resolves and must not be the default.
// Source: https://github.com/orgs/EasyTier/discussions/1848
const DEFAULT_RELAY: &str = "tcp://us01.225284.xyz:11010";
const SECRETS_KEY: &str = "remote-network-v1";

pub(crate) struct NetworkState {
    mobile: mobile::Mobile,
    inner: Mutex<Inner>,
    operation: Mutex<()>,
    revocations: watch::Sender<u64>,
}

impl Default for NetworkState {
    fn default() -> Self {
        let (revocations, _) = watch::channel(0);
        Self {
            mobile: mobile::Mobile::default(),
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
    proxy_clients: transport::PeerClients,
    monitor: Option<JoinHandle<()>>,
    invitations: HashMap<String, u128>,
    browser_launches: HashMap<String, u128>,
    pairings: HashMap<String, PendingPair>,
    pairing_error: Option<String>,
    outgoing: Option<OutgoingPair>,
    discovered: Vec<runtime::DiscoveredPeer>,
    network_connected: bool,
    last_error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct NetworkConfig {
    mobile_registered: bool,
    version: u32,
    enabled: bool,
    device_id: String,
    device_name: String,
    ipv4: String,
    certificate_pem: String,
    network_name: String,
    relay_url: String,
    paired: Vec<DeviceIdentity>,
    hidden_peers: Vec<String>,
    browser_port: u16,
    workspace_dirs: Vec<String>,
    display_content: DisplayContent,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        let id = uuid::Uuid::new_v4();
        let bytes = id.as_bytes();
        Self {
            mobile_registered: false,
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
            hidden_peers: Vec::new(),
            browser_port: 0,
            workspace_dirs: Vec::new(),
            display_content: DisplayContent::default(),
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
    browser_key: String,
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
            .merge(mobile::routes())
            .route("/status", get(status_api))
            .route("/settings", post(settings_api))
            .route("/display-content", post(display::update))
            .route("/workspace", post(workspace_api))
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
    mobile::sync(&state).await;
    Ok(())
}

pub(crate) async fn shutdown(state: &SharedState) {
    let _operation = state.remote_network.operation.lock().await;
    mobile::stop(state).await;
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
        inner.pairing_error = None;
        inner.invitations.clear();
        inner.browser_launches.clear();
        inner.pairings.clear();
        inner.proxy_clients.clear();
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
            "当前系统暂不支持内置组网"
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
    let first_start = runtime::launch(&root(&state.config_path), &config, &secrets).await;
    let mut inner = state.remote_network.inner.lock().await;
    match first_start {
        Ok(running) => {
            inner.running = Some(running);
            inner.last_error = None;
        }
        Err(error) => {
            inner.last_error = Some(format!("组网暂未就绪，将自动重试：{error}"));
        }
    }
    let weak = std::sync::Arc::downgrade(state);
    let monitor = tokio::spawn(async move {
        let mut delay = 5;
        loop {
            let Some(state) = weak.upgrade() else {
                break;
            };
            if recover_worker(&state).await {
                delay = 5;
                refresh(&state).await;
            } else {
                delay = (delay * 2).min(60);
            }
            drop(state);
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
    });
    inner.monitor = Some(monitor);
    Ok(())
}

/// Restore only the transport to the overlay. The browser listener, device
/// credentials, and any pending pairing are not tied to a worker's lifetime.
async fn recover_worker(state: &SharedState) -> bool {
    let Ok(_operation) = state.remote_network.operation.try_lock() else {
        return false;
    };
    let (previous, config, secrets) = {
        let mut inner = state.remote_network.inner.lock().await;
        if !inner.config.enabled || inner.transport.is_none() {
            return false;
        }
        if inner
            .running
            .as_ref()
            .is_some_and(runtime::Running::is_running)
        {
            return true;
        }
        inner.discovered.clear();
        inner.network_connected = false;
        inner.proxy_clients.clear();
        (
            inner.running.take(),
            inner.config.clone(),
            inner.secrets.clone(),
        )
    };
    if let Some(previous) = previous {
        let _ = runtime::stop(previous).await;
    }
    let running = runtime::launch(&root(&state.config_path), &config, &secrets).await;
    let mut inner = state.remote_network.inner.lock().await;
    match running {
        Ok(running) => {
            inner.running = Some(running);
            true
        }
        Err(error) => {
            inner.last_error = Some(format!("组网暂未就绪，将自动重试：{error}"));
            false
        }
    }
}

async fn refresh(state: &SharedState) {
    let snapshot = {
        let mut inner = state.remote_network.inner.lock().await;
        inner.invitations.retain(|_, expires| *expires > now_ms());
        inner
            .pairings
            .retain(|_, pending| pending.expires_at > now_ms());
        let Some(running) = inner.running.as_mut() else {
            return;
        };
        if !running.is_running() {
            inner.last_error = Some("内置组网中断，正在重新连接".into());
            inner.discovered.clear();
            inner.network_connected = false;
            return;
        }
        runtime::snapshot(running)
    };
    match snapshot {
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
            inner.pairing_error =
                Some("配对请求已过期。请只在一台电脑提供新的连接码，另一台输入。".into());
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
            if response.status().is_client_error() {
                return Ok(json!({"status":"invalid"}));
            }
            response
                .error_for_status()
                .context("目标电脑暂不可用，将继续发送配对请求")?;
            let mut inner = state.remote_network.inner.lock().await;
            if let Some(current) = inner
                .outgoing
                .as_mut()
                .filter(|v| v.request.request_id == outgoing.request.request_id)
            {
                current.acknowledged = true;
            }
        }
        let response = client
            .get(format!(
                "https://{}:{SERVICE_PORT}/network/pair/{}",
                outgoing.target.ipv4, outgoing.request.request_id
            ))
            .bearer_auth(&outgoing.request.poll_token)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        if response.status().is_client_error() {
            return Ok(json!({"status":"invalid"}));
        }
        Ok(response.error_for_status()?.json().await?)
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
            inner
                .pairings
                .retain(|_, p| p.request.device.id != outgoing.target.id);
            inner.config.paired.retain(|v| v.id != outgoing.target.id);
            inner
                .config
                .hidden_peers
                .retain(|ip| ip != &outgoing.target.ipv4);
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
            inner.pairing_error = None;
            inner.last_error = None;
            state
                .remote_network
                .revocations
                .send_modify(|v| *v = v.wrapping_add(1));
        }
        Ok(value) if value["status"] == "declined" => {
            inner.outgoing = None;
            inner.pairing_error = Some("目标设备已拒绝配对".into());
        }
        Ok(value) if value["status"] == "invalid" => {
            inner.outgoing = None;
            inner.pairing_error = Some(
                "对方没有这条有效请求。请只在一台电脑生成新连接码，由另一台输入，不要互换连接码。"
                    .into(),
            );
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
    let mut incoming_ips = Vec::new();
    let now = now_ms();
    for p in inner
        .pairings
        .values()
        .filter(|p| p.decision == PairDecision::Pending && p.expires_at > now)
    {
        let d = &p.request.device;
        if !incoming_ips.contains(&d.ipv4.as_str())
            && !inner.config.paired.iter().any(|v| v.id == d.id)
        {
            incoming_ips.push(d.ipv4.as_str());
            devices.push(json!({"id":d.id,"name":d.name,"online":inner.discovered.iter().any(|v|v.ipv4==d.ipv4),"paired":false,"connectionType":"incoming"}));
        }
    }
    if let Some(outgoing) = &inner.outgoing {
        if !inner
            .config
            .paired
            .iter()
            .any(|v| v.id == outgoing.target.id)
            && !incoming_ips.contains(&outgoing.target.ipv4.as_str())
        {
            devices.push(json!({"id":outgoing.target.id,"name":outgoing.target.name,"online":inner.discovered.iter().any(|v|v.ipv4==outgoing.target.ipv4),"paired":false,"connectionType":if outgoing.acknowledged {"pairing"} else {"sending"}}));
        }
    }
    for peer in &inner.discovered {
        if !inner.config.hidden_peers.contains(&peer.ipv4)
            && !incoming_ips.contains(&peer.ipv4.as_str())
            && !inner.config.paired.iter().any(|v| v.ipv4 == peer.ipv4)
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
        .is_some_and(|running| running.is_running())
        && inner.transport.is_some()
}

async fn status(state: &SharedState) -> Result<Value> {
    ensure_loaded(state).await?;
    let mut inner = state.remote_network.inner.lock().await;
    let now = now_ms();
    inner.pairings.retain(|_, p| p.expires_at > now);
    if inner.outgoing.as_ref().is_some_and(|o| o.expires_at <= now) {
        inner.outgoing = None;
        inner.pairing_error =
            Some("配对请求已过期。请只在一台电脑提供新的连接码，另一台输入。".into());
    }
    let ready = runtime::ready(&root(&state.config_path));
    let running = network_running(&mut inner);
    Ok(
        json!({"enabled":inner.config.enabled,"displayContent":inner.config.display_content,"runtimeReady":ready,"runtimeEmbedded":ready,"runtimeInstalled":runtime::legacy_installed(&root(&state.config_path)),"runtimeVersion":runtime::VERSION,"networkName":inner.config.network_name,"workspaceDirs":inner.config.workspace_dirs,"deviceName":inner.config.device_name,"deviceId":inner.config.device_id,"relayUrl":inner.config.relay_url,"networkReady":running,"connected":running && inner.network_connected,"devices":device_rows(&inner),"pendingPairings":inner.pairings.values().filter(|v|v.decision==PairDecision::Pending).map(|v|json!({"requestId":v.request.request_id,"deviceName":v.request.device.name,"deviceId":v.request.device.id,"expiresAt":v.expires_at})).collect::<Vec<_>>(),"pairing":inner.outgoing.is_some(),"outgoingPairing":inner.outgoing.as_ref().map(|o|json!({"deviceId":o.target.id,"deviceName":o.target.name,"delivered":o.acknowledged})), "pairingError":inner.pairing_error,"browserUrl":inner.transport.as_ref().map(|v|v.browser_base.clone()),"lastError":inner.last_error}),
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
    let mut status = status(&state).await?;
    status["mobile"] = mobile::info(&state).await;
    Ok(Json(status))
}

// Only the local management router may change this list. A paired browser
// obtains the selected directory names from its target's Codex project list.
pub(crate) async fn workspace_dirs(state: &SharedState) -> Result<Vec<String>> {
    ensure_loaded(state).await?;
    Ok(state
        .remote_network
        .inner
        .lock()
        .await
        .config
        .workspace_dirs
        .clone())
}
#[derive(Deserialize)]
struct WorkspaceEdit {
    path: String,
    #[serde(default)]
    remove: bool,
}
async fn workspace_api(
    State(state): State<SharedState>,
    Json(edit): Json<WorkspaceEdit>,
) -> ApiResult {
    ensure_loaded(&state).await?;
    let mut inner = state.remote_network.inner.lock().await;
    let mut updated = inner.config.clone();
    if edit.remove {
        updated.workspace_dirs.retain(|v| v != &edit.path);
    } else {
        let dir = Path::new(&edit.path);
        ensure_workspace(dir)?;
        if !updated.workspace_dirs.contains(&edit.path) {
            check(
                updated.workspace_dirs.len() < 100,
                "最多可添加 100 个工作目录",
            )?;
            updated.workspace_dirs.push(edit.path);
        }
    }
    if updated.workspace_dirs != inner.config.workspace_dirs {
        crate::desktop::transaction::atomic_write(
            &config_file(&state.config_path),
            &serde_json::to_vec_pretty(&updated).map_err(anyhow::Error::from)?,
        )?;
        inner.config = updated;
    }
    Ok(Json(json!({"workspaceDirs":inner.config.workspace_dirs})))
}
fn ensure_workspace(dir: &Path) -> Result<()> {
    ensure!(
        dir.is_absolute() && dir.is_dir(),
        "请选择本机上现有的文件夹"
    );
    Ok(())
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
    mobile::sync(&state).await;
    Ok(Json(status(&state).await?))
}

// Backward-compatible endpoint for earlier local callers. Networking is now
// compiled into Hub, so this never downloads or asks for installation consent.
async fn prepare_api(State(state): State<SharedState>, Json(_input): Json<Value>) -> ApiResult {
    ensure_loaded(&state).await?;
    let _operation = state.remote_network.operation.lock().await;
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
        "请先停用远程连接，再清理旧版 Hub 下载的组件",
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
            "当前系统暂不支持内置组网",
        )?;
        let identity_changed = ensure_identity(&mut inner)?;
        let network_changed =
            inner.config.network_name.is_empty() || inner.secrets.network_secret.is_empty();
        if network_changed {
            inner.config.network_name = format!("hub-{}", uuid::Uuid::new_v4().simple());
            inner.secrets.network_secret = random_token();
            inner.config.paired.clear();
            inner.config.hidden_peers.clear();
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
        inner.pairing_error = None;
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
            "当前系统暂不支持内置组网",
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
            inner.config.hidden_peers.clear();
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
        if inner.outgoing.as_ref().is_some_and(|o| {
            o.target.id == code.target.id
                && o.request.invitation_token == code.invitation_token
                && o.expires_at > now_ms()
        }) {
            drop(inner);
            return Ok(Json(status(&state).await?));
        }
        inner.pairing_error = None;
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
    // When both computers entered each other's code they already proposed one
    // token each. Accepting either request must produce the same two directional
    // credentials, even if both users click Allow before the next poll.
    let crossed = inner
        .outgoing
        .as_ref()
        .filter(|o| {
            o.expires_at > now_ms()
                && o.target.id == request.device.id
                && o.target.ipv4 == request.device.ipv4
                && o.target.certificate_pem == request.device.certificate_pem
        })
        .map(|o| o.request.return_token.clone());
    if approve {
        inner
            .pairings
            .retain(|id, p| id == request_id || p.request.device.id != request.device.id);
        inner.config.paired.retain(|v| v.id != request.device.id);
        inner
            .config
            .hidden_peers
            .retain(|ip| ip != &request.device.ipv4);
        inner.config.paired.push(request.device.clone());
        inner.secrets.peers.insert(
            request.device.id,
            PeerTokens {
                inbound: crossed.clone().unwrap_or_else(random_token),
                outbound: request.return_token,
            },
        );
        persist(state, &inner)?;
        state
            .remote_network
            .revocations
            .send_modify(|v| *v = v.wrapping_add(1));
    }
    if crossed.is_some() {
        inner.outgoing = None;
    }
    inner.pairing_error = None;
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
        check(id != inner.config.device_id, "不能删除本机")?;
        let pos = inner
            .config
            .paired
            .iter()
            .position(|v| v.id == id)
            .context("设备不在已授权列表中")?;
        let removed = inner.config.paired.remove(pos);
        if !inner.config.hidden_peers.contains(&removed.ipv4) {
            inner.config.hidden_peers.push(removed.ipv4);
        }
        inner.secrets.peers.remove(&id);
        inner.proxy_clients.remove(&id);
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
    let mut inner = state.remote_network.inner.lock().await;
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
    let now = now_ms();
    inner.browser_launches.retain(|_, until| *until > now);
    let ticket = random_token();
    inner.browser_launches.insert(ticket.clone(), now + 120_000);
    url.query_pairs_mut().append_pair("ticket", &ticket);
    if let Some(id) = input.device_id {
        url.query_pairs_mut().append_pair("device", &id);
    }
    Ok(Json(json!({"url":url.as_str()})))
}

#[cfg(test)]
pub(crate) fn mobile_test_fixture() -> (tempfile::TempDir, SharedState) { tests::fixture() }

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
    async fn workspace_picker_local_edits_persist_without_starting_network_or_deleting_files() {
        let (directory, state) = fixture();
        let chosen = directory.path().join("test-workspace");
        std::fs::create_dir(&chosen).unwrap();
        let path = chosen.to_str().unwrap();
        let edit = |path: &str, remove: bool| {
            workspace_api(
                State(state.clone()),
                Json(WorkspaceEdit {
                    path: path.to_owned(),
                    remove,
                }),
            )
        };
        assert!(edit("relative", false).await.is_err());
        for _ in 0..2 {
            assert_eq!(
                edit(path, false).await.unwrap().0["workspaceDirs"],
                json!([path])
            );
        }
        let saved: NetworkConfig =
            serde_json::from_slice(&std::fs::read(config_file(&state.config_path)).unwrap())
                .unwrap();
        assert_eq!(saved.workspace_dirs, [path]);
        assert!(!saved.enabled);
        assert!(
            edit(path, true).await.unwrap().0["workspaceDirs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(chosen.is_dir());
        assert!(state.remote_network.inner.lock().await.running.is_none());
    }

    #[tokio::test]
    async fn disabled_startup_creates_no_runtime_or_network_files() {
        let (_directory, state) = fixture();
        startup(state.clone()).await.unwrap();
        let value = status(&state).await.unwrap();
        assert_eq!(value["enabled"], false);
        assert_eq!(
            value["runtimeReady"],
            cfg!(any(windows, target_os = "macos"))
        );
        assert_eq!(value["runtimeEmbedded"], value["runtimeReady"]);
        assert_eq!(value["runtimeInstalled"], false);
        assert_eq!(value["connected"], false);
        assert!(!root(&state.config_path).exists());
        assert!(state.remote_network.inner.lock().await.running.is_none());
        shutdown(&state).await;
    }

    #[tokio::test]
    async fn legacy_preparation_cannot_enable_networking_or_create_runtime_files() {
        let (_directory, state) = fixture();
        let cancelled = prepare_api(State(state.clone()), Json(json!({"confirmed":false}))).await;
        assert!(cancelled.err().unwrap().0.to_string().contains("启用"));
        let disabled = prepare_api(State(state.clone()), Json(json!({"confirmed":true}))).await;
        assert!(disabled.err().unwrap().0.to_string().contains("启用"));
        assert!(!root(&state.config_path).exists());
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[tokio::test]
    async fn embedded_network_is_ready_without_an_installer_or_confirmation() {
        let (_directory, state) = fixture();
        ensure_loaded(&state).await.unwrap();
        state.remote_network.inner.lock().await.config.enabled = true;
        let value = prepare_api(State(state.clone()), Json(json!({"confirmed":false})))
            .await
            .unwrap()
            .0;
        assert_eq!(value["runtimeEmbedded"], true);
        assert_eq!(value["runtimeReady"], true);
        assert_eq!(value["runtimeInstalled"], false);
        assert_eq!(value["networkReady"], false);
        assert!(!root(&state.config_path).exists());
        assert!(state.remote_network.inner.lock().await.running.is_none());
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
    async fn crossed_requests_display_real_delivery_and_agree_on_both_tokens() {
        let (_a, a) = fixture();
        let (_b, b) = fixture();
        let mut requests = Vec::new();
        for state in [&a, &b] {
            ensure_loaded(state).await.unwrap();
            let mut inner = state.remote_network.inner.lock().await;
            inner.config.enabled = true;
            ensure_identity(&mut inner).unwrap();
            requests.push(PairRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                invitation_token: random_token(),
                device: inner.config.identity(),
                return_token: random_token(),
                poll_token: random_token(),
            });
        }
        let expires = now_ms() + 60_000;
        for (state, mine, other) in [
            (&a, &requests[0], &requests[1]),
            (&b, &requests[1], &requests[0]),
        ] {
            {
                let mut inner = state.remote_network.inner.lock().await;
                inner.outgoing = Some(OutgoingPair {
                    target: other.device.clone(),
                    request: mine.clone(),
                    expires_at: expires,
                    acknowledged: false,
                });
            }
            let sending = status(state).await.unwrap();
            assert_eq!(sending["outgoingPairing"]["delivered"], false);
            assert_eq!(sending["devices"][1]["connectionType"], "sending");
            assert_eq!(sending["pendingPairings"], json!([]));
            {
                let mut inner = state.remote_network.inner.lock().await;
                inner.outgoing.as_mut().unwrap().acknowledged = true;
                assert_eq!(device_rows(&inner)[1]["connectionType"], "pairing");
                inner.pairings.insert(
                    other.request_id.clone(),
                    PendingPair {
                        request: other.clone(),
                        expires_at: expires,
                        decision: PairDecision::Pending,
                    },
                );
                assert!(inner.secrets.peers.is_empty());
            }
            let incoming = status(state).await.unwrap();
            assert_eq!(incoming["pendingPairings"][0]["deviceId"], other.device.id);
            assert_eq!(incoming["devices"][1]["connectionType"], "incoming");
            assert_eq!(incoming["devices"].as_array().unwrap().len(), 2);
        }
        decide_pair(&a, &requests[1].request_id, true)
            .await
            .unwrap();
        decide_pair(&b, &requests[0].request_id, true)
            .await
            .unwrap();
        let aa = a.remote_network.inner.lock().await;
        let bb = b.remote_network.inner.lock().await;
        assert!(aa.outgoing.is_none() && bb.outgoing.is_none());
        let a_grant = &aa.secrets.peers[&requests[1].device.id];
        let b_grant = &bb.secrets.peers[&requests[0].device.id];
        assert_eq!(a_grant.inbound, b_grant.outbound);
        assert_eq!(a_grant.outbound, b_grant.inbound);
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
        // Revocation is a local trust change. No live peer or overlay is needed.
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
        let removed: NetworkConfig =
            serde_json::from_slice(&std::fs::read(config_file(&state.config_path)).unwrap())
                .unwrap();
        assert!(removed.paired.is_empty());
        let mut fresh = Inner {
            config: removed,
            ..Default::default()
        };
        fresh.discovered.push(runtime::DiscoveredPeer {
            ipv4: "10.203.0.17".into(),
            hostname: "still on the overlay".into(),
            direct: true,
        });
        assert_eq!(
            device_rows(&fresh).len(),
            1,
            "deleted peer must not reappear as an unpaired copy"
        );
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
