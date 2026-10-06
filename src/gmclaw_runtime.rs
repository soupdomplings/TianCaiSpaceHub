//! IM-triggered desktop startup and bounded local connection checks.
//!
//! Once enabled, connection checks automatically obtain the temporary
//! authorization of a verified, same-user desktop Harness. It remains in
//! memory and never enters Hub config. Only explicit `/tg` or a desktop launch
//! button may start a desktop.
//! Desktop startup uses the saved Hub token only in the child environment.
use std::{
    future::Future,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[cfg(not(windows))]
use std::process::{Command, Stdio};

use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, header::HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, watch};

use crate::{
    app_state::SharedState,
    config::AppConfig,
    gmclaw_executor::validate_endpoint,
    gmclaw_im::{GmClawBridgeConfig, sender_allowed},
    types::InboundMessage,
};

mod credentials;
mod display;
#[cfg(windows)]
mod windows_start;

const MAX_STATUS_BYTES: usize = 32 * 1024;
pub(crate) const STATUS_TIMEOUT: Duration = Duration::from_secs(18);
const CONNECTION_WAIT: Duration = Duration::from_secs(30);
const MONITOR_INTERVAL: Duration = Duration::from_secs(5);
const RECOVERY_INTERVAL: Duration = Duration::from_secs(5);
const OVERVIEW_MAX_AGE: Duration = Duration::from_secs(15);
static LAUNCH_LOCK: Mutex<()> = Mutex::const_new(());
static ACTIVE_AUTHORIZATION: Mutex<Option<ActiveAuthorization>> = Mutex::const_new(None);
static LAST_LAUNCH: Mutex<Option<Instant>> = Mutex::const_new(None);
static STATUS_PROBE: Mutex<Option<StatusProbe>> = Mutex::const_new(None);
static AUTHORIZATION_RECOVERY: Mutex<Option<AuthorizationRecovery>> = Mutex::const_new(None);
static DISPLAY_REFRESHES: Mutex<std::collections::VecDeque<PendingDisplayRefresh>> =
    Mutex::const_new(std::collections::VecDeque::new());
// Ownership survives a popped, completed or superseded display attempt. Only
// row IDs are kept; this is deliberately not a message or credential cache.
static DISPLAY_OWNERSHIP: Mutex<std::collections::VecDeque<PendingDisplayRefresh>> =
    Mutex::const_new(std::collections::VecDeque::new());
static DISPLAY_DIAGNOSTIC: Mutex<Option<DisplayDiagnostic>> = Mutex::const_new(None);
const DISPLAY_DIAGNOSTIC_MAX_AGE: Duration = Duration::from_secs(30);

struct PendingDisplayRefresh {
    task_id: String,
    session_id: String,
    hub_row_ids: Vec<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DisplayState {
    Pending,
    Prepared,
    Updated,
    Closed,
    Waiting,
    Limited,
    Failed,
}

struct DisplayDiagnostic {
    state: DisplayState,
    detail: &'static str,
    recorded_at: Instant,
}

async fn record_display(state: DisplayState, detail: &'static str) {
    *DISPLAY_DIAGNOSTIC.lock().await = Some(DisplayDiagnostic {
        state,
        detail,
        recorded_at: Instant::now(),
    });
}

fn merge_display_rows(current: &mut Vec<i64>, additional: &[i64]) {
    for id in additional
        .iter()
        .copied()
        .filter(|id| *id > 0 && *id <= 9_007_199_254_740_991)
    {
        if !current.contains(&id) {
            current.push(id);
        }
    }
    // Native row IDs increase across inserts. A stale in-flight snapshot must
    // not evict newer reply rows when merged after the cache reaches capacity.
    current.sort_unstable();
    if current.len() > 128 {
        current.drain(..current.len() - 128);
    }
}

async fn remember_display_rows(task_id: &str, session_id: &str, rows: &[i64]) -> Vec<i64> {
    let mut cache = DISPLAY_OWNERSHIP.lock().await;
    let mut remembered = cache
        .iter()
        .find(|entry| entry.task_id == task_id && entry.session_id == session_id)
        .map(|entry| entry.hub_row_ids.clone())
        .unwrap_or_default();
    merge_display_rows(&mut remembered, rows);
    cache.retain(|entry| entry.task_id != task_id || entry.session_id != session_id);
    if cache.len() >= 128 {
        cache.pop_front();
    }
    cache.push_back(PendingDisplayRefresh {
        task_id: task_id.to_owned(),
        session_id: session_id.to_owned(),
        hub_row_ids: remembered.clone(),
    });
    remembered
}

/// Never retain an arbitrary OS/CDP error string. All diagnostic values come
/// from this fixed list, without paths, task identities or response contents.
fn display_failure_detail(error: &anyhow::Error) -> &'static str {
    if let Some(detail) = display::view_issue_detail(error) {
        return detail;
    }
    let detail = error.to_string();
    if detail.contains("桌面页面尚未出现") {
        "同步通道已连接，但桌面页面尚未出现；后台继续检查"
    } else if detail.contains("桌面页面地址与安装路径不匹配") {
        "桌面页面地址与安装路径不匹配，页面未修改"
    } else if detail.contains("桌面页面无法唯一核验") {
        "检测到多个匹配的桌面页面，未修改任何页面"
    } else if detail.contains("桌面页面标识无法核验") {
        "桌面页面标识未通过核对，页面未修改"
    } else if detail.contains("版本") || detail.contains("资源") {
        "天工安装资源或版本未通过核对，页面未修改"
    } else if detail.contains("尚未启用") {
        "同步通道未启用；首次请正常退出天工后从 Hub 启动一次"
    } else if detail.contains("端口") || detail.contains("进程") || detail.contains("用户") {
        "同步监听或当前天工实例身份未通过核对，页面未修改"
    } else if detail.contains("超时") {
        "展示同步超时；消息已保存，后台继续检查，不重放任务"
    } else if detail.contains("作用域")
        || detail.contains("字段")
        || detail.contains("结构")
        || detail.contains("视图")
    {
        "原生视图或闭包字段未通过核对，页面未修改"
    } else if detail.contains("窗口") || detail.contains("尚未准备") {
        "天工会话窗口尚未就绪，页面未修改"
    } else if detail.contains("连接") || detail.contains("响应") || detail.contains("协议") {
        "本机展示通信尚未完成，消息已保存，不重放任务"
    } else {
        "展示同步检查未完成，消息已保存，不重放任务"
    }
}

