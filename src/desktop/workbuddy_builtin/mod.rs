//! Explicitly authorize WorkBuddy sharing, then relay independently from Hub.
use crate::{
    ai_gateway::config::{ProviderConfig, ProviderType},
    config::AppConfig,
};
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Notify, oneshot};

pub(crate) mod direct;
mod preferences;
mod prepare;
const LEASE_TTL: Duration = Duration::from_secs(4);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
pub const BASE_URL: &str = "http://127.0.0.1/workbuddy-builtin";
const SOURCE_PREFIX: &str = "WorkBuddy 内置 · ";

pub fn is_source(provider: &ProviderConfig) -> bool {
    provider.workbuddy_builtin.is_some()
        && provider.desktop_binding.is_none()
        && !provider.is_client_reserved()
        && provider.name.starts_with(SOURCE_PREFIX)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinBinding {
    pub account_id: String,
    pub model_id: String,
    #[serde(default)]
    pub capabilities: Value,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinModel {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_tools: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_images: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_thinking: Option<bool>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub billing: Value,
}
impl BuiltinModel {
    fn capabilities(&self) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(json!({}));
        if let Some(object) = value.as_object_mut() {
            object.remove("id");
            object.remove("name");
            if let Some(output) = self.max_output_tokens {
                object.insert("maxTokens".into(), output.into());
            }
        }
        value
    }
}

// Local ledger references intentionally have no Serialize or Debug implementation.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalReference {
    pub url: String,
    pub token: String,
    pub context: String,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    bridge_id: String,
    logged_in: bool,
    account_id: Option<String>,
    account_name: Option<String>,
    #[serde(default)]
    models: Vec<BuiltinModel>,
    reference: Option<LocalReference>,
    #[serde(default)]
    state: String,
    #[serde(default)]
    stage: bool,
    #[serde(default)]
    auth_only: bool,
    authorization_epoch: Option<u64>,
    authorization: Option<direct::Authorization>,
}
struct Snapshot {
    report: Report,
    at: Instant,
}
struct Lease {
    token: String,
    bridge_id: String,
    snapshot: StdMutex<Option<Snapshot>>,
    changed: Notify,
    startup: StdMutex<String>,
    direct: Option<Arc<direct::Runtime>>,
    authorization_epoch: Mutex<u64>,
}
impl Lease {
    fn startup_state(&self) -> String {
        self.startup.lock().map(|s| s.clone()).unwrap_or_default()
    }
    fn startup_failed(&self) -> bool {
        matches!(
            self.startup_state().as_str(),
            "unsupported-bootstrap" | "unsupported-host" | "host-attach-failed"
        )
    }
    fn live(&self) -> Option<ReportView> {
        let snapshot = self.snapshot.lock().ok()?;
        let snapshot = snapshot.as_ref().filter(|s| s.at.elapsed() < LEASE_TTL)?;
        Some(ReportView {
            logged_in: snapshot.report.logged_in,
            account_id: snapshot.report.account_id.clone(),
            account_name: snapshot.report.account_name.clone(),
            models: snapshot.report.models.clone(),
            reference: snapshot.report.reference.clone(),
            state: snapshot.report.state.clone(),
        })
    }
}
struct ReportView {
    logged_in: bool,
    account_id: Option<String>,
    account_name: Option<String>,
    models: Vec<BuiltinModel>,
    reference: Option<LocalReference>,
    state: String,
}
impl ReportView {
    fn from_metadata(value: Value) -> Option<Self> {
        let account_id = value["accountId"].as_str()?.to_owned();
        if account_id.len() != 64 || !account_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        Some(Self {
            logged_in: true,
            account_id: Some(account_id),
            account_name: value["accountName"].as_str().map(str::to_owned),
            models: serde_json::from_value(value["models"].clone()).ok()?,
            reference: None,
            state: "independent-ready".into(),
        })
    }
}

fn public_metadata(report: &Report) -> Value {
    json!({
        "accountId": report.account_id,
        "accountName": report.account_name,
        "models": report.models,
    })
}
struct Session {
    lease: Arc<Lease>,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.task.abort();
    }
}
#[derive(Default)]
struct Inner {
    session: Option<Session>,
    direct: Option<Arc<direct::Runtime>>,
    needs_restart: bool,
    detail: Option<String>,
}
#[derive(Default)]
pub struct Service {
    gate: Mutex<()>,
    inner: Mutex<Inner>,
}

