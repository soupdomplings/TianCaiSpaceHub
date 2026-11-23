//! Desktop endpoint for the WeChat client. Pairing keys reach the phone by QR only.
mod envelope;
mod rpc;
use super::{ApiResult, SecretStore, SharedState, ensure_loaded, root};
use anyhow::{Context, Result, ensure};
use axum::{Json, Router, extract::State, routing::post};
use envelope::Envelope;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tokio::{
    sync::Mutex,
    task::{JoinHandle, JoinSet},
};

#[derive(Default)]
pub(super) struct Mobile {
    local: Mutex<Local>,
    worker: Mutex<Option<JoinHandle<()>>>,
    streams: rpc::Streams,
}
#[derive(Default)]
struct Local {
    loaded: bool,
    vault: Vault,
    invite: Option<Invite>,
    pending: Vec<Pending>,
    error: Option<String>,
}
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Vault {
    origin: String,
    device: String,
    token: String,
    bindings: BTreeMap<String, Binding>,
    revoked: BTreeSet<String>,
    writes: BTreeMap<String, u64>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Binding {
    name: String,
    key: String,
}
struct Invite {
    id: String,
    key: String,
    expires: u64,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    binding_id: String,
    invite_id: String,
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Poll {
    requests: Vec<Envelope>,
    pending: Vec<Pending>,
}

fn now() -> u64 {
    crate::types::now_ms() as u64
}
fn origin() -> Result<String> {
    let raw = std::env::var("TIANCAI_MOBILE_RELAY_URL")
        .ok()
        .or_else(|| option_env!("TIANCAI_MOBILE_RELAY_URL").map(str::to_owned))
        .filter(|s| !s.trim().is_empty())
        .context("微信小程序连接服务尚未配置")?;
    let url = url::Url::parse(raw.trim())?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "小程序服务需要 HTTPS 域名"
    );
    Ok(url.origin().ascii_serialization())
}
fn store(state: &SharedState) -> Result<SecretStore> {
    SecretStore::new(root(&state.config_path).join("phone-credentials"))
}
fn load(state: &SharedState, local: &mut Local) -> Result<()> {
    if !local.loaded {
        if let Some(bytes) = store(state)?.get("phone-v1")? {
            local.vault = serde_json::from_slice(&bytes)?
        }
        local.loaded = true;
    }
    Ok(())
}
fn save(state: &SharedState, local: &mut Local, next: Vault) -> Result<()> {
    store(state)?.set("phone-v1", &serde_json::to_vec(&next)?)?;
    local.vault = next;
    Ok(())
}
fn http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(6))
        .timeout(Duration::from_secs(25))
        .build()?)
}
async fn cloud(
    http: &reqwest::Client,
    origin: &str,
    token: &str,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    let url = format!("{origin}/v1/hub/{path}");
    let mut request = match body {
        Some(v) => http.post(url).json(&v),
        None => http.get(url),
    };
    if !token.is_empty() {
        request = request.bearer_auth(token)
    }
    let mut res = request.send().await.context("小程序连接服务暂不可达")?;
    ensure!(
        res.status().is_success(),
        "小程序连接服务返回 {}",
        res.status().as_u16()
    );
    let mut bytes = vec![];
    while let Some(chunk) = res.chunk().await? {
        ensure!(bytes.len() + chunk.len() <= 8 * 1024 * 1024, "中转响应过大");
        bytes.extend_from_slice(&chunk)
    }
    Ok(serde_json::from_slice(&bytes)?)
}
pub(super) fn routes() -> Router<SharedState> {
    Router::new()
        .route("/mobile/invite", post(invite))
        .route("/mobile/decide", post(decide))
        .route("/mobile/revoke", post(revoke))
}
pub(super) async fn info(state: &SharedState) -> Value {
    let registered = state.remote_network.inner.lock().await.config.mobile_registered;
    let mut l = state.remote_network.mobile.local.lock().await;
    if registered && let Err(err) = load(state, &mut l) {
        l.error = Some(err.to_string());
    }
    let valid = l.invite.as_ref().filter(|i| i.expires > now());
    json!({"configured":origin().is_ok(),"error":l.error,"bindings":l.vault.bindings.iter().map(|(id,b)|json!({"id":id,"name":b.name})).collect::<Vec<_>>(),
        "pending":l.pending.iter().filter(|p|valid.is_some_and(|i|i.id==p.invite_id)).map(|p|json!({"id":p.binding_id,"name":p.name})).collect::<Vec<_>>()})
}
async fn invite(State(state): State<SharedState>) -> ApiResult {
    ensure_loaded(&state).await?;
    let _operation = state.remote_network.operation.lock().await;
    let name = {
        let inner = state.remote_network.inner.lock().await;
        super::check(inner.config.enabled, "请先在设置中启用远程连接")?;
        inner.config.device_name.clone()
    };
    let origin = origin()?;
    let http = http()?;
    let mut local = state.remote_network.mobile.local.lock().await;
    load(&state, &mut local)?;
    if local.vault.token.is_empty() {
        let res = cloud(&http, &origin, "", "register", Some(json!({"name":name}))).await?;
        let next = Vault {
            origin: origin.clone(),
            device: res["deviceId"]
                .as_str()
                .context("注册响应缺少设备标识")?
                .to_owned(),
            token: res["token"]
                .as_str()
                .context("注册响应缺少设备凭据")?
                .to_owned(),
            ..Default::default()
        };
        save(&state, &mut local, next)?;
    }
    super::check(
        local.vault.origin == origin,
        "当前域名与已注册设备的服务不同",
    )?;
    let res = cloud(
        &http,
        &origin,
        &local.vault.token,
        "invite",
        Some(json!({})),
    )
    .await?;
    let id = res["inviteId"]
        .as_str()
        .context("未获取配对邀请")?
        .to_owned();
    let expires = res["expiresAt"].as_u64().context("无效的配对有效期")?;
    let key = envelope::new_key();
    let qr = json!({"v":1,"deviceId":local.vault.device,"inviteId":id,"key":key}).to_string();
    local.invite = Some(Invite { id, key, expires });
    local.pending.clear();
    drop(local);
    {
        let mut inner = state.remote_network.inner.lock().await;
        if !inner.config.mobile_registered {
            inner.config.mobile_registered = true;
            super::persist(&state, &inner)?
        }
    }
    start(&state).await?;
    Ok(Json(json!({"qr":qr,"expiresAt":expires})))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Decision {
    binding_id: String,
    allow: bool,
}
async fn decide(State(s): State<SharedState>, Json(d): Json<Decision>) -> ApiResult {
    let _operation = s.remote_network.operation.lock().await;
    super::check(
        s.remote_network.inner.lock().await.config.enabled,
        "远程连接已关闭",
    )?;
    let mut l = s.remote_network.mobile.local.lock().await;
    let p = l
        .pending
        .iter()
        .find(|p| p.binding_id == d.binding_id)
        .context("手机申请已过期")?
        .clone();
    let invite = l
        .invite
        .as_ref()
        .filter(|i| i.id == p.invite_id && i.expires > now())
        .context("请重新扫码")?;
    let mut next = l.vault.clone();
    if d.allow {
        next.bindings.insert(
            d.binding_id.clone(),
            Binding {
                name: p.name,
                key: invite.key.clone(),
            },
        );
    } else {
        next.bindings.remove(&d.binding_id);
    }
    let http = http()?;
    // Save the key first: a lost acknowledgement must never leave an unrecoverable approved pairing.
    save(&s, &mut l, next)?;
    cloud(
        &http,
        &l.vault.origin,
        &l.vault.token,
        "decide",
        Some(json!({"bindingId":d.binding_id,"allow":d.allow})),
    )
    .await?;
    l.invite = None;
    l.pending.clear();
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    binding_id: String,
}
async fn revoke(State(s): State<SharedState>, Json(t): Json<Target>) -> ApiResult {
    let mut l = s.remote_network.mobile.local.lock().await;
    load(&s, &mut l)?;
    let mut next = l.vault.clone();
    super::check(
        next.bindings.remove(&t.binding_id).is_some(),
        "没有这台已配对手机",
    )?;
    next.revoked.insert(t.binding_id.clone());
    save(&s, &mut l, next)?;
    drop(l);
    s.remote_network.mobile.streams.revoke(&t.binding_id).await;
    Ok(Json(json!({"ok":true})))
}

pub(super) async fn sync(s: &SharedState) {
    if let Err(err) = start(s).await {
        s.remote_network.mobile.local.lock().await.error = Some(err.to_string());
    }
}
pub(super) async fn start(s: &SharedState) -> Result<()> {
    let enabled = {
        let i = s.remote_network.inner.lock().await;
        i.config.enabled && i.config.mobile_registered
    };
    if !enabled {
        stop(s).await;
        return Ok(());
    }
    let mut worker = s.remote_network.mobile.worker.lock().await;
    if worker.as_ref().is_some_and(|h| !h.is_finished()) {
        return Ok(());
    }
    let mut l = s.remote_network.mobile.local.lock().await;
    load(s, &mut l)?;
    let origin = origin()?;
    ensure!(
        l.vault.origin == origin && !l.vault.token.is_empty(),
        "微信设备凭据与服务域名不匹配，请检查发行配置"
    );
    let token = l.vault.token.clone();
    drop(l);
    let state = s.clone();
    let http = http()?;
    *worker = Some(tokio::spawn(async move {
        run(state, http, origin, token).await
    }));
    Ok(())
}
pub(super) async fn stop(s: &SharedState) {
    if let Some(handle) = s.remote_network.mobile.worker.lock().await.take() {
        handle.abort();
        let _ = handle.await;
    }
    let mut l = s.remote_network.mobile.local.lock().await;
    l.pending.clear();
    l.invite = None;
    s.remote_network.mobile.streams.clear().await;
}
async fn run(state: SharedState, http: reqwest::Client, origin: String, token: String) {
    let mut jobs = JoinSet::new();
    let mut delay = 1;
    loop {
        while jobs.try_join_next().is_some() {}
        let revoked = state
            .remote_network
            .mobile
            .local
            .lock()
            .await
            .vault
            .revoked
            .clone();
        for id in revoked {
            if cloud(
                &http,
                &origin,
                &token,
                "revoke",
                Some(json!({"bindingId":id})),
            )
            .await
            .is_ok()
            {
                let mut l = state.remote_network.mobile.local.lock().await;
                let mut next = l.vault.clone();
                next.revoked.remove(&id);
                let _ = save(&state, &mut l, next);
            }
        }
        let poll = async {
            Ok::<Poll, anyhow::Error>(serde_json::from_value(
                cloud(&http, &origin, &token, "poll", None).await?,
            )?)
        }
        .await;
        match poll {
            Ok(p) => {
                delay = 1;
                {
                    let mut l = state.remote_network.mobile.local.lock().await;
                    l.error = None;
                    l.pending = p.pending;
                }
                for e in p.requests {
                    if jobs.len() >= 24 {
                        jobs.join_next().await;
                    }
                    let (s, h, o, t) = (state.clone(), http.clone(), origin.clone(), token.clone());
                    jobs.spawn(async move {
                        let _ = handle(&s, &h, &o, &t, e).await;
                    });
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(err) => {
                state.remote_network.mobile.local.lock().await.error = Some(err.to_string());
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(30);
            }
        }
    }
}
async fn handle(
    s: &SharedState,
    http: &reqwest::Client,
    origin: &str,
    token: &str,
    e: Envelope,
) -> Result<()> {
    let mut l = s.remote_network.mobile.local.lock().await;
    let key = l
        .vault
        .bindings
        .get(&e.binding_id)
        .context("手机未配对")?
        .key
        .clone();
    let call = e.open(&key)?;
    let now = now();
    let allowed = now.abs_diff(call.issued_at) <= 120_000;
    let repeated = l.vault.writes.contains_key(&e.request_id);
    if allowed && !repeated && call.method == "POST" {
        let mut next = l.vault.clone();
        next.writes.retain(|_, expiry| *expiry >= now);
        ensure!(next.writes.len() < 400, "请稍后再次操作");
        next.writes
            .insert(e.request_id.clone(), call.issued_at.saturating_add(120_000));
        save(s, &mut l, next)?;
    }
    drop(l);
    let result = if !allowed || repeated {
        json!({"status":409,"body":{"error":{"message":"操作已过期或已提交，请刷新会话核对"}}})
    } else {
        rpc::execute(s, &e.binding_id, call).await.unwrap_or_else(
            |err| json!({"status":409,"body":{"error":{"message":err.to_string()}}}),
        )
    };
    ensure!(
        s.remote_network
            .mobile
            .local
            .lock()
            .await
            .vault
            .bindings
            .contains_key(&e.binding_id),
        "手机已撤销"
    );
    let sealed = e.reply(&key, &result)?;
    cloud(
        http,
        origin,
        token,
        "reply",
        Some(serde_json::to_value(sealed)?),
    )
    .await?;
    Ok(())
}