/// Native records remain authoritative. Queue identities and Hub-owned reply
/// row IDs so that a stale native write is never mistaken for an IM update.
pub(crate) async fn queue_desktop_refresh(
    task_id: &str,
    session_id: &str,
    hub_row_id: Option<i64>,
) {
    let row_ids: Vec<i64> = hub_row_id.into_iter().collect();
    let owned = remember_display_rows(task_id, session_id, &row_ids).await;
    let mut pending = DISPLAY_REFRESHES.lock().await;
    let mut hub_row_ids = pending
        .iter()
        .find(|entry| entry.task_id == task_id && entry.session_id == session_id)
        .map(|entry| entry.hub_row_ids.clone())
        .unwrap_or_default();
    merge_display_rows(&mut hub_row_ids, &owned);
    pending.retain(|entry| entry.task_id != task_id);
    if pending.len() >= 128 {
        pending.pop_front();
    }
    pending.push_back(PendingDisplayRefresh {
        task_id: task_id.to_owned(),
        session_id: session_id.to_owned(),
        hub_row_ids,
    });
    drop(pending);
    let mut diagnostic = DISPLAY_DIAGNOSTIC.lock().await;
    if diagnostic.as_ref().is_none_or(|value| {
        value.recorded_at.elapsed() > DISPLAY_DIAGNOSTIC_MAX_AGE
            || matches!(
                value.state,
                DisplayState::Updated | DisplayState::Closed | DisplayState::Prepared
            )
    }) {
        *diagnostic = Some(DisplayDiagnostic {
            state: DisplayState::Pending,
            detail: "消息已保存，等待后台刷新桌面",
            recorded_at: Instant::now(),
        });
    }
}

async fn refresh_pending_display() {
    let Some(mut entry) = DISPLAY_REFRESHES.lock().await.pop_front() else {
        return;
    };
    // The pending queue is scheduling only. Popping cannot discard ownership.
    let owned = remember_display_rows(&entry.task_id, &entry.session_id, &entry.hub_row_ids).await;
    merge_display_rows(&mut entry.hub_row_ids, &owned);
    let operation = async {
        let installation = tokio::task::spawn_blocking(discover)
            .await
            .map_err(|_| anyhow::anyhow!("天工桌面运行检查未完成"))?
            .map_err(|_| anyhow::anyhow!("天工桌面运行身份无法核验"))?;
        let executable = installation.executable.context("天工桌面尚未准备")?;
        let rows =
            remember_display_rows(&entry.task_id, &entry.session_id, &entry.hub_row_ids).await;
        display::refresh(
            &executable,
            &installation.process_ids,
            &entry.task_id,
            &entry.session_id,
            &rows,
        )
        .await
    };
    let finished = match tokio::time::timeout(Duration::from_secs(13), operation).await {
        Ok(Ok(display::DisplayRefresh::Updated)) => {
            record_display(DisplayState::Updated, "原生对话消息和缓存已更新").await;
            true
        }
        Ok(Ok(display::DisplayRefresh::Closed)) => {
            record_display(
                DisplayState::Closed,
                "任务未在桌面打开；已刷新列表，原生打开后读取已保存消息",
            )
            .await;
            true
        }
        Ok(Ok(display::DisplayRefresh::Deferred(reason))) => {
            record_display(DisplayState::Waiting, reason).await;
            false
        }
        Ok(Ok(display::DisplayRefresh::Limited(reason))) => {
            record_display(DisplayState::Limited, reason).await;
            true
        }
        Ok(Err(error)) => {
            record_display(DisplayState::Failed, display_failure_detail(&error)).await;
            false
        }
        Err(_) => {
            record_display(
                DisplayState::Failed,
                "展示同步超过等待时间；消息已保存，不重放任务",
            )
            .await;
            false
        }
    };
    if !finished {
        let owned =
            remember_display_rows(&entry.task_id, &entry.session_id, &entry.hub_row_ids).await;
        merge_display_rows(&mut entry.hub_row_ids, &owned);
        let mut pending = DISPLAY_REFRESHES.lock().await;
        // Keep a newer scheduled entry, but merge all same-session ownership.
        // Dropping these old IDs can leave a cached reply placeholder stuck.
        if let Some(newer) = pending
            .iter_mut()
            .find(|item| item.task_id == entry.task_id)
        {
            if newer.session_id == entry.session_id {
                merge_display_rows(&mut newer.hub_row_ids, &entry.hub_row_ids);
            }
        } else if pending.len() < 128 {
            pending.push_back(entry);
        }
    }
}

async fn append_display_status(mut status: GmClawRuntimeStatus) -> GmClawRuntimeStatus {
    let ready = tokio::time::timeout(Duration::from_secs(5), async {
        let installation = tokio::task::spawn_blocking(discover)
            .await
            .map_err(|_| anyhow::anyhow!("天工桌面运行检查未完成"))?
            .map_err(|_| anyhow::anyhow!("天工桌面运行身份无法核验"))?;
        let executable = installation.executable.context("天工桌面尚未就绪")?;
        display::available(&executable, &installation.process_ids).await
    })
    .await;
    match ready {
        Ok(Ok(())) => {
            record_display(
                DisplayState::Prepared,
                "通道与原生视图能力已准备，尚未确认窗口消息更新",
            )
            .await;
            status
                .detail
                .push_str("；桌面对话同步：通道与视图能力已准备，尚未确认窗口消息更新");
        }
        Ok(Err(error)) => {
            let detail = display_failure_detail(&error);
            record_display(DisplayState::Failed, detail).await;
            status
                .detail
                .push_str("；消息可继续处理并保存。桌面对话同步：");
            status.detail.push_str(detail);
        }
        Err(_) => {
            let detail = "同步能力检查超时，消息仍可处理保存；首次请从 Hub 启动天工启用通道";
            record_display(DisplayState::Failed, detail).await;
            status.detail.push_str("；桌面对话同步：");
            status.detail.push_str(detail);
        }
    }
    status
}