async fn detach_native(inner: &mut Inner) {
    if let Some(session) = inner.session.take() {
        // Finish any current capture, then make this callback lease incapable
        // of restoring authorization after an explicit stop or reauthorization.
        *session.lease.authorization_epoch.lock().await = u64::MAX;
        drop(session);
    }
}

async fn receive(
    State(lease): State<Arc<Lease>>,
    headers: HeaderMap,
    Json(mut report): Json<Report>,
) -> StatusCode {
    if headers.contains_key("origin")
        || headers.get("authorization").and_then(|h| h.to_str().ok())
            != Some(format!("Bearer {}", lease.token).as_str())
        || report.bridge_id != lease.bridge_id
        || report.models.len() > 512
    {
        return StatusCode::FORBIDDEN;
    }
    if report.stage {
        let rank = |stage: &str| match stage {
            "main-loader-ready" => 1,
            "main-bootstrap-ready" => 2,
            "daemon-preload-ready" => 3,
            "server-module-ready" => 4,
            "unsupported-bootstrap" | "unsupported-host" | "host-attach-failed" => 10,
            _ => 0,
        };
        let Ok(mut stage) = lease.startup.lock() else {
            return StatusCode::SERVICE_UNAVAILABLE;
        };
        if rank(&report.state) == 0 {
            return StatusCode::BAD_REQUEST;
        }
        // Startup callbacks may finish out of order; keep host/account reports.
        if rank(&report.state) > rank(&stage) {
            *stage = report.state;
        }
        drop(stage);
        lease.changed.notify_one();
        return StatusCode::NO_CONTENT;
    }
    if report
        .account_id
        .as_ref()
        .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        || report
            .account_name
            .as_ref()
            .is_some_and(|s| s.len() > 512 || s.chars().any(char::is_control))
        || report
            .models
            .iter()
            .any(|m| !valid_id(&m.id) || m.name.len() > 512 || m.name.chars().any(char::is_control))
        || report
            .reference
            .as_ref()
            .is_some_and(|r| valid_reference(r).is_err())
    {
        return StatusCode::BAD_REQUEST;
    }
    let mut epoch = lease.authorization_epoch.lock().await;
    if report.auth_only {
        let Some(incoming) = report.authorization_epoch.filter(|n| *n > 0) else {
            return StatusCode::BAD_REQUEST;
        };
        if incoming <= *epoch {
            return StatusCode::NO_CONTENT;
        }
        let Some(runtime) = &lease.direct else {
            return StatusCode::BAD_REQUEST;
        };
        let granting = report.authorization.is_some();
        let result = if let Some(authorization) = report.authorization.take() {
            if !report.logged_in || report.account_id.is_none() {
                return StatusCode::BAD_REQUEST;
            }
            let mut metadata = public_metadata(&report);
            // A token rotation can coincide with a remote catalog refresh.
            // Keep this same account's last confirmed directory until it loads.
            if report.models.is_empty()
                && let Some(previous) = runtime.snapshot().await
                && previous["accountId"] == metadata["accountId"]
            {
                metadata["models"] = previous["models"].clone();
            }
            runtime.capture(authorization, metadata).await
        } else {
            runtime.clear().await
        };
        if result.is_err() {
            return StatusCode::BAD_REQUEST;
        }
        *epoch = incoming;
        if !granting {
            // Auth-only revocations must invalidate old ledger references too.
            if let Ok(mut snapshot) = lease.snapshot.lock() {
                *snapshot = None;
            }
        }
        lease.changed.notify_one();
        return StatusCode::NO_CONTENT;
    }
    if report.authorization.is_some() {
        return StatusCode::BAD_REQUEST;
    }
    if report.authorization_epoch.unwrap_or(0) != *epoch {
        // A late heartbeat must not restore the previous account or catalog.
        return StatusCode::NO_CONTENT;
    }
    if report.logged_in
        && report.state == "ready"
        && !report.models.is_empty()
        && let Some(runtime) = &lease.direct
        && runtime
            .update_metadata(public_metadata(&report))
            .await
            .is_err()
    {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    if !report.logged_in || report.account_id.is_none() || report.models.is_empty() {
        report.reference = None;
    }
    if !report.logged_in {
        report.models.clear();
        report.account_id = None;
    }
    let Ok(mut snapshot) = lease.snapshot.lock() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    *snapshot = Some(Snapshot {
        report,
        at: Instant::now(),
    });
    drop(snapshot);
    lease.changed.notify_one();
    StatusCode::NO_CONTENT
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}

fn startup_failure(stage: &str) -> String {
    let detail = match stage {
        "main-loader-ready" => "主程序适配器已载入，但未命中 WorkBuddy 子进程启动模块",
        "main-bootstrap-ready" => "主程序接线已完成，但 WorkBuddy 子进程尚未载入适配器",
        "daemon-preload-ready" => "子进程适配器已载入，但模型服务模块尚未加载",
        "server-module-ready" => "模型服务模块已接线，但宿主未完成回调握手",
        "unsupported-bootstrap" => "WorkBuddy 主程序的子进程启动位置已变化",
        "unsupported-host" => "WorkBuddy 模型服务接线位置已变化",
        "host-attach-failed" => "WorkBuddy 模型服务初始化未完成",
        _ => "主程序适配器未完成回调握手",
    };
    format!(
        "WorkBuddy 分享未就绪：{detail}；已结束本次等待，请正常退出 WorkBuddy 后重试。客户端原启动保留"
    )
}
fn valid_reference(reference: &LocalReference) -> Result<()> {
    let url = url::Url::parse(&reference.url)?;
    ensure!(
        url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/skills/vlm/chat"
            && url.query().is_none()
            && url.fragment().is_none(),
        "WorkBuddy 本机引用无效"
    );
    ensure!(
        !reference.token.is_empty()
            && reference.token.len() <= 512
            && reference.token.is_ascii()
            && !reference.token.chars().any(char::is_whitespace)
            && !reference.context.is_empty()
            && reference.context.len() <= 8192
            && reference.context.is_ascii()
            && !reference.context.chars().any(char::is_control),
        "WorkBuddy 会话引用无效"
    );
    Ok(())
}

impl Service {
    pub(crate) fn feature_visible(&self, path: &Path) -> Result<bool> {
        preferences::visible(path)
    }

    pub(crate) async fn reveal(&self, path: &Path) -> Result<()> {
        let _guard = self.gate.lock().await;
        preferences::write(path, true)
    }

    pub(crate) async fn hide(&self, path: &Path) -> Result<()> {
        let _guard = self.gate.lock().await;
        // Hiding the entry deliberately leaves sharing, credentials and models.
        preferences::write(path, false)
    }

    pub async fn enable(&self, path: &Path) -> Result<()> {
        let _guard = self.gate.lock().await;
        let mut inner = self.inner.lock().await;
        let reauthorize = if let Some(runtime) = &inner.direct {
            runtime.last_error().await.is_some()
                || matches!(
                    runtime.public_account_status().await["authorization"]["status"].as_str(),
                    Some("expired" | "rejected")
                )
        } else {
            false
        } || inner
            .session
            .as_ref()
            .and_then(|s| s.lease.live())
            .is_some_and(|r| r.state == "authorization-unavailable");
        if reauthorize {
            detach_native(&mut inner).await;
            if let Some(runtime) = inner.direct.take() {
                runtime.clear().await?;
            }
        }
        if inner
            .session
            .as_ref()
            .is_some_and(|s| s.lease.live().is_some())
        {
            return Ok(());
        }
        if let Some(runtime) = &inner.direct
            && let Some(metadata) = runtime.snapshot().await
            && let Some(account) = metadata["accountId"].as_str()
            && runtime.ready_for(account).await
            && runtime.last_error().await.is_none()
        {
            return Ok(());
        }
        // An expired attachment cannot be silently recovered from a running app.
        detach_native(&mut inner).await;
        inner.direct = None;
        inner.needs_restart = false;
        inner.detail = None;
        let runtime = Arc::new(direct::Runtime::new(path)?);
        // A stale or unreadable Hub-owned grant must allow this explicit enable
        // to obtain fresh authorization rather than fail on every retry.
        let restored = match runtime.load().await {
            Ok(metadata) => metadata,
            Err(_) => {
                runtime.clear().await?;
                None
            }
        };
        if let Some(metadata) = restored
            && metadata["models"]
                .as_array()
                .is_some_and(|models| !models.is_empty())
        {
            inner.direct = Some(runtime);
            return Ok(());
        }
        let path = path.to_owned();
        let prepared = tokio::task::spawn_blocking(move || prepare::prepare(&path)).await??;
        let prepare::Outcome::Ready(prepared) = prepared else {
            inner.needs_restart = true;
            inner.detail = Some("请先退出 WorkBuddy，再点击启用分享；现有任务不会自动关闭".into());
            return Ok(());
        };
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let lease = Arc::new(Lease {
            token: uuid::Uuid::new_v4().to_string(),
            bridge_id: uuid::Uuid::new_v4().to_string(),
            snapshot: StdMutex::new(None),
            changed: Notify::new(),
            startup: StdMutex::new(String::new()),
            direct: Some(runtime.clone()),
            authorization_epoch: Mutex::new(0),
        });
        let router = Router::new()
            .route("/", post(receive))
            .layer(axum::extract::DefaultBodyLimit::max(512 * 1024))
            .with_state(lease.clone());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await;
        });
        let session = Session {
            lease: lease.clone(),
            stop: Some(stop),
            task,
        };
        let callback = format!("http://{address}/");
        let token = lease.token.clone();
        let bridge_id = lease.bridge_id.clone();
        if let Err(error) = prepared.launch(&callback, &token, &bridge_id).await {
            inner.detail = Some(error.to_string());
            inner.needs_restart = true;
            return Ok(());
        }
        let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
        inner.session = Some(session);
        inner.direct = Some(runtime.clone());
        inner.detail = None;
        drop(inner);
        loop {
            let notified = lease.changed.notified();
            if runtime.snapshot().await.is_some() || lease.live().is_some() {
                return Ok(());
            }
            if lease.startup_failed() || tokio::time::timeout_at(deadline, notified).await.is_err()
            {
                let mut inner = self.inner.lock().await;
                inner.detail = Some(startup_failure(&lease.startup_state()));
                inner.needs_restart = true;
                inner.session = None;
                inner.direct = None;
                return Ok(());
            }
        }
    }

    pub async fn disable(&self) -> Result<()> {
        let _guard = self.gate.lock().await;
        let mut inner = self.inner.lock().await;
        detach_native(&mut inner).await;
        if let Some(runtime) = &inner.direct {
            runtime.clear().await?;
        }
        inner.direct = None;
        inner.needs_restart = false;
        inner.detail = Some(
            "已停用分享并移除 Hub 保存的授权；已保存的客户端模型保留，下次启用需重新授权。".into(),
        );
        Ok(())
    }

    pub(crate) async fn refresh_account(&self) -> Result<()> {
        let runtime = self
            .inner
            .lock()
            .await
            .direct
            .clone()
            .context("请先启用 WorkBuddy 分享并完成授权")?;
        runtime.refresh_account().await?;
        Ok(())
    }

    pub async fn view(&self, path: &Path, config: &AppConfig) -> Result<Value> {
        let inner = self.inner.lock().await;
        let native = inner.session.as_ref().and_then(|s| s.lease.live());
        let host_connected = native.is_some();
        let enabled = inner.session.is_some() || inner.direct.is_some();
        let needs_restart = inner.needs_restart;
        let saved_detail = inner.detail.clone();
        let runtime = inner.direct.clone();
        drop(inner);
        let direct_report = if let Some(runtime) = &runtime {
            runtime.snapshot().await.and_then(ReportView::from_metadata)
        } else {
            None
        };
        let authorization_error = if let Some(runtime) = &runtime {
            runtime.last_error().await
        } else {
            None
        };
        let account = if let Some(runtime) = &runtime {
            runtime.public_account_status().await
        } else {
            json!({"authorization":{"status":"missing"},"credits":{"status":"unknown","unit":"credits"}})
        };
        let authorization_expired = matches!(
            account["authorization"]["status"].as_str(),
            Some("expired" | "rejected")
        );
        let independent = direct_report.is_some() && authorization_error.is_none();
        let authorization_unavailable = native
            .as_ref()
            .is_some_and(|r| r.state == "authorization-unavailable");
        let live = direct_report.or(native);
        let ready = live.as_ref().is_some_and(|r| {
            r.logged_in
                && !r.models.is_empty()
                && authorization_error.is_none()
                && !authorization_expired
                && (independent || r.reference.is_some() && !authorization_unavailable)
        });
        let detail = if authorization_expired && authorization_error.is_none() {
            "WorkBuddy 授权已到期，请点击重新授权。".into()
        } else if let Some(error) = &authorization_error {
            format!("{error}；请点击重新授权。")
        } else if independent && ready {
            if host_connected {
                "独立分享已就绪，可以彻底退出 WorkBuddy；模型调用和授权续期由 Hub 处理，按 WorkBuddy 账户积分扣费。".into()
            } else {
                "独立分享已就绪，无需启动 WorkBuddy；使用最近同步的模型目录，按 WorkBuddy 账户积分扣费。".into()
            }
        } else {
            saved_detail.unwrap_or_else(|| {
                    match live.as_ref().map(|r| r.state.as_str()) {
                Some("authorization-unavailable") => {
                    "未取得 WorkBuddy 独立分享授权，请点击重新授权；当前还不能退出 WorkBuddy。".into()
                }
                Some("independent-ready") => {
                    "已保存独立授权，等待 WorkBuddy 完成本次模型目录同步后重新读取。".into()
                }
                Some("unsupported-host") => {
                    "WorkBuddy 宿主接线位置不兼容，未启用分享；请使用普通启动".into()
                }
                Some("ready") if ready => {
                    "已连接当前 WorkBuddy 账户的可选模型目录；实际可用权限及费用以该账户/组织为准"
                        .into()
                }
                Some("login-required") => {
                    "请在 WorkBuddy 内完成登录后重新读取；独立授权将保存在本机系统保护存储中。".into()
                }
                Some("catalog-pending") => {
                    "WorkBuddy 尚未确认当前账户的远端模型目录，请等待加载完成后重新读取".into()
                }
                Some(_) => "WorkBuddy 尚未提供可用的本机分享引用，请重新读取状态".into(),
                None if enabled => {
                    "WorkBuddy 宿主连接已中断，请正常退出 WorkBuddy 后重新启用分享"
                        .into()
                }
                None => {
                    "首次启用会打开 WorkBuddy 获取授权；独立分享就绪后可彻底退出，之后启用无需再启动客户端。".into()
                }
            }
                })
        };
        let rows = live
            .as_ref()
            .map(|r| {
                r.models
                    .iter()
                    .map(|m| {
                        let mut value = serde_json::to_value(m).unwrap_or(json!({}));
                        value["shared"] = config
                            .ai_gateway
                            .providers
                            .iter()
                            .any(|p| {
                                p.enabled
                                    && is_source(p)
                                    && p.workbuddy_builtin.as_ref().is_some_and(|b| {
                                        Some(&b.account_id) == r.account_id.as_ref()
                                            && b.model_id == m.id
                                    })
                            })
                            .into();
                        value
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let library = super::resources::Library::new(path)?;
        let targets = library
            .targets(&library.load()?)?
            .into_iter()
            .filter(|t| t.client != super::clients::ClientId::WorkBuddy && !t.models_inherited)
            .collect::<Vec<_>>();
        Ok(
            json!({"revision":config.revision,"enabled":enabled,"ready":ready,
            "loggedIn":live.as_ref().is_some_and(|r|r.logged_in),"accountName":live.as_ref().and_then(|r|r.account_name.as_ref()),
            "needsRestart":needs_restart,"detail":detail,"models":rows,"targets":targets,
            "featureVisible":self.feature_visible(path)?,"account":account,
            "independent":independent,"hostConnected":host_connected,"catalogCached":independent && !host_connected,
            "reauthorizationRequired":authorization_error.is_some() || authorization_unavailable || authorization_expired}),
        )
    }

    pub(crate) async fn direct_for(
        &self,
        binding: &BuiltinBinding,
    ) -> Result<Option<Arc<direct::Runtime>>> {
        let runtime = self.inner.lock().await.direct.clone();
        let Some(runtime) = runtime else {
            return Ok(None);
        };
        let Some(report) = runtime.snapshot().await.and_then(ReportView::from_metadata) else {
            return Ok(None);
        };
        ensure!(
            report.account_id.as_deref() == Some(&binding.account_id),
            "WorkBuddy 账户已切换或退出，旧分享已撤销；请重新选择当前账户模型"
        );
        ensure!(
            report.models.iter().any(|m| m.id == binding.model_id),
            "WorkBuddy 当前账户目录不再提供此模型，请重新读取并分享"
        );
        Ok(Some(runtime))
    }

    pub async fn reference(&self, binding: &BuiltinBinding) -> Result<LocalReference> {
        let inner = self.inner.lock().await;
        let report = inner
            .session
            .as_ref()
            .and_then(|s| s.lease.live())
            .context("WorkBuddy 分享未启用或宿主已退出，请在内置模型分享页重新启用")?;
        ensure!(
            report.logged_in && report.account_id.as_deref() == Some(&binding.account_id),
            "WorkBuddy 账户已切换或退出，旧分享已撤销；请重新选择当前账户模型"
        );
        ensure!(
            report.models.iter().any(|m| m.id == binding.model_id),
            "WorkBuddy 当前账户目录不再提供此模型，请重新读取并分享"
        );
        report
            .reference
            .context("WorkBuddy 本机分享引用尚未就绪，请重新读取状态")
    }

    pub async fn source(&self, config: &AppConfig, model_id: &str) -> Result<ProviderConfig> {
        let inner = self.inner.lock().await;
        let native = inner.session.as_ref().and_then(|s| s.lease.live());
        let runtime = inner.direct.clone();
        drop(inner);
        let independent = if let Some(runtime) = &runtime {
            ensure!(
                runtime.last_error().await.is_none(),
                "WorkBuddy 授权已失效，请重新授权"
            );
            runtime.snapshot().await.and_then(ReportView::from_metadata)
        } else {
            None
        };
        let direct_ready = independent.is_some();
        let report = independent
            .or(native)
            .context("请先启用 WorkBuddy 内置模型分享")?;
        ensure!(
            report.logged_in
                && (direct_ready || report.reference.is_some())
                && report.state != "authorization-unavailable",
            "WorkBuddy 当前账户分享尚未就绪"
        );
        let model = report
            .models
            .iter()
            .find(|m| m.id == model_id)
            .context("当前账户模型目录已变化，请重新选择")?;
        let account_id = report.account_id.context("WorkBuddy 当前账户未就绪")?;
        if let Some(source) = config.ai_gateway.providers.iter().find(|p| {
            is_source(p)
                && p.workbuddy_builtin
                    .as_ref()
                    .is_some_and(|b| b.account_id == account_id && b.model_id == model_id)
        }) {
            let mut source = source.clone();
            source.enabled = true;
            if let Some(binding) = &mut source.workbuddy_builtin {
                binding.capabilities = model.capabilities();
            }
            return Ok(source);
        }
        let short = &account_id[..8];
        Ok(ProviderConfig {
            name: format!("WorkBuddy 内置 · {} · {short} · {}", model.name, model.id),
            provider_type: ProviderType::ChatCompletions,
            compatibility: Some("openai_chat".into()),
            base_url: BASE_URL.into(),
            models: vec![model.id.clone()],
            model_aliases: [(model.id.clone(), model.id.clone())].into(),
            workbuddy_builtin: Some(BuiltinBinding {
                account_id,
                model_id: model.id.clone(),
                capabilities: model.capabilities(),
            }),
            timeout_secs: 600,
            ..Default::default()
        })
    }
}

pub fn validate_provider(provider: &ProviderConfig) -> Result<()> {
    let Some(binding) = &provider.workbuddy_builtin else {
        return Ok(());
    };
    ensure!(
        provider.provider_type == ProviderType::ChatCompletions
            && provider.base_url == BASE_URL
            && provider.api_key.is_empty()
            && provider.chatgpt_auth_id.is_none()
            && provider.tiangong_builtin.is_none()
            && valid_id(&binding.model_id)
            && binding.account_id.len() == 64
            && binding.account_id.bytes().all(|b| b.is_ascii_hexdigit())
            && provider
                .models
                .iter()
                .chain(provider.model_aliases.values())
                .all(|m| m == &binding.model_id),
        "WorkBuddy 内置模型绑定已变化，请从当前账户目录重新分享；不要手工填写登录凭据"
    );
    Ok(())
}

/// Whole-config editors may change ordinary client settings, but cannot create
/// a host binding or silently convert an existing one to a plain API channel.
pub fn validate_edit(previous: &AppConfig, next: &AppConfig) -> Result<()> {
    for provider in &next.ai_gateway.providers {
        let old = previous
            .ai_gateway
            .providers
            .iter()
            .find(|p| p.name == provider.name);
        let old_binding = old.and_then(|p| p.workbuddy_builtin.as_ref());
        if old_binding.is_some() || provider.workbuddy_builtin.is_some() {
            ensure!(
                old_binding.is_some() && old_binding == provider.workbuddy_builtin.as_ref(),
                "WorkBuddy 内置模型由当前账户目录管理，请从分享页重新选择；原绑定未修改"
            );
            validate_provider(provider)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
