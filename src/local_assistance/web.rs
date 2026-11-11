use super::*;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/desktop/assistance", get(status).post(save))
        .route("/api/desktop/assistance/read", post(status))
        .route("/api/desktop/assistance/control", post(user_control))
        .route("/api/desktop/assistance/clear", post(clear_records))
        .route("/api/desktop/assistance/prepare", post(prepare))
        .route("/api/desktop/assistance/uninstall", post(uninstall))
        .route("/api/local-assistance/mcp", post(mcp))
        .route("/api/local-assistance/model/{task}/responses", post(model))
        .route(
            "/api/local-assistance/model/{task}/responses/compact",
            post(compact),
        )
        .route(
            "/api/local-assistance/model/{task}/models",
            get(model_catalog),
        )
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
}
fn response(r: Result<Value>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::CONFLICT, Json(json!({"error":e.to_string()}))).into_response(),
    }
}
fn trusted(headers: &HeaderMap) -> Result<()> {
    ensure!(
        headers.get("origin").is_none()
            && headers
                .get("x-tiancai-desktop")
                .and_then(|v| v.to_str().ok())
                == Some("1"),
        "请从 Hub 界面操作本机协助"
    );
    Ok(())
}
pub async fn view(state: &SharedState) -> Result<Value> {
    let (settings, revision) = settings::read(&state.config_path)?;
    let config = state.config.lock().await.clone();
    let options = settings::models(&config);
    let mut runtime = super::runtime::check(&state.config_path, settings.binary.as_deref());
    runtime.can_uninstall &=
        !settings.enabled && !crate::assistant::settings::read(&state.config_path)?.enabled;
    let binary = runtime.program.clone();
    let inner = state.local_assistance.inner.lock().await;
    let tasks: Vec<_> = inner.tasks.values().map(|t| t.view(true)).collect();
    let records = records::list(&state.config_path)?;
    let detail = if settings.native_disable_pending {
        format!(
            "本机协助已停用，等待天工可用后移除配置。{}",
            inner.host_detail
        )
    } else if settings.native_catalog_pending {
        format!(
            "工具目录正在接入。{}\n确认天工已打开，稍后重新打开此设置查看结果。",
            inner.host_detail
        )
    } else {
        inner.host_detail.clone()
    };
    Ok(
        json!({"enabled":settings.enabled,"model":settings.model,"route":settings.route,"reasoningEffort":settings.reasoning_effort,"binary":binary,"selectedBinary":settings.binary,
        "allowWorkspaceWrite":settings.allow_workspace_write,"fullAccess":settings.full_access,"autoClear":settings.auto_clear,"revision":revision,
        "models":options,"hostReady":inner.host.is_some() && !settings.native_catalog_pending,"hostDetail":detail,"tasks":tasks,
        "nativeCatalogPending":settings.native_catalog_pending,
        "runtime":runtime,
        "triggerState":inner.trigger_state,
        "nativeDisablePending":settings.native_disable_pending,"records":records}),
    )
}
async fn status(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    response(
        async {
            trusted(&headers)?;
            view(&state).await
        }
        .await,
    )
}
#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Prepare {
    confirmed: bool,
    binary: Option<PathBuf>,
}
impl Default for Prepare {
    fn default() -> Self {
        Self {
            confirmed: false,
            binary: None,
        }
    }
}
async fn prepare(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Prepare>,
) -> Response {
    response(async {
        trusted(&headers)?;
        let _gate = state.local_assistance.setup.lock().await;
        let proxy = state.config.lock().await.outbound_proxy.clone();
        let saved = settings::read(&state.config_path)?.0;
        ensure!(saved.enabled || crate::assistant::settings::read(&state.config_path)?.enabled,
            "请先启用天工本机协助或天才搭档，再确认准备共享 Server");
        let selected = request.binary.as_deref().or(saved.binary.as_deref());
        let ready = super::runtime::prepare(&state.config_path, selected, &proxy, request.confirmed).await?;
        Ok(json!({"binary":ready.program,"runtime":super::runtime::check(&state.config_path,None),
            "detail":"共享天才搭档 Server 已准备。保存设置后，新任务将使用此后台。"}))
    }.await)
}
async fn uninstall(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Prepare>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            ensure!(
                request.confirmed,
                "请本人确认移除 Hub 自管的天才搭档 Server"
            );
            let _gate = state.local_assistance.setup.lock().await;
            ensure!(
                !settings::read(&state.config_path)?.0.enabled
                    && !crate::assistant::settings::read(&state.config_path)?.enabled,
                "天工本机协助与天才搭档共用此 Server，请先停用两个功能再移除安装"
            );
            ensure!(
                state
                    .local_assistance
                    .inner
                    .lock()
                    .await
                    .tasks
                    .values()
                    .all(|task| task.worker_stopped)
                    && !crate::assistant::has_active(&state).await,
                "共享 Server 的后台仍在退出，请稍后重试移除"
            );
            super::runtime::uninstall(&state.config_path).await
        }
        .await,
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Save {
    revision: String,
    enabled: bool,
    model: String,
    route: String,
    #[serde(default)]
    reasoning_effort: Option<String>,
    binary: Option<PathBuf>,
    allow_workspace_write: bool,
    #[serde(default)]
    full_access: bool,
}
async fn save(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(r): Json<Save>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            let _gate = state.local_assistance.setup.lock().await;
            let (old, revision) = settings::read(&state.config_path)?;
            ensure!(r.revision == revision, "协助设置已变化，请重新打开设置");
            let mut next = settings::Settings {
                enabled: r.enabled,
                model: r.model,
                route: r.route,
                reasoning_effort: r.reasoning_effort,
                binary: r.binary,
                allow_workspace_write: r.allow_workspace_write,
                full_access: r.full_access,
                auto_clear: false,
                ..old.clone()
            };
            if next.token.is_empty() {
                next.token = settings::token();
            }
            let config = state.config.lock().await.clone();
            settings::validate(&state.config_path, &config, &next)?;
            // Saving opt-in and native MCP metadata never installs a runtime.
            // UI receives needsPreparation and asks the person before prepare.
            if !next.enabled {
                // Local revocation must work even if TianGong is closed. Synchronize
                // its owned native entry next time the official endpoint is ready.
                next.token = settings::token();
                next.native_disable_pending = old.native_fingerprint.is_some();
                next.native_catalog_pending = false;
                settings::write(&state.config_path, &next, &revision)?;
                let host = state.local_assistance.inner.lock().await.host.clone();
                if let Some(host) = host {
                    disconnect(&state, &host).await;
                }
                drop(_gate);
                if let Err(error) = sync_disabled(&state).await {
                    state.local_assistance.inner.lock().await.host_detail = error.to_string();
                }
                return view(&state).await;
            }
            next.native_disable_pending = false;
            next.native_catalog_pending = true;
            // The native writer uses the official API and refuses independent edits.
            let client = crate::desktop::tiangong_resources::client(&config.gmclaw_bridge).await?;
            let before = client.mcp_connection(settings::MCP_NAME).await?;
            if let Some(before) = &before {
                ensure!(
                    old.native_fingerprint.as_deref()
                        == Some(&settings::native_fingerprint(before)),
                    "天工已独立修改此 MCP；请在天工移除本机协助连接后重新启用，未覆盖现有内容"
                );
            }
            let after = settings::native_value(
                &next,
                config.local_listen_port().context("Hub 本机服务端口无效")?,
            );
            let store = crate::desktop::transaction::TransactionStore::new(
                settings::root(&state.config_path).join("native-undo"),
            )?;
            store.put_record(
                &uuid::Uuid::new_v4().to_string(),
                &json!({"before":before,"after":after}),
            )?;
            if before.is_some() {
                client
                    .mcp_update_connection(settings::MCP_NAME, after.clone())
                    .await?;
            } else if next.enabled {
                let mut v = after.clone();
                v["server_name"] = settings::MCP_NAME.into();
                client.mcp_create_connection(v).await?;
            }
            let confirmed = client.mcp_connection(settings::MCP_NAME).await?;
            ensure!(
                confirmed.is_some(),
                "天工未返回已保存的协助连接，请重新打开设置重试"
            );
            ensure!(
                confirmed.as_ref().map(settings::native_fingerprint)
                    == Some(settings::native_fingerprint(&after)),
                "天工协助连接保存后与预期不同，未继续启用；请在天工检查该连接后重试"
            );
            next.native_fingerprint = confirmed.as_ref().map(settings::native_fingerprint);
            if let Err(error) = settings::write(&state.config_path, &next, &revision) {
                // Compensate only if no other writer has changed our native result.
                if client
                    .mcp_connection(settings::MCP_NAME)
                    .await?
                    .as_ref()
                    .map(settings::native_fingerprint)
                    == next.native_fingerprint
                {
                    if let Some(before) = before {
                        let _ = client
                            .mcp_update_connection(
                                settings::MCP_NAME,
                                settings::native_fields(&before),
                            )
                            .await;
                    } else if confirmed.is_some() {
                        let _ = client.mcp_delete_connection(settings::MCP_NAME).await;
                    }
                }
                return Err(error);
            }
            let host = state.local_assistance.inner.lock().await.host.clone();
            if let Some(host) = host {
                super::disconnect(&state, &host).await;
            }
            drop(_gate);
            if let Err(e) = sync_catalogue(&state).await {
                state.local_assistance.inner.lock().await.host_detail = e.to_string();
            }
            view(&state).await
        }
        .await,
    )
}
pub(super) async fn probe_local(port: u16, token: &str) -> Result<Value> {
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let url = format!("http://127.0.0.1:{port}/api/local-assistance/mcp");
    let init:Value=http.post(&url).bearer_auth(token).json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"tiancai-setup","version":env!("CARGO_PKG_VERSION")}}})).send().await?.error_for_status()?.json().await?;
    ensure!(
        init["id"] == 1
            && init["result"]["serverInfo"]["name"] == settings::MCP_NAME
            && init["result"]["protocolVersion"] == "2025-06-18",
        "本机协助握手未通过，未将天工标记为已接入"
    );
    let response: Value = http
        .post(url)
        .bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    ensure!(
        response["id"] == 2 && response["result"]["tools"] == json!(tools()),
        "本机协助工具目录不完整，未将天工标记为已接入"
    );
    Ok(response["result"]["tools"].clone())
}
pub(super) async fn sync_catalogue(state: &SharedState) -> Result<()> {
    let _gate = state.local_assistance.setup.lock().await;
    let (s, revision) = settings::read(&state.config_path)?;
    if !s.enabled {
        return Ok(());
    }
    let config = state.config.lock().await.clone();
    let tools = probe_local(
        config.local_listen_port().context("Hub 本机端口无效")?,
        &s.token,
    )
    .await?;
    let client = crate::desktop::tiangong_resources::client(&config.gmclaw_bridge).await?;
    sync_catalogue_native(state, s, &revision, &client, tools).await
}
// The caller holds the setup gate; tests inject only a loopback native API.
pub(super) async fn sync_catalogue_native(
    state: &SharedState,
    mut s: settings::Settings,
    revision: &str,
    client: &crate::gmclaw_desktop::DesktopClient,
    tools: Value,
) -> Result<()> {
    let Some(before) = client.mcp_connection(settings::MCP_NAME).await? else {
        // An actual 404 is user intent; an unreachable endpoint is never deletion.
        s.enabled = false;
        s.token = settings::token();
        s.native_fingerprint = None;
        s.native_disable_pending = false;
        s.native_catalog_pending = false;
        settings::write(&state.config_path, &s, revision)?;
        let host = state.local_assistance.inner.lock().await.host.clone();
        if let Some(host) = host {
            disconnect(state, &host).await;
        }
        let mut inner = state.local_assistance.inner.lock().await;
        inner.host_detail =
            "本机协助已在天工移除，Hub 已同步停用；如需使用，可在此重新启用并保存".into();
        inner.trigger_state = "removed".into();
        return Ok(());
    };
    if before["status"] == "inactive" {
        let mut active = before.clone();
        active["status"] = "active".into();
        if s.native_fingerprint.as_deref() == Some(&settings::native_fingerprint(&active)) {
            s.enabled = false;
            s.token = settings::token();
            s.native_fingerprint = Some(settings::native_fingerprint(&before));
            s.native_catalog_pending = false;
            s.native_disable_pending = false;
            settings::write(&state.config_path, &s, revision)?;
            let host = state.local_assistance.inner.lock().await.host.clone();
            if let Some(host) = host {
                disconnect(state, &host).await;
            }
            let mut inner = state.local_assistance.inner.lock().await;
            inner.host_detail =
                "本机协助已在天工停用，Hub 已同步；如需使用，可在此重新启用并保存".into();
            inner.trigger_state = "disabled".into();
            return Ok(());
        }
    }
    ensure!(
        s.native_fingerprint.as_deref() == Some(&settings::native_fingerprint(&before)),
        "天工内的本机协助配置已独立修改，未覆盖工具目录"
    );
    let prior = before["tools_json"]
        .as_str()
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .unwrap_or(before["tools_json"].clone());
    let port = state
        .config
        .lock()
        .await
        .local_listen_port()
        .context("Hub 本机端口无效")?;
    let desired = settings::native_value(&s, port);
    let static_changed =
        settings::native_fingerprint(&desired) != settings::native_fingerprint(&before);
    let changed = static_changed || prior != tools || before["is_connected"] != 1;
    let mut revision = revision.to_string();
    if changed && !s.native_catalog_pending {
        s.native_catalog_pending = true;
        revision = settings::write_revision(&state.config_path, &s, &revision)?;
    }
    if static_changed {
        let store = crate::desktop::transaction::TransactionStore::new(
            settings::root(&state.config_path).join("native-undo"),
        )?;
        store.put_record(
            &uuid::Uuid::new_v4().to_string(),
            &json!({"before":before,"after":desired}),
        )?;
        // Owned static upgrades (including a changed Hub port) are transactional
        // in local state; external native edits still fail the prior fingerprint.
        client
            .mcp_update_connection(settings::MCP_NAME, settings::native_fields(&desired))
            .await?;
        s.native_fingerprint = Some(settings::native_fingerprint(&desired));
        ensure!(
            client
                .mcp_connection(settings::MCP_NAME)
                .await?
                .as_ref()
                .map(settings::native_fingerprint)
                == s.native_fingerprint,
            "天工协助配置升级后读回不一致，请核对连接；未继续覆盖"
        );
        revision = settings::write_revision(&state.config_path, &s, &revision)?;
    }
    if changed {
        client
            .mcp_update_connection(
                settings::MCP_NAME,
                json!({"is_connected":1,"tools_json":tools,"conn_last_error":"","retry_count":0}),
            )
            .await?;
    }
    let confirmed = client
        .mcp_connection(settings::MCP_NAME)
        .await?
        .context("天工协助目录保存后无法读取")?;
    ensure!(
        s.native_fingerprint.as_deref() == Some(&settings::native_fingerprint(&confirmed)),
        "天工协助配置在更新目录时变化，未继续接入"
    );
    let actual = confirmed["tools_json"]
        .as_str()
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .unwrap_or(confirmed["tools_json"].clone());
    ensure!(
        confirmed["is_connected"] == 1 && actual == tools,
        "天工工具目录保存未确认，请稍后重试"
    );
    if changed || s.native_catalog_pending {
        client.reload_configuration().await?;
    }
    if !changed && !s.native_catalog_pending {
        return Ok(());
    }
    s.native_catalog_pending = false;
    settings::write(&state.config_path, &s, &revision)
}
pub(super) async fn sync_disabled(state: &SharedState) -> Result<()> {
    let _gate = state.local_assistance.setup.lock().await;
    let (s, revision) = settings::read(&state.config_path)?;
    if s.enabled || !s.native_disable_pending {
        return Ok(());
    }
    let config = state.config.lock().await.clone();
    let client = crate::desktop::tiangong_resources::client(&config.gmclaw_bridge).await?;
    sync_disabled_native(state, s, &revision, &client).await
}
pub(super) async fn sync_disabled_native(
    state: &SharedState,
    mut s: settings::Settings,
    revision: &str,
    client: &crate::gmclaw_desktop::DesktopClient,
) -> Result<()> {
    if let Some(before) = client.mcp_connection(settings::MCP_NAME).await? {
        ensure!(
            s.native_fingerprint.as_deref() == Some(&settings::native_fingerprint(&before)),
            "本机协助已停用；天工内该连接已独立修改，请手动停用或移除"
        );
        let store = crate::desktop::transaction::TransactionStore::new(
            settings::root(&state.config_path).join("native-undo"),
        )?;
        store.put_record(
            &uuid::Uuid::new_v4().to_string(),
            &json!({"before":before,"after":null}),
        )?;
        client.mcp_delete_connection(settings::MCP_NAME).await?;
        ensure!(
            client.mcp_connection(settings::MCP_NAME).await?.is_none(),
            "天工尚未确认移除本机协助，请稍后重试"
        );
        s.native_fingerprint = None;
    } else {
        s.native_fingerprint = None;
    }
    client.reload_configuration().await?;
    s.native_disable_pending = false;
    settings::write(&state.config_path, &s, revision)?;
    let mut inner = state.local_assistance.inner.lock().await;
    inner.host_detail = "本机协助已停用，并已从天工移除配置。".into();
    inner.trigger_state = "disabled".into();
    Ok(())
}
async fn clear_records(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    response(
        async {
            trusted(&headers)?;
            records::clear_finished(&state).await
        }
        .await,
    )
}
#[derive(Deserialize)]
struct UserControl {
    task: String,
    action: String,
}
async fn user_control(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(r): Json<UserControl>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            let action = match r.action.as_str() {
                "stop" => Control::Cancel,
                "delete" => Control::Delete,
                _ => anyhow::bail!("未知协助操作"),
            };
            control(&state, None, &r.task, action).await
        }
        .await,
    )
}
fn credential(headers: &HeaderMap, token: &str) -> bool {
    !token.is_empty()
        && headers.get("origin").is_none()
        && headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            == Some(token)
}
async fn mcp(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> Response {
    let auth = settings::read(&state.config_path)
        .ok()
        .is_some_and(|(s, _)| s.enabled && credential(&headers, &s.token));
    if !auth {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let id = v.get("id").cloned();
    let method = v["method"].as_str().unwrap_or("");
    if id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    let result: Result<Value> = match method {
        "initialize" => Ok(
            json!({"protocolVersion":match v["params"]["protocolVersion"].as_str(){Some("2024-11-05")=>"2024-11-05",Some("2025-03-26")=>"2025-03-26",_=>"2025-06-18"},
            "capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":settings::MCP_NAME,"version":env!("CARGO_PKG_VERSION")},
            "instructions":"天工任务遇到连续失败、缺少能力或关键验证失败时主动调用 assist_start，无需用户点名。上下文由 Hub 自动提供。结果回到天工继续任务，禁止模型自行批准。"}),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools":tools()})),
        "tools/call" => {
            let guard = matches!(
                v["params"]["name"].as_str(),
                Some("assist_start" | "assist_followup")
            )
            .then(|| {
                state.desktop_mcp_activity.begin(
                    "tiancai-local-assistance",
                    "本机协助",
                    crate::desktop::clients::ClientId::Tiangong,
                    false,
                )
            });
            let call = tool(&state, &v["params"]).await;
            drop(guard);
            if let Err(error) = &call {
                let mut inner = state.local_assistance.inner.lock().await;
                inner.trigger_state = "call_error".into();
                inner.host_detail = format!("天工已尝试调用协助，但未通过：{error}");
            }
            Ok(match call {
                Ok(mut value) => {
                    if let Some(obj) = value.as_object_mut() {
                        let count = obj
                            .remove("pending")
                            .and_then(|v| v.as_array().map(Vec::len))
                            .unwrap_or(0);
                        obj.insert("pendingCount".into(), json!(count));
                    }
                    json!({"content":[{"type":"text","text":serde_json::to_string(&value).unwrap_or_default()}],"isError":false})
                }
                Err(e) => json!({"content":[{"type":"text","text":e.to_string()}],"isError":true}),
            })
        }
        _ => Err(anyhow::anyhow!("不支持的 MCP 方法")),
    };
    Json(match result {
        Ok(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
        Err(e) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":e.to_string()}}),
    })
    .into_response()
}
pub(super) fn tools() -> Vec<Value> {
    let common = json!({"context_ref":{"type":"string","description":"系统指令中自动提供的本次天工上下文，不可编造"},"task":{"type":"string","description":"已返回的协助任务 ID"}});
    let mut list = vec![
        json!({"name":"assist_start","description":"遇到同类失败两次、能力不足或关键验证失败时自动请求本机协助。只交接目标、必要证据与失败方法，保留用户限制；无需让用户点名。相同问题复用 problem_key，不并发修改同一项目。",
        "inputSchema":{"type":"object","properties":{"context_ref":common["context_ref"],"goal":{"type":"string"},"problem_key":{"type":"string"},"analysis_only":{"type":"boolean","default":true}},"required":["context_ref","goal","problem_key"],"additionalProperties":false}}),
    ];
    for (name, description) in [
        (
            "assist_wait",
            "自动等待已有协助，最多20秒；starting、running、waiting_user 都继续等待同一任务，不结束主任务。waiting_user 时用户在天工处理，绝不代批代答；处理后继续等待结果。",
        ),
        (
            "assist_followup",
            "协助完成后在同一会话补充证据追问，最多两次，不能新建同问题任务。",
        ),
        (
            "assist_cancel",
            "停止本次协助，等待 cancelled/stopped；中断不会回滚文件。",
        ),
        (
            "assist_finish",
            "采纳结果后结束协助，使后台退出；按设置自动清理独立会话，天工主对话结果保留。",
        ),
    ] {
        let mut props = common.clone();
        if name == "assist_followup" {
            props["message"] = json!({"type":"string"});
        }
        if name == "assist_wait" {
            props["seconds"] = json!({"type":"integer","minimum":0,"maximum":20,"default":20});
        }
        let required = if name == "assist_followup" {
            json!(["context_ref", "task", "message"])
        } else {
            json!(["context_ref", "task"])
        };
        list.push(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":props,"required":required,"additionalProperties":false}}));
    }
    list
}
pub(super) async fn tool(state: &SharedState, p: &Value) -> Result<Value> {
    let args = &p["arguments"];
    let context = args["context_ref"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .context("缺少 arguments.context_ref：请使用本轮系统规则由 Hub 提供的真实 context_ref 重试，不要向用户索取或猜测；旧任务未注入规则时需新建天工任务")?;
    let name = p["name"].as_str().unwrap_or("");
    if name == "assist_start" {
        let result = admit(
            state.clone(),
            context,
            args["goal"].as_str().unwrap_or(""),
            args["problem_key"].as_str().unwrap_or(""),
            args["analysis_only"].as_bool().unwrap_or(true),
        )
        .await;
        if result.is_ok() {
            let mut inner = state.local_assistance.inner.lock().await;
            inner.trigger_state = "called".into();
            inner.host_detail = "天工已发起本机协助，进度与人类确认在天工窗口处理".into();
        }
        return result;
    }
    let id = args["task"].as_str().context("协助任务 ID 不能为空")?;
    task_view(state, context, id).await?;
    match name {
        "assist_wait" => {
            let seconds = args["seconds"].as_u64().unwrap_or(20).min(20);
            let notified = state.local_assistance.changed.notified();
            let view = task_view(state, context, id).await?;
            if matches!(
                view["phase"].as_str(),
                Some("starting" | "running" | "cancelling" | "waiting_user")
            ) && seconds > 0
            {
                let _ = tokio::time::timeout(Duration::from_secs(seconds), notified).await;
            }
            task_view(state, context, id).await
        }
        "assist_followup" => {
            control(
                state,
                Some(context),
                id,
                Control::Followup(args["message"].as_str().unwrap_or("").into()),
            )
            .await
        }
        "assist_cancel" => control(state, Some(context), id, Control::Cancel).await,
        "assist_finish" => control(state, Some(context), id, Control::Finish).await,
        _ => anyhow::bail!("未知协助工具；审批和用户输入不能通过模型工具提交"),
    }
}

pub(super) async fn model_pin(
    state: &SharedState,
    id: &str,
    headers: &HeaderMap,
) -> Result<(String, String, String)> {
    let (enabled, _) = settings::read(&state.config_path)?;
    ensure!(enabled.enabled, "本机协助已停用");
    let inner = state.local_assistance.inner.lock().await;
    let t = inner.tasks.get(id).context("协助任务不存在")?;
    ensure!(
        credential(headers, &t.api_token)
            && live(&inner, t)
            && !t.worker_stopped
            && !t.phase.terminal(),
        "协助模型请求不属于活动任务或天工已断开"
    );
    let (model, route, fingerprint) = (
        t.model.clone(),
        t.route.clone(),
        t.route_fingerprint.clone(),
    );
    drop(inner);
    let config = state.config.lock().await;
    let p = config
        .ai_gateway
        .providers
        .iter()
        .find(|p| {
            p.enabled
                && !p.is_client_reserved()
                && crate::ai_gateway::config::provider_route_id(p) == route
                && p.matches_model(&model)
        })
        .context("协助来源已停用或移除")?;
    ensure!(
        settings::source_fingerprint(p)? == fingerprint,
        "协助来源在任务中发生变化，已停止请求；请在天工重新发起协助"
    );
    Ok((model, route, fingerprint))
}
async fn model(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    mut headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (model, route, fingerprint) = match model_pin(&state, &id, &headers).await {
        Ok(v) => v,
        Err(e) => return response(Err(e)),
    };
    let v: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    if v["model"] != model {
        return response(Err(anyhow::anyhow!(
            "协助模型与已选择模型不一致，未自动切换来源"
        )));
    }
    headers.remove("authorization");
    headers.insert(
        "x-openai-actor-authorization",
        axum::http::HeaderValue::from_static("Bearer codexhub"),
    );
    crate::ai_gateway::handler::handle_local_assistance_responses(
        state,
        headers,
        body,
        (route, fingerprint),
    )
    .await
}
async fn compact(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(e) = model_pin(&state, &id, &headers).await {
        return response(Err(e));
    }
    // A failed compact must preserve the existing prompt; never route elsewhere.
    (StatusCode::BAD_REQUEST,Json(json!({"error":{"message":"本机协助暂不提供独立压缩；请缩小子任务或结束后重新协助","type":"invalid_request_error","code":"unsupported_compaction"}}))).into_response()
}
async fn model_catalog(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    match model_pin(&state, &id, &headers).await {
        Ok((model, route, _)) => {
            let config = state.config.lock().await;
            let mut scoped = config.ai_gateway.clone();
            scoped.providers.retain(|p| {
                crate::ai_gateway::config::provider_route_id(p) == route && !p.is_client_reserved()
            });
            let (mut v, _) =
                crate::ai_gateway::catalog::configured_models_response_with_etag(&scoped);
            if let Some(models) = v["models"].as_array_mut() {
                models.retain(|v| v["slug"] == model);
            }
            Json(v).into_response()
        }
        Err(e) => response(Err(e)),
    }
}