struct StatusProbe {
    configuration_key: String,
    started_at: Instant,
    result: watch::Receiver<Option<GmClawRuntimeStatus>>,
}

struct AuthorizationRecovery {
    configuration_key: String,
    attempted_at: Instant,
}

struct ActiveAuthorization {
    configuration_key: String,
    header: HeaderValue,
}

fn authorization_key(config: &GmClawBridgeConfig) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        serde_json::to_vec(&(config.enabled, &config.endpoint, &config.auth_token))
            .expect("connection authorization identity"),
    ))
}

/// A validated running-instance authorization, or the saved Hub startup token.
/// This only reads the in-memory cache; status checks perform runtime discovery.
pub(crate) async fn connection_authorization(config: &GmClawBridgeConfig) -> Result<HeaderValue> {
    let configured = authorization(&config.auth_token)?;
    let active = ACTIVE_AUTHORIZATION.lock().await;
    Ok(active
        .as_ref()
        .filter(|active| active.configuration_key == authorization_key(config))
        .map(|active| active.header.clone())
        .unwrap_or(configured))
}

pub(crate) fn runtime_authorization_identity(header: &HeaderValue) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(header.as_bytes()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GmClawConnectionState {
    NotConfigured,
    NotRunning,
    Unverified,
    Connected,
    AuthFailed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmClawRuntimeStatus {
    pub state: GmClawConnectionState,
    pub detail: String,
    pub installed: bool,
    pub running: bool,
}

#[derive(Default)]
struct Installation {
    executable: Option<PathBuf>,
    running: bool,
    process_ids: Vec<u32>,
}

impl Installation {
    fn status(&self, state: GmClawConnectionState, detail: &str) -> GmClawRuntimeStatus {
        GmClawRuntimeStatus {
            state,
            detail: detail.to_owned(),
            installed: self.executable.is_some(),
            running: self.running,
        }
    }
}

/// Two OS-random UUIDs provide 244 random bits without another dependency.
/// Existing user-provided tokens remain valid; never rotate on refresh/startup.
pub fn ensure_auth_token(config: &mut GmClawBridgeConfig) {
    if config.auth_token.trim().is_empty() {
        config.auth_token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
    }
}

/// Called only after an explicit IM selection has passed the caller's session
/// and busy checks. Ordinary checks may reconnect an enabled bridge, but never
/// prepare configuration or launch a desktop. Preserve the existing token.
pub async fn prepare_for_im(
    state: &SharedState,
    message: &InboundMessage,
) -> Result<(GmClawBridgeConfig, GmClawRuntimeStatus)> {
    let mut current = state.config.lock().await;
    let path = state.config_path.clone();
    let config = tokio::task::spawn_blocking(move || -> Result<AppConfig> {
        let mut config = AppConfig::load_or_default(&path)
            .map_err(|_| anyhow::anyhow!("无法读取天工连接设置，请稍后重试 /tg"))?;
        crate::normalize_config_paths(&mut config, &path);
        Ok(config)
    })
    .await
    .context("无法准备天工自动连接，请稍后重试 /tg")??;
    // Publish every successful disk read, even if access was revoked or the
    // settings are invalid. Other IM messages must not reuse stale permissions.
    *current = config.clone();
    ensure!(
        sender_allowed(&config, message),
        "当前账号或发送者已无接入权限"
    );
    let mut config = config;
    let needs_save =
        !config.gmclaw_bridge.enabled || config.gmclaw_bridge.auth_token.trim().is_empty();
    config.gmclaw_bridge.enabled = true;
    ensure_auth_token(&mut config.gmclaw_bridge);
    config.gmclaw_bridge.validate()?;
    if needs_save {
        let path = state.config_path.clone();
        let save_result = tokio::task::spawn_blocking(move || -> Result<AppConfig> {
            config.save(&path).map_err(|_| {
                anyhow::anyhow!("天工自动连接设置保存失败或配置已变化，请稍后重试 /tg")
            })?;
            Ok(config)
        })
        .await
        .context("无法保存天工自动连接，请稍后重试 /tg")?;
        config = match save_result {
            Ok(saved) => saved,
            Err(error) => {
                // A failed optimistic save can mean another process updated
                // permissions after our first read. Keep its new disk state.
                if let Ok(mut latest) = AppConfig::load_or_default(&state.config_path) {
                    crate::normalize_config_paths(&mut latest, &state.config_path);
                    *current = latest;
                }
                return Err(error);
            }
        };
        *current = config.clone();
    }
    drop(current);

    let expected_revision = config.revision.clone();
    let executable = config.gmclaw_bridge.desktop_path.as_ref().map(Into::into);
    let validate_before_spawn = || async {
        let mut guard = state.config.lock().await;
        let mut latest = AppConfig::load_or_default(&state.config_path)
            .map_err(|_| anyhow::anyhow!("无法核对天工连接设置，本次未启动；请重试 /tg"))?;
        crate::normalize_config_paths(&mut latest, &state.config_path);
        *guard = latest.clone();
        ensure!(
            latest.revision == expected_revision && latest.gmclaw_bridge.enabled,
            "连接检查期间配置已变化，本次未启动天工；请重试 /tg"
        );
        ensure!(
            sender_allowed(&latest, message),
            "当前账号或发送者已无接入权限"
        );
        Ok::<_, anyhow::Error>(guard)
    };
    let runtime = launch(&config.gmclaw_bridge, executable, validate_before_spawn).await?;

    // `launch` may return an already-running instance without spawning. Recheck
    // disk permissions/configuration in that path too before handing a session
    // the connection identity. Do not replace changes made during probing.
    let mut current = state.config.lock().await;
    let mut latest = AppConfig::load_or_default(&state.config_path)
        .map_err(|_| anyhow::anyhow!("无法核对天工连接设置，请重试 /tg"))?;
    crate::normalize_config_paths(&mut latest, &state.config_path);
    *current = latest.clone();
    ensure!(
        sender_allowed(&latest, message),
        "当前账号或发送者已无接入权限"
    );
    ensure!(
        serde_json::to_value(&latest.gmclaw_bridge)?
            == serde_json::to_value(&config.gmclaw_bridge)?,
        "连接检查期间天工设置已变化，请重试 /tg"
    );
    let bridge = latest.gmclaw_bridge.clone();
    Ok((bridge, runtime))
}

fn authorization(token: &str) -> Result<HeaderValue> {
    ensure!(
        !token.trim().is_empty()
            && token.len() <= 8192
            && token.is_ascii()
            && !token.chars().any(char::is_control),
        "天工连接授权尚未准备好，请在消息入口重新发送 /tg"
    );
    let mut header = HeaderValue::from_str(&format!("Bearer {}", token.trim()))
        .context("天工连接授权格式无效")?;
    header.set_sensitive(true);
    Ok(header)
}

async fn status_json(response: reqwest::Response) -> Result<Value> {
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= MAX_STATUS_BYTES as u64),
        "天工状态响应过大"
    );
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("无法读取天工状态")?;
        ensure!(
            bytes.len() + chunk.len() <= MAX_STATUS_BYTES,
            "天工状态响应过大"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).context("天工状态响应格式无效")
}

