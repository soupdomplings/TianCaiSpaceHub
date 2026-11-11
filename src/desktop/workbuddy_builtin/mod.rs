//! Share the currently logged-in WorkBuddy host without exporting its login.
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

mod prepare;
const LEASE_TTL: Duration = Duration::from_secs(4);
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
}
impl Lease {
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
    needs_restart: bool,
    detail: Option<String>,
}
#[derive(Default)]
pub struct Service {
    gate: Mutex<()>,
    inner: Mutex<Inner>,
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
    lease.changed.notify_waiters();
    StatusCode::NO_CONTENT
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
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
    pub async fn enable(&self, path: &Path) -> Result<()> {
        let _guard = self.gate.lock().await;
        let mut inner = self.inner.lock().await;
        if inner
            .session
            .as_ref()
            .is_some_and(|s| s.lease.live().is_some())
        {
            return Ok(());
        }
        // An expired attachment cannot be silently recovered from a running app.
        inner.session = None;
        inner.needs_restart = false;
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
        tokio::task::spawn_blocking(move || prepared.launch(&callback, &token, &bridge_id))
            .await??;
        inner.session = Some(session);
        inner.detail = None;
        drop(inner);
        let notified = lease.changed.notified();
        if lease.live().is_none() {
            let _ = tokio::time::timeout(Duration::from_secs(12), notified).await;
        }
        Ok(())
    }

    pub async fn disable(&self) {
        let _guard = self.gate.lock().await;
        let mut inner = self.inner.lock().await;
        inner.session = None;
        inner.needs_restart = false;
        inner.detail = Some("已停用分享。已保存的客户端模型保留，WorkBuddy 会在租约结束后释放分享会话；再次启用需先退出 WorkBuddy".into());
    }

    pub async fn view(&self, path: &Path, config: &AppConfig) -> Result<Value> {
        let inner = self.inner.lock().await;
        let live = inner.session.as_ref().and_then(|s| s.lease.live());
        let ready = live
            .as_ref()
            .is_some_and(|r| r.logged_in && r.reference.is_some() && !r.models.is_empty());
        let detail = inner.detail.clone().unwrap_or_else(|| {
            match live.as_ref().map(|r| r.state.as_str()) {
                Some("unsupported-host") => {
                    "WorkBuddy 宿主接线位置不兼容，未启用分享；请使用普通启动".into()
                }
                Some("ready") if ready => {
                    "已连接当前 WorkBuddy 账户的可选模型目录；实际可用权限及费用以该账户/组织为准"
                        .into()
                }
                Some("login-required") => {
                    "请在 WorkBuddy 内完成登录后重新读取；Hub 不读取或复制账户凭据".into()
                }
                Some("catalog-pending") => {
                    "WorkBuddy 尚未确认当前账户的远端模型目录，请等待加载完成后重新读取".into()
                }
                Some(_) => "WorkBuddy 尚未提供可用的本机分享引用，请重新读取状态".into(),
                None if inner.session.is_some() => {
                    "正在等待 WorkBuddy 宿主连接；若长时间未就绪，请退出 WorkBuddy 后重新启用"
                        .into()
                }
                None => {
                    "点击启用分享后连接 WorkBuddy 当前登录账户；读取此页面不会启动客户端".into()
                }
            }
        });
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
            json!({"revision":config.revision,"enabled":inner.session.is_some(),"ready":ready,
            "loggedIn":live.as_ref().is_some_and(|r|r.logged_in),"accountName":live.as_ref().and_then(|r|r.account_name.as_ref()),
            "needsRestart":inner.needs_restart,"detail":detail,"models":rows,"targets":targets}),
        )
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
        let report = inner
            .session
            .as_ref()
            .and_then(|s| s.lease.live())
            .context("请先启用 WorkBuddy 内置模型分享")?;
        ensure!(
            report.logged_in && report.reference.is_some(),
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