/// Checks only the health/authenticated status endpoints observed in 1.1.1.
/// Never sends a model request, creates a session, or exposes component messages.
pub async fn status(config: &GmClawBridgeConfig) -> GmClawRuntimeStatus {
    bounded_status(config, config.enabled).await
}

/// Keep an already-enabled bridge connected across desktop restarts. No task,
/// model request, configuration write, or desktop startup occurs in this loop.
pub(crate) async fn run_connection_monitor(
    state: SharedState,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(run_display_monitor(state.clone(), shutdown.clone()));
    let mut interval = tokio::time::interval(MONITOR_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if *shutdown.borrow() {
            return;
        }
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
            _ = interval.tick() => {
                let config = state.config.lock().await.gmclaw_bridge.clone();
                if config.enabled {
                    tokio::select! {
                        _ = status(&config) => {},
                        _ = shutdown.changed() => return,
                    }
                }
            }
        }
    }
}

/// Display retries never delay authorization recovery or the IM reply. No
/// messages are replayed, and only tasks written by this Hub run are queued.
async fn run_display_monitor(state: SharedState, mut shutdown: watch::Receiver<bool>) {
    let mut interval = tokio::time::interval(MONITOR_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if *shutdown.borrow() {
            return;
        }
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
            _ = interval.tick() => {
                let config = state.config.lock().await.gmclaw_bridge.clone();
                if config.enabled && !DISPLAY_REFRESHES.lock().await.is_empty() {
                    tokio::select! {
                        _ = refresh_pending_display() => {},
                        _ = shutdown.changed() => return,
                    }
                }
            }
        }
    }
}

/// Dashboard timeout must not abort a shared credential discovery. Prefer the
/// latest same-configuration observation while the monitor completes a refresh.
pub(crate) async fn overview_status(config: &GmClawBridgeConfig) -> GmClawRuntimeStatus {
    let key = status_key(config, config.enabled);
    let cached = {
        let probe = STATUS_PROBE.lock().await;
        probe
            .as_ref()
            .filter(|probe| {
                probe.configuration_key == key && probe.started_at.elapsed() <= OVERVIEW_MAX_AGE
            })
            .and_then(|probe| probe.result.borrow().clone())
    };
    let runtime = match cached {
        Some(runtime) => runtime,
        None => tokio::time::timeout(Duration::from_millis(1700), status(config))
            .await
            .unwrap_or_else(|_| {
                Installation::default().status(
                    GmClawConnectionState::Unverified,
                    "天工本地连接正在自动检查，请稍后刷新",
                )
            }),
    };
    append_display_overview(runtime, config.enabled).await
}

async fn append_display_overview(
    mut runtime: GmClawRuntimeStatus,
    enabled: bool,
) -> GmClawRuntimeStatus {
    if !enabled {
        return runtime;
    }
    let pending = !DISPLAY_REFRESHES.lock().await.is_empty();
    runtime.detail.push_str("；桌面对话同步：");
    let diagnostic = DISPLAY_DIAGNOSTIC.lock().await;
    if let Some(latest) = diagnostic
        .as_ref()
        .filter(|value| value.recorded_at.elapsed() <= DISPLAY_DIAGNOSTIC_MAX_AGE)
    {
        let label = match latest.state {
            DisplayState::Pending => "待更新",
            DisplayState::Prepared => "能力已准备",
            DisplayState::Updated => "上次已更新",
            DisplayState::Closed => "任务未打开",
            DisplayState::Waiting => "等待刷新",
            DisplayState::Limited => "暂不支持本次刷新",
            DisplayState::Failed => "上次未完成",
        };
        runtime.detail.push_str(label);
        runtime.detail.push_str("，");
        runtime.detail.push_str(latest.detail);
        runtime.detail.push_str(&format!(
            "（{} 秒前）",
            latest.recorded_at.elapsed().as_secs()
        ));
        if pending && !matches!(latest.state, DisplayState::Pending | DisplayState::Waiting) {
            runtime.detail.push_str("；仍有已保存消息待更新");
        }
    } else if pending {
        runtime
            .detail
            .push_str("有已保存消息待更新，尚未取得最近的刷新结果");
    } else {
        runtime
            .detail
            .push_str("尚无最近的窗口更新核验，授权已连接不代表消息已刷新");
    }
    runtime
}

/// Process identity only; no HTTP requests, runtime authorization or app launch.
pub(crate) async fn desktop_process_ids() -> Vec<u32> {
    tokio::time::timeout(
        Duration::from_secs(2),
        tokio::task::spawn_blocking(discover),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .and_then(Result::ok)
    .map(|mut installation| {
        installation.process_ids.sort_unstable();
        installation.process_ids.dedup();
        installation.process_ids
    })
    .unwrap_or_default()
}

async fn bounded_status(
    config: &GmClawBridgeConfig,
    recover_authorization: bool,
) -> GmClawRuntimeStatus {
    tokio::time::timeout(STATUS_TIMEOUT, shared_status(config, recover_authorization))
        .await
        .unwrap_or_else(|_| {
            Installation::default().status(
                GmClawConnectionState::Unverified,
                "天工连接检查超过等待时间，暂未确认就绪；已启用的接入会自动继续检查",
            )
        })
}

fn status_key(config: &GmClawBridgeConfig, recover_authorization: bool) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        serde_json::to_vec(&(
            config.enabled,
            &config.endpoint,
            &config.auth_token,
            &config.desktop_path,
            recover_authorization,
        ))
        .expect("connection probe identity"),
    ))
}

async fn shared_status(
    config: &GmClawBridgeConfig,
    recover_authorization: bool,
) -> GmClawRuntimeStatus {
    let key = status_key(config, recover_authorization);
    let mut receiver = {
        let mut probe = STATUS_PROBE.lock().await;
        let reusable = probe.as_ref().is_some_and(|probe| {
            probe.configuration_key == key
                && probe.result.borrow().is_none()
                && probe.started_at.elapsed() <= STATUS_TIMEOUT
        });
        if !reusable {
            let config = config.clone();
            let (result_tx, result_rx) = watch::channel(None);
            *probe = Some(StatusProbe {
                configuration_key: key,
                started_at: Instant::now(),
                result: result_rx,
            });
            // Owned worker outlives short dashboard requests, so their 1.8s
            // deadline cannot repeatedly cancel the bounded OS credential read.
            tokio::spawn(async move {
                let result = tokio::time::timeout(
                    STATUS_TIMEOUT,
                    inspect_status(&config, recover_authorization),
                )
                .await
                .unwrap_or_else(|_| {
                    Installation::default().status(
                        GmClawConnectionState::Unverified,
                        "天工连接检查超时，已启用的接入会自动重试",
                    )
                });
                let _ = result_tx.send(Some(result));
            });
        }
        probe
            .as_ref()
            .expect("connection probe prepared")
            .result
            .clone()
    };
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }
        if receiver.changed().await.is_err() {
            return Installation::default().status(
                GmClawConnectionState::Unverified,
                "天工本地连接检查尚未完成，将自动重新检查",
            );
        }
    }
}

async fn inspect_status(
    config: &GmClawBridgeConfig,
    recover_authorization: bool,
) -> GmClawRuntimeStatus {
    let desktop_path = config.desktop_path.clone();
    let installation = match tokio::task::spawn_blocking(move || -> Result<Installation> {
        let mut installation = discover()?;
        if let Some(path) = desktop_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            installation.executable =
                Some(resolve_executable(Path::new(path)).context("天工桌面路径无效")?);
        }
        Ok(installation)
    })
    .await
    {
        Ok(Ok(value)) => value,
        _ => {
            return Installation::default().status(
                GmClawConnectionState::Error,
                "无法确认天工安装或进程状态，请稍后刷新；未启动新进程",
            );
        }
    };
    probe(config, installation, recover_authorization).await
}

async fn probe(
    config: &GmClawBridgeConfig,
    mut installation: Installation,
    recover_authorization: bool,
) -> GmClawRuntimeStatus {
    use GmClawConnectionState::*;
    if config.auth_token.trim().is_empty() {
        return installation.status(
            NotConfigured,
            "在飞书、微信或企业微信发送 /tg，Hub 会自动连接天工",
        );
    }
    let (mut url, configured_authorization) = match (
        validate_endpoint(&config.endpoint),
        authorization(&config.auth_token),
    ) {
        (Ok(url), Ok(header)) => (url, header),
        _ => return installation.status(Error, "天工本机连接设置无效，请检查已有连接设置"),
    };
    let http = match Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .read_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(client) => client,
        Err(_) => return installation.status(Error, "无法创建天工本机连接"),
    };
    url.set_path("/health");
    let response = match http.get(url.clone()).send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() => {
            return if installation.running {
                installation.status(
                    Unverified,
                    "天工正在启动或退出，本地连接尚未建立；正在等待服务就绪",
                )
            } else {
                installation.status(
                    NotRunning,
                    if config.enabled {
                        "天工尚未启动，打开天工后 Hub 会自动连接"
                    } else {
                        "天工尚未启动，发送 /tg 后 Hub 会尝试自动启动并连接"
                    },
                )
            };
        }
        Err(error) if error.is_timeout() => {
            return installation.status(
                Unverified,
                "天工本地服务响应超时，暂未确认就绪；已启用的接入会自动重新检查",
            );
        }
        Err(_) => {
            return installation.status(
                Error,
                "天工本地健康接口请求失败，请检查天工；已启用的接入会自动重新检查",
            );
        }
    };
    installation.running = true;
    if !response.status().is_success()
        || !status_json(response)
            .await
            .is_ok_and(|body| body["status"] == "healthy")
    {
        return installation.status(Error, "该地址的服务不符合天工健康接口，请检查连接地址");
    }

    // A public health response alone cannot prove authorization. Also refuse a
    // sidecar that accepts unauthenticated status requests: verify_auth permits
    // this in 1.1.1 when it was started without an auth token.
    url.set_path("/ctrl/status");
    match http.get(url.clone()).send().await {
        Ok(response)
            if matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) => {}
        Ok(_) => {
            return installation.status(Unverified, "天工状态接口未确认授权保护，暂不标记为已连接");
        }
        Err(_) => {
            return installation.status(Unverified, "天工已启动，授权状态暂时无法确认，请稍后刷新");
        }
    }
    let active_authorization = connection_authorization(config)
        .await
        .expect("configured authorization validated");
    let mut candidates = vec![active_authorization];
    if candidates[0] != configured_authorization {
        candidates.push(configured_authorization);
    }
    let mut accepted = None;
    for header in candidates {
        match authenticated_status(&http, &url, header.clone()).await {
            Ok(response) if is_auth_failure(response.status()) => {}
            Ok(response) => {
                accepted = Some((response, header));
                break;
            }
            Err(_) => {
                return installation.status(
                    Unverified,
                    "天工已启动，授权状态暂时无法确认；已启用的接入会自动重新检查",
                );
            }
        }
    }
    if accepted.is_none() && recover_authorization {
        // Hold only the discovery gate, never an application config lock.
        // Failures are throttled per configuration and concurrent probes cannot
        // spawn duplicate OS credential readers.
        let recovery_key = status_key(config, true);
        let mut recovery = AUTHORIZATION_RECOVERY.lock().await;
        if recovery.as_ref().is_some_and(|attempt| {
            attempt.configuration_key == recovery_key
                && attempt.attempted_at.elapsed() < RECOVERY_INTERVAL
        }) {
            return installation.status(Unverified, "天工运行授权正在自动重新识别，请稍后等待连接");
        }
        *recovery = Some(AuthorizationRecovery {
            configuration_key: recovery_key,
            attempted_at: Instant::now(),
        });
        match credentials::running_authorization(&config.endpoint).await {
            Ok(Some(header)) => match authenticated_status(&http, &url, header.clone()).await {
                Ok(response) if !is_auth_failure(response.status()) => {
                    accepted = Some((response, header))
                }
                _ => {
                    return installation.status(
                        AuthFailed,
                        "天工运行授权尚未通过核验，请确认天工服务就绪；Hub 会自动重新连接",
                    );
                }
            },
            Ok(None) => {
                return installation.status(
                    AuthFailed,
                    "暂未找到可接入的天工运行实例，请确认 Hub 和天工由同一系统用户打开；Hub 会自动重新检查",
                );
            }
            Err(_) => {
                return installation.status(
                    AuthFailed,
                    "暂未完成天工运行授权识别，请确认 Hub 和天工由同一系统用户打开；Hub 会自动重新检查",
                );
            }
        }
    }
    let Some((response, header)) = accepted else {
        return installation.status(
            AuthFailed,
            "天工运行授权尚未连接，请在消息入口发送 /tg 启用接入",
        );
    };
    if !response.status().is_success() {
        return installation.status(Unverified, "天工授权状态接口暂不可用，请稍后刷新");
    }
    match status_json(response).await {
        Ok(body) if body["status"] == "running" => {
            // Cache only after the authenticated response proves the protocol.
            // Runtime credentials never rotate the saved configuration identity.
            *ACTIVE_AUTHORIZATION.lock().await = Some(ActiveAuthorization {
                configuration_key: authorization_key(config),
                header,
            });
            let ready = body["components"].as_object().is_some_and(|components| {
                !components.is_empty()
                    && components
                        .values()
                        .all(|component| component["ready"] == true)
            });
            if ready {
                installation.status(Connected, "天工已连接，授权与组件状态检查通过")
            } else {
                installation.status(
                    Unverified,
                    "天工连接授权已通过，部分组件尚未就绪，请稍后刷新或检查天工桌面",
                )
            }
        }
        _ => installation.status(Unverified, "天工状态响应未通过核对，请检查版本和连接地址"),
    }
}

fn is_auth_failure(status: StatusCode) -> bool {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
}

async fn authenticated_status(
    http: &Client,
    url: &url::Url,
    header: HeaderValue,
) -> Result<reqwest::Response> {
    Ok(http
        .get(url.clone())
        .header(reqwest::header::AUTHORIZATION, header)
        .send()
        .await?)
}

/// Explicit startup only. Save the exact token atomically before this call.
/// Existing processes are never killed, reconfigured, or silently replaced.
pub async fn launch<F, Fut, Guard>(
    config: &GmClawBridgeConfig,
    executable: Option<PathBuf>,
    validate_before_spawn: F,
) -> Result<GmClawRuntimeStatus>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Guard>>,
{
    let _guard = LAUNCH_LOCK.lock().await;
    authorization(&config.auth_token)?;
    let url = validate_endpoint(&config.endpoint)?;
    let deadline = Instant::now() + CONNECTION_WAIT;
    let mut runtime = bounded_status(config, true).await;
    let mut validate_before_spawn = Some(validate_before_spawn);
    let mut spawned = false;
    loop {
        use GmClawConnectionState::*;
        match runtime.state {
            Connected => {
                *LAST_LAUNCH.lock().await = None;
                return Ok(append_display_status(runtime).await);
            }
            AuthFailed | Error | NotConfigured => return Ok(runtime),
            _ => {}
        }
        if runtime.state == NotRunning && !runtime.running && !spawned {
            let recently_started = LAST_LAUNCH
                .lock()
                .await
                .is_some_and(|started| started.elapsed() < Duration::from_secs(60));
            if !recently_started {
                let started =
                    start_desktop(config, &url, executable.clone(), &mut validate_before_spawn)
                        .await?;
                if let Some(status) = started {
                    runtime = status;
                    if runtime.state == Error {
                        return Ok(runtime);
                    }
                } else {
                    spawned = true;
                    *LAST_LAUNCH.lock().await = Some(Instant::now());
                }
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            if spawned && runtime.state == NotRunning {
                runtime.state = Error;
                runtime.detail = "天工启动后未保持运行，请检查天工安装和本地端口后重试 /tg".into();
            } else if !runtime.running && runtime.state == NotRunning {
                runtime.state = Unverified;
                runtime.detail =
                    "上一次天工启动尚未确认完成，本次未重复启动；请稍后重试 /tg".into();
            }
            return Ok(runtime);
        }
        tokio::time::sleep(Duration::from_millis(350).min(remaining)).await;
        let remaining = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(remaining, bounded_status(config, true)).await {
            Ok(status) => runtime = status,
            Err(_) => {
                runtime.state = Unverified;
                runtime.detail = "天工连接仍在初始化，本次暂未确认就绪；可稍后重新发送 /tg".into();
                return Ok(runtime);
            }
        }
    }
}

/// Returns None only after spawning; Some means re-use or report the process
/// discovered just before startup. Do not hold a config lock while waiting.
async fn start_desktop<F, Fut, Guard>(
    config: &GmClawBridgeConfig,
    url: &url::Url,
    executable: Option<PathBuf>,
    validate_before_spawn: &mut Option<F>,
) -> Result<Option<GmClawRuntimeStatus>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Guard>>,
{
    ensure!(
        url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port_or_known_default() == Some(7861),
        "天工桌面使用固定本地端口，自动启动请使用 http://127.0.0.1:7861"
    );
    let installation = tokio::task::spawn_blocking(discover)
        .await
        .context("无法检查天工进程")??;
    if installation.running {
        return Ok(Some(installation.status(
            GmClawConnectionState::Unverified,
            "天工已启动，正在等待连接就绪",
        )));
    }
    // A missing desktop and a connect timeout do not prove its port is free.
    // In particular, never replace an exiting sidecar or a different listener.
    let port_check = tokio::net::TcpListener::bind("127.0.0.1:7861").await;
    match port_check {
        Ok(listener) => drop(listener),
        Err(_) => {
            return Ok(Some(installation.status(
                GmClawConnectionState::Unverified,
                "天工本地端口仍被占用或无法使用，本次未重复启动；请等待退出完成后重试 /tg",
            )));
        }
    }
    let executable = executable
        .or(installation.executable)
        .context("未找到天工 Claw 安装，请先在 Hub 所在电脑安装天工 Claw 后重试 /tg")?;
    let executable = resolve_executable(&executable)
        .context("天工桌面安装不完整或已有程序路径无效，请检查安装后重试 /tg")?;
    // Explicit startup enables the local display adapter once. An existing
    // normally-started desktop is never closed or relaunched to add this flag.
    let display_listener =
        tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, display::DISPLAY_PORT))
            .await
            .context("天工桌面同步端口暂不可用，本次未启动；请稍后重试")?;
    drop(display_listener);
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new(&executable);
        command
            .arg("--remote-debugging-address=127.0.0.1")
            .arg(format!("--remote-debugging-port={}", display::DISPLAY_PORT))
            .current_dir(executable.parent().context("天工安装目录无效")?)
            .env("GMCLAW_AUTH_TOKEN", config.auth_token.trim())
            .env_remove("ELECTRON_RUN_AS_NODE")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };
    // The caller rechecks the saved revision after discovery/probing and can
    // keep its configuration guard alive through the actual process creation.
    let configuration_guard = validate_before_spawn
        .take()
        .context("本次连接已发起启动，请等待天工就绪后重试 /tg")?()
    .await?;
    #[cfg(windows)]
    {
        // Stable Command::spawn inherits all inheritable Windows handles even
        // with Stdio::null(). The desktop must never retain Hub's sockets.
        windows_start::start(&executable, config.auth_token.trim(), display::DISPLAY_PORT)
            .context("无法启动天工桌面，请检查程序安装及访问权限")?;
    }
    #[cfg(not(windows))]
    let mut child = command
        .spawn()
        .context("无法启动天工桌面，请检查程序安装及访问权限")?;
    drop(configuration_guard);
    // Reap on Unix without tying desktop lifetime to a Hub task/request.
    #[cfg(not(windows))]
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(None)
}

fn resolve_executable(path: &Path) -> Option<PathBuf> {
    if valid_executable(path) {
        return Some(path.to_path_buf());
    }
    #[cfg(target_os = "macos")]
    {
        if !path.is_absolute()
            || !path.is_dir()
            || !path.extension().is_some_and(|extension| extension == "app")
        {
            return None;
        }
        let output = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", "Print :CFBundleExecutable"])
            .arg(path.join("Contents/Info.plist"))
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let name = String::from_utf8_lossy(&output.stdout);
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\']) {
            return None;
        }
        let executable = path.join("Contents/MacOS").join(name);
        if valid_executable(&executable) {
            return Some(executable);
        }
    }
    None
}

#[cfg(windows)]
fn valid_executable(path: &Path) -> bool {
    path.is_absolute()
        && path.is_file()
        && path.file_name().is_some_and(|name| {
            name.to_string_lossy()
                .eq_ignore_ascii_case("tiangong-desktop.exe")
        })
        && path.parent().is_some_and(|parent| {
            parent.join("resources/app.asar").is_file()
                && parent
                    .join("resources/harness-sidecar/packaging/harness_sidecar.py")
                    .is_file()
        })
}

#[cfg(target_os = "macos")]
fn valid_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_absolute()
        && path
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        && path.parent().is_some_and(|parent| {
            parent.file_name().is_some_and(|name| name == "MacOS")
                && parent.parent().is_some_and(|contents| {
                    contents.join("Resources/app.asar").is_file()
                        && contents
                            .join("Resources/harness-sidecar/packaging/harness_sidecar.py")
                            .is_file()
                })
        })
}

#[cfg(not(any(windows, target_os = "macos")))]
fn valid_executable(_: &Path) -> bool {
    false
}

#[cfg(windows)]
fn discover() -> Result<Installation> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_NO_MORE_FILES, GetLastError, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
    };
    let mut result = Installation::default();
    let mut desktop_parents = std::collections::HashMap::new();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    ensure!(snapshot != INVALID_HANDLE_VALUE, "无法检查天工进程");
    let snapshot_result = (|| -> Result<()> {
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut available = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        loop {
            if !available {
                ensure!(
                    unsafe { GetLastError() } == ERROR_NO_MORE_FILES,
                    "无法读取天工进程列表"
                );
                break;
            }
            let end = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..end])
                .eq_ignore_ascii_case("tiangong-desktop.exe")
            {
                // Even an unreadable matching process blocks duplicate startup.
                result.running = true;
                let process = unsafe {
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID)
                };
                if !process.is_null() {
                    let mut image = vec![0_u16; 32_768];
                    let mut length = image.len() as u32;
                    let readable = unsafe {
                        QueryFullProcessImageNameW(process, 0, image.as_mut_ptr(), &mut length)
                    } != 0;
                    unsafe { CloseHandle(process) };
                    if readable {
                        let path =
                            PathBuf::from(String::from_utf16_lossy(&image[..length as usize]));
                        if valid_executable(&path) {
                            result.process_ids.push(entry.th32ProcessID);
                            desktop_parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
                            result.executable = Some(path);
                        }
                    }
                }
            }
            available = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        Ok(())
    })();
    unsafe { CloseHandle(snapshot) };
    snapshot_result?;
    // Renderer/helper processes share the desktop executable on Windows; only
    // main instances identify a run, so renderer restarts do not reset evidence.
    let desktop_ids = result
        .process_ids
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    result.process_ids.retain(|id| {
        desktop_parents
            .get(id)
            .is_none_or(|parent| !desktop_ids.contains(parent))
    });
    if result.executable.is_none() {
        result.executable = windows_install_candidates()
            .into_iter()
            .find(|path| valid_executable(path));
    }
    Ok(result)
}

#[cfg(windows)]
fn windows_install_candidates() -> Vec<PathBuf> {
    use winreg::{
        RegKey,
        enums::{
            HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
        },
    };
    let mut candidates = Vec::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let root = RegKey::predef(hive);
            if let Ok(key) = root.open_subkey_with_flags(
                "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\tiangong-desktop.exe",
                KEY_READ | view,
            ) {
                if let Ok(value) = key.get_value::<String, _>("") {
                    if let Some(path) = windows_registered_file(&value, false) {
                        candidates.push(path);
                    }
                }
            }
            let Ok(uninstall) = root.open_subkey_with_flags(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                KEY_READ | view,
            ) else {
                continue;
            };
            for name in uninstall.enum_keys().filter_map(Result::ok) {
                let Ok(key) = uninstall.open_subkey_with_flags(name, KEY_READ | view) else {
                    continue;
                };
                let display: String = key.get_value("DisplayName").unwrap_or_default();
                let display = display.to_ascii_lowercase();
                let install_location = key.get_value::<String, _>("InstallLocation").ok();
                let display_icon = key.get_value::<String, _>("DisplayIcon").ok();
                let uninstall_command = key.get_value::<String, _>("UninstallString").ok();
                if !(display.contains("tiangong")
                    || display.contains("gmclaw")
                    || display.contains("天工")
                    || install_location
                        .iter()
                        .chain(display_icon.iter())
                        .chain(uninstall_command.iter())
                        .any(|value| value.to_ascii_lowercase().contains("tiangong-desktop")))
                {
                    continue;
                }
                if let Some(location) = install_location {
                    let path = PathBuf::from(location.trim().trim_matches('"'));
                    if path.is_absolute() {
                        candidates.push(path.join("tiangong-desktop.exe"));
                    }
                }
                if let Some(icon) = display_icon {
                    if let Some(path) = windows_registered_file(&icon, true) {
                        // The official NSIS installer can register an .ico
                        // file without InstallLocation, including on another
                        // drive. Its containing directory identifies the app.
                        if let Some(parent) = path.parent() {
                            candidates.push(parent.join("tiangong-desktop.exe"));
                        }
                        candidates.push(path);
                    }
                }
                if let Some(command) = uninstall_command {
                    if let Some(path) = windows_registered_file(&command, false) {
                        if let Some(parent) = path.parent() {
                            candidates.push(parent.join("tiangong-desktop.exe"));
                        }
                    }
                }
            }
        }
    }
    for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(base) = std::env::var_os(key) {
            candidates.push(PathBuf::from(base).join("tiangong-desktop/tiangong-desktop.exe"));
        }
    }
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(base).join("Programs/tiangong-desktop/tiangong-desktop.exe"));
    }
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|path| seen.insert(path.to_string_lossy().to_ascii_lowercase()));
    candidates
}

/// Read installation metadata as a path only; never execute a registry command.
#[cfg(windows)]
fn windows_registered_file(value: &str, is_icon: bool) -> Option<PathBuf> {
    let value = value.trim();
    let path = if let Some(quoted) = value.strip_prefix('"') {
        quoted.split_once('"')?.0
    } else if is_icon {
        value.rsplit_once(',').map_or(value, |(path, index)| {
            if index.trim().parse::<i32>().is_ok() {
                path
            } else {
                value
            }
        })
    } else {
        // App Paths normally has no arguments; UninstallString often does.
        // Retain spaces in the directory while excluding any command flags.
        let lower = value.to_ascii_lowercase();
        let end = lower.match_indices(".exe").find_map(|(index, _)| {
            let end = index + 4;
            (end == value.len()
                || value[end..].starts_with(char::is_whitespace)
                || value[end..].starts_with(','))
            .then_some(end)
        })?;
        &value[..end]
    };
    let path = PathBuf::from(path.trim());
    path.is_absolute().then_some(path)
}

#[cfg(target_os = "macos")]
fn discover() -> Result<Installation> {
    let mut result = Installation::default();
    // comm contains the executable path only; never request command arguments.
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,comm="])
        .output()
        .context("无法检查天工进程")?;
    ensure!(output.status.success(), "无法读取天工进程列表");
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((pid, path)) = line.trim().split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        let path = PathBuf::from(path.trim());
        let lower = line.to_ascii_lowercase();
        if (lower.contains("tiangong") || lower.contains("gmclaw") || lower.contains("天工"))
            && lower.contains(".app/contents/macos/")
        {
            result.running = true;
            if valid_executable(&path) {
                result.process_ids.push(pid);
                result.executable = Some(path);
            }
        }
    }
    if result.executable.is_some() {
        return Ok(result);
    }
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home_dir) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home_dir).join("Applications"));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if !name.ends_with(".app")
                || !(name.contains("tiangong") || name.contains("gmclaw") || name.contains("天工"))
            {
                continue;
            }
            if let Some(executable) = resolve_executable(&entry.path()) {
                result.executable = Some(executable);
                return Ok(result);
            }
        }
    }
    Ok(result)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn discover() -> Result<Installation> {
    anyhow::bail!("天工自动连接仅支持 Windows 和 macOS")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_authorization_is_per_installation_and_stable() {
        let mut first = GmClawBridgeConfig::default();
        let mut second = GmClawBridgeConfig::default();
        ensure_auth_token(&mut first);
        ensure_auth_token(&mut second);
        assert_eq!(first.auth_token.len(), 64);
        assert!(
            first
                .auth_token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
        assert_ne!(first.auth_token, second.auth_token);
        let saved = first.auth_token.clone();
        ensure_auth_token(&mut first);
        assert_eq!(first.auth_token, saved);
        assert!(!format!("{first:?}").contains(&saved));
    }

    #[test]
    fn existing_manual_authorization_survives_automatic_setup() {
        let mut config = GmClawBridgeConfig {
            auth_token: "existing-local-fixture".into(),
            ..Default::default()
        };
        ensure_auth_token(&mut config);
        assert_eq!(config.auth_token, "existing-local-fixture");
        assert!(authorization(&config.auth_token).unwrap().is_sensitive());
        assert!(authorization("invalid\r\nheader").is_err());
    }
}
