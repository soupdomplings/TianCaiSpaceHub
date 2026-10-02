use axum::{
    Json, Router,
    body::Body,
    extract::{Query, State},
    http::{
        Request, StatusCode,
        header::{CACHE_CONTROL, EXPIRES, HeaderValue, PRAGMA},
    },
    middleware::{self, Next},
    response::IntoResponse,
    routing::{get, post},
};
use serde::Serialize;
use serde_json::json;

use crate::{
    app_state::{FeishuWsState, ImAccountRuntimeState, SharedState, TelegramState, WechatState},
    chain_log, codex_app_config,
    config::AppConfig,
    remote_control_backend,
};

mod codex_app;
mod im_api;
mod oauth;
mod onboarding;
pub(crate) mod plugins;

pub async fn start_bridge_if_ready(state: &SharedState, event_message: &'static str) -> bool {
    im_api::start_bridge_task(state, im_api::BridgeStartMode::KeepExisting, event_message).await
}

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/oauth/authorize", get(oauth::oauth_authorize))
        .route("/oauth/token", post(oauth::oauth_token))
        .route("/api/status", get(status))
        .route("/api/gui/dashboard", get(gui_dashboard))
        .route("/api/shutdown", post(shutdown))
        .route("/api/config", get(get_config).post(save_config))
        .route("/api/external-import/commit", post(commit_external_import))
        .route(
            "/api/workbuddy/config",
            get(workbuddy_config).post(save_workbuddy_config),
        )
        .route(
            "/api/workbuddy/config/restore",
            post(restore_workbuddy_config),
        )
        .route(
            "/api/workbuddy/config/delete",
            post(delete_workbuddy_config),
        )
        .route(
            "/api/gmclaw/config",
            get(gmclaw_config).post(save_gmclaw_config),
        )
        .route("/api/gmclaw/config/restore", post(restore_gmclaw_config))
        .route("/api/gmclaw/config/activate", post(activate_gmclaw_config))
        .route("/api/gmclaw/config/delete", post(delete_gmclaw_config))
        .route(
            "/api/chatgpt/login/start",
            post(crate::ai_gateway::chatgpt_auth::start_login_api),
        )
        .route(
            "/api/chatgpt/login/status",
            post(crate::ai_gateway::chatgpt_auth::login_status_api),
        )
        .route(
            "/api/chatgpt/login/cancel",
            post(crate::ai_gateway::chatgpt_auth::cancel_login_api),
        )
        .route(
            "/api/chatgpt/account/import",
            post(crate::ai_gateway::chatgpt_auth::import_account_api),
        )
        .route(
            "/api/chatgpt/account/usage",
            post(crate::ai_gateway::chatgpt_auth::account_usage_api),
        )
        .route(
            "/api/chatgpt/account/status",
            post(crate::ai_gateway::chatgpt_auth::account_status_api),
        )
        .route(
            "/api/chatgpt/account/models",
            post(crate::ai_gateway::chatgpt_auth::account_models_api),
        )
        .route(
            "/api/chatgpt/account/logout",
            post(crate::ai_gateway::chatgpt_auth::logout_api),
        )
        .route(
            "/api/codex-app/configure",
            post(codex_app::configure_codex_app),
        )
        .route(
            "/api/codex-app/provider/websocket",
            post(codex_app::set_codex_app_provider_websocket),
        )
        .route(
            "/api/codex-app/provider/delete",
            post(codex_app::delete_codex_app_provider),
        )
        .route(
            "/api/codex-app/repair-gui-environment",
            post(codex_app::repair_codex_app_gui_environment),
        )
        .route(
            "/api/codex-app/uninstall",
            post(codex_app::uninstall_codex_app),
        )
        .route("/api/codex-app/status", get(codex_app::codex_app_status))
        .route(
            "/api/codex-app/models/refresh",
            post(codex_app::refresh_codex_app_models),
        )
        .route(
            "/api/codex-app/enhanced-launch",
            post(codex_app::launch_codex_app_enhanced),
        )
        .route(
            "/api/codex-app/enhanced-launch/preflight",
            get(codex_app::codex_app_enhanced_preflight),
        )
        .route(
            "/api/codex-app/sessions",
            get(codex_app::codex_app_sessions),
        )
        .route(
            "/api/codex-app/session/provider",
            post(codex_app::move_codex_app_session_provider),
        )
        .route("/api/bridge/start", post(im_api::start_bridge))
        .route("/api/bridge/stop", post(im_api::stop_bridge))
        .route(
            "/api/im-channel/enabled",
            post(im_api::set_im_channel_enabled),
        )
        .route("/api/im/accounts", get(im_api::im_accounts))
        .route(
            "/api/im/account/enabled",
            post(im_api::set_im_account_enabled),
        )
        .route("/api/im/account/delete", post(im_api::delete_im_account))
        .route(
            "/api/remote-control/backend-status",
            get(remote_control_backend_status),
        )
        .route(
            "/api/feishu/onboard/start",
            post(onboarding::feishu_onboard_start),
        )
        .route(
            "/api/feishu/onboard/poll",
            post(onboarding::feishu_onboard_poll),
        )
        .route("/api/feishu/bot", get(im_api::feishu_bot_status))
        .route("/api/telegram/bot", get(im_api::telegram_bot_status))
        .route(
            "/api/telegram/configure",
            post(im_api::configure_telegram_bot),
        )
        .route(
            "/api/wechat/onboard/start",
            post(onboarding::wechat_onboard_start),
        )
        .route(
            "/api/wechat/onboard/poll",
            post(onboarding::wechat_onboard_poll),
        )
        .route("/api/wechat/bot", get(im_api::wechat_bot_status))
        .route(
            "/api/wecom/onboard/start",
            post(onboarding::wecom_onboard_start),
        )
        .route(
            "/api/wecom/onboard/poll",
            post(onboarding::wecom_onboard_poll),
        )
        .route("/api/wecom/bot", get(im_api::wecom_bot_status))
        .route("/api/events", get(events))
        .merge(plugins::router())
        .merge(remote_control_backend::router())
        .nest("/ai-gateway", crate::ai_gateway::router())
        .layer(middleware::from_fn(access_log))
        .with_state(state)
}

async fn access_log(request: Request<Body>, next: Next) -> impl IntoResponse {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let started = std::time::Instant::now();
    let mut response = next.run(request).await;
    let status = response.status();
    let elapsed_ms = started.elapsed().as_millis();
    if path.starts_with("/backend-api/") || path.starts_with("/api/") {
        let headers = response.headers_mut();
        headers.insert(
            CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, max-age=0, must-revalidate"),
        );
        headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
        headers.insert(EXPIRES, HeaderValue::from_static("0"));
    }
    chain_log::write_line(format!(
        "[http] method={} path={} status={} elapsed_ms={}",
        method,
        path,
        status.as_u16(),
        elapsed_ms
    ));
    tracing::info!(
        target: "codexhub::http",
        method = %method,
        path,
        status = status.as_u16(),
        elapsed_ms,
        "http request"
    );
    response
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    service: String,
    pid: u32,
    instance_id: String,
    started_at_ms: u64,
    running: bool,
    bind: String,
    local_connection_mode: crate::config::LocalConnectionMode,
    outbound_proxy_mode: crate::config::OutboundProxyMode,
    state_path: String,
    feishu_ws: FeishuWsState,
    telegram: TelegramState,
    wechat: WechatState,
    im_accounts: Vec<ImAccountRuntimeState>,
}

async fn status(State(state): State<SharedState>) -> Json<StatusResponse> {
    Json(status_snapshot(&state).await)
}

async fn status_snapshot(state: &SharedState) -> StatusResponse {
    let running = state
        .bridge_task
        .lock()
        .await
        .as_ref()
        .map(|handle| !handle.is_finished())
        .unwrap_or(false);
    let config = state.config.lock().await;
    let feishu_ws = state.feishu_ws.lock().await.clone();
    let telegram = state.telegram.lock().await.clone();
    let wechat = state.wechat.lock().await.clone();
    let im_accounts = state
        .im_accounts
        .lock()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    StatusResponse {
        service: state.daemon_identity.service.clone(),
        pid: state.daemon_identity.pid,
        instance_id: state.daemon_identity.instance_id.clone(),
        started_at_ms: state.daemon_identity.started_at_ms,
        running,
        bind: config.bind.clone(),
        local_connection_mode: config.local_connection_mode,
        outbound_proxy_mode: config.outbound_proxy.mode,
        state_path: config.state_path.to_string_lossy().to_string(),
        feishu_ws,
        telegram,
        wechat,
        im_accounts,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuiDashboardResponse {
    status: StatusResponse,
    remote: remote_control_backend::RemoteControlStatusResponse,
    codex_app: codex_app_config::CodexAppConfigStatus,
    im_accounts: im_api::ImAccountsResponse,
    ai_gateway: crate::ai_gateway::config::AiGatewayConfig,
}

async fn gui_dashboard(State(state): State<SharedState>) -> Json<GuiDashboardResponse> {
    let status = status_snapshot(&state).await;
    let remote = remote_control_backend::status_snapshot(&state).await;
    let codex_app = codex_app::codex_app_status_snapshot(&state).await;
    let im_accounts = im_api::im_accounts_snapshot(&state).await;
    let ai_gateway = state.config.lock().await.ai_gateway.clone();
    Json(GuiDashboardResponse {
        status,
        remote,
        codex_app,
        im_accounts,
        ai_gateway,
    })
}

async fn shutdown(State(state): State<SharedState>) -> impl IntoResponse {
    state
        .push_event("warn", "shutdown_requested", "daemon shutdown requested")
        .await;
    im_api::stop_bridge_task(&state).await;
    let accepted = state.request_shutdown().await;
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "accepted": accepted })),
    )
}

async fn get_config(State(state): State<SharedState>) -> Json<AppConfig> {
    let mut current = state.config.lock().await;
    if let Ok(mut latest) = AppConfig::load_or_default(&state.config_path) {
        crate::normalize_config_paths(&mut latest, &state.config_path);
        *current = latest;
    }
    Json(current.clone())
}

async fn save_config(
    State(state): State<SharedState>,
    Json(mut config): Json<AppConfig>,
) -> impl IntoResponse {
    let mut current = state.config.lock().await;
    if config.revision.is_none() {
        return (
            StatusCode::CONFLICT,
            Json(
                json!({"error":"配置缺少版本，请刷新后重试 / Reload configuration before saving"}),
            ),
        );
    }
    if let Err(err) = crate::outbound_http::validate_for_local_port(
        &config.outbound_proxy,
        config.local_listen_port(),
    ) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        );
    }
    if let Err(err) = config.save(&state.config_path) {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": err.to_string() })),
        );
    }
    if let Err(err) = crate::outbound_http::init(&config.outbound_proxy, config.local_listen_port())
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err.to_string() })),
        );
    }
    *current = config;
    drop(current);
    state
        .push_event("info", "config_saved", "configuration saved")
        .await;
    (StatusCode::OK, Json(json!({ "ok": true })))
}

async fn commit_external_import(
    State(state): State<SharedState>,
    Json(request): Json<crate::external_import::CommitImport>,
) -> impl IntoResponse {
    let mut current = state.config.lock().await;
    let outcome = (|| -> Result<AppConfig, String> {
        crate::external_import::validate_draft(&request.draft)?;
        let mut config = AppConfig::load_or_default(&state.config_path)
            .map_err(|_| "无法读取当前配置 / Cannot read current configuration")?;
        crate::external_import::merge_import(&mut config, &request)?;
        crate::normalize_config_paths(&mut config, &state.config_path);
        config.save(&state.config_path).map_err(
            |_| "配置已变化或保存失败，请刷新后重试 / Configuration changed or could not be saved",
        )?;
        Ok(config)
    })();
    match outcome {
        Ok(config) => {
            *current = config;
            (StatusCode::OK, Json(json!({"ok":true})))
        }
        Err(error) => (StatusCode::CONFLICT, Json(json!({"error":error}))),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmClawConfigQuery {
    entry_id: Option<String>,
}

async fn gmclaw_config(
    State(state): State<SharedState>,
    Query(query): Query<GmClawConfigQuery>,
) -> axum::response::Response {
    let mut current = state.config.lock().await;
    let path = state.config_path.clone();
    // Use the same latest on-disk provider as save/restore so a refresh also
    // resolves conflicts caused by edits outside this running Hub instance.
    match tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let mut config = AppConfig::load_or_default(&path)?;
        crate::normalize_config_paths(&mut config, &path);
        let status = crate::gmclaw_config::load_selected(&config, query.entry_id.as_deref())?;
        Ok((status, config))
    })
    .await
    {
        Ok(Ok((status, config))) => {
            *current = config;
            Json(status).into_response()
        }
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": error.to_string()})),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "无法读取天工 Claw 配置 / Cannot read GMClaw configuration"})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
struct GmClawRestoreRequest {
    revision: String,
}

enum GmClawMutation {
    Save(crate::gmclaw_config::GmClawSaveRequest),
    Restore(String),
    Activate(crate::gmclaw_config::GmClawEntryRequest),
    Delete(crate::gmclaw_config::GmClawEntryRequest),
}

async fn activate_gmclaw_config(
    State(state): State<SharedState>,
    Json(request): Json<crate::gmclaw_config::GmClawEntryRequest>,
) -> axum::response::Response {
    mutate_gmclaw_config(state, GmClawMutation::Activate(request)).await
}

async fn delete_gmclaw_config(
    State(state): State<SharedState>,
    Json(request): Json<crate::gmclaw_config::GmClawEntryRequest>,
) -> axum::response::Response {
    mutate_gmclaw_config(state, GmClawMutation::Delete(request)).await
}

async fn save_gmclaw_config(
    State(state): State<SharedState>,
    Json(request): Json<crate::gmclaw_config::GmClawSaveRequest>,
) -> axum::response::Response {
    mutate_gmclaw_config(state, GmClawMutation::Save(request)).await
}

async fn restore_gmclaw_config(
    State(state): State<SharedState>,
    Json(request): Json<GmClawRestoreRequest>,
) -> axum::response::Response {
    mutate_gmclaw_config(state, GmClawMutation::Restore(request.revision)).await
}

async fn mutate_gmclaw_config(
    state: SharedState,
    mutation: GmClawMutation,
) -> axum::response::Response {
    // Serialize with ordinary configuration saves while keeping SQLite and file
    // I/O off Tokio's worker threads. Re-read the latest disk revision inside it.
    let mut current = state.config.lock().await;
    let path = state.config_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let outcome = (|| -> anyhow::Result<_> {
            let mut config = AppConfig::load_or_default(&path)?;
            crate::normalize_config_paths(&mut config, &path);
            match mutation {
                GmClawMutation::Save(request) => {
                    crate::gmclaw_config::save(&request, &mut config, &path)
                }
                GmClawMutation::Restore(revision) => {
                    crate::gmclaw_config::restore_backup(&revision, &mut config, &path)
                }
                GmClawMutation::Activate(request) => {
                    crate::gmclaw_config::activate(&request, &mut config, &path)
                }
                GmClawMutation::Delete(request) => {
                    crate::gmclaw_config::delete(&request, &mut config, &path)
                }
            }
        })();
        // Include rollback results in live state even when the mutation failed.
        let latest = AppConfig::load_or_default(&path).map(|mut config| {
            crate::normalize_config_paths(&mut config, &path);
            config
        });
        (outcome, latest)
    })
    .await;
    match result {
        Ok((outcome, latest)) => {
            if let Ok(config) = latest {
                *current = config;
            }
            drop(current);
            match outcome {
                Ok(status) => {
                    state.push_event("info", "gmclaw_config_updated", "GMClaw model configuration updated").await;
                    Json(status).into_response()
                }
                Err(error) => (
                    StatusCode::CONFLICT,
                    Json(json!({"error": error.to_string()})),
                ).into_response(),
            }
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "天工 Claw 配置操作失败，请刷新状态 / GMClaw configuration operation failed; refresh its status"})),
        ).into_response(),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkBuddyConfigQuery {
    entry_id: Option<String>,
}

async fn workbuddy_config(
    State(state): State<SharedState>,
    Query(query): Query<WorkBuddyConfigQuery>,
) -> axum::response::Response {
    let mut current = state.config.lock().await;
    let path = state.config_path.clone();
    match tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let mut config = AppConfig::load_or_default(&path)?;
        crate::normalize_config_paths(&mut config, &path);
        let status = crate::workbuddy_config::load_selected(&config, query.entry_id.as_deref())?;
        Ok((status, config))
    })
    .await
    {
        Ok(Ok((status, config))) => {
            *current = config;
            Json(status).into_response()
        }
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"无法读取 WorkBuddy 配置 / Cannot read WorkBuddy configuration"})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
struct WorkBuddyRestoreRequest {
    revision: String,
}

enum WorkBuddyMutation {
    Save(crate::workbuddy_config::WorkBuddySaveRequest),
    Delete(crate::workbuddy_config::WorkBuddyEntryRequest),
    Restore(String),
}

async fn save_workbuddy_config(
    State(state): State<SharedState>,
    Json(request): Json<crate::workbuddy_config::WorkBuddySaveRequest>,
) -> axum::response::Response {
    mutate_workbuddy_config(state, WorkBuddyMutation::Save(request)).await
}

async fn delete_workbuddy_config(
    State(state): State<SharedState>,
    Json(request): Json<crate::workbuddy_config::WorkBuddyEntryRequest>,
) -> axum::response::Response {
    mutate_workbuddy_config(state, WorkBuddyMutation::Delete(request)).await
}

async fn restore_workbuddy_config(
    State(state): State<SharedState>,
    Json(request): Json<WorkBuddyRestoreRequest>,
) -> axum::response::Response {
    mutate_workbuddy_config(state, WorkBuddyMutation::Restore(request.revision)).await
}

async fn mutate_workbuddy_config(
    state: SharedState,
    mutation: WorkBuddyMutation,
) -> axum::response::Response {
    let mut current = state.config.lock().await;
    let path = state.config_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let outcome = (|| -> anyhow::Result<_> {
            let mut config = AppConfig::load_or_default(&path)?;
            crate::normalize_config_paths(&mut config, &path);
            match mutation {
                WorkBuddyMutation::Save(request) => {
                    crate::workbuddy_config::save(&request, &mut config, &path)
                }
                WorkBuddyMutation::Delete(request) => {
                    crate::workbuddy_config::delete(&request, &mut config, &path)
                }
                WorkBuddyMutation::Restore(revision) => {
                    crate::workbuddy_config::restore_backup(&revision, &mut config, &path)
                }
            }
        })();
        let latest = AppConfig::load_or_default(&path).map(|mut config| {
            crate::normalize_config_paths(&mut config, &path);
            config
        });
        (outcome, latest)
    })
    .await;
    match result {
        Ok((outcome, latest)) => {
            if let Ok(config) = latest { *current = config; }
            drop(current);
            match outcome {
                Ok(status) => {
                    state.push_event("info", "workbuddy_config_updated", "WorkBuddy model configuration updated").await;
                    Json(status).into_response()
                }
                Err(error) => (StatusCode::CONFLICT, Json(json!({"error":error.to_string()}))).into_response(),
            }
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"WorkBuddy 配置操作失败，请刷新状态 / WorkBuddy configuration operation failed; refresh its status"}))).into_response(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteControlBackendStatusResponse {
    available: bool,
    enabled: bool,
    remote_control_base_url: String,
    remote_control_connected: bool,
    remote_control_initialized: bool,
    server_name: Option<String>,
    environment_id: Option<String>,
    installation_id: Option<String>,
    current_thread_id: Option<String>,
    feishu_configured: bool,
    telegram_configured: bool,
    wechat_configured: bool,
    wecom_configured: bool,
    reason: Option<String>,
}

async fn remote_control_backend_status(
    State(state): State<SharedState>,
) -> Json<RemoteControlBackendStatusResponse> {
    let config = state.config.lock().await.clone();
    let remote = remote_control_backend::status_snapshot(&state).await;
    let feishu_configured = im_api::feishu_configured(&config);
    let telegram_configured = im_api::telegram_configured(&config);
    let wechat_configured = im_api::wechat_configured(&config);
    let wecom_configured = im_api::wecom_configured(&config);
    let im_configured = im_api::im_bridge_configured(&config);
    let reason = if !config.bridge.enabled {
        Some("bridge disabled".to_string())
    } else if !im_configured {
        Some("No enabled IM channel is configured".to_string())
    } else {
        None
    };
    Json(RemoteControlBackendStatusResponse {
        available: config.bridge.enabled && im_configured,
        enabled: config.bridge.enabled,
        remote_control_base_url: config.remote_control_base_url(),
        remote_control_connected: remote.connected,
        remote_control_initialized: remote.initialized,
        server_name: remote.server_name,
        environment_id: remote.environment_id,
        installation_id: remote.installation_id,
        current_thread_id: remote.current_thread_id,
        feishu_configured,
        telegram_configured,
        wechat_configured,
        wecom_configured,
        reason,
    })
}

async fn events(State(state): State<SharedState>) -> impl IntoResponse {
    let events = state.events.lock().await.clone();
    Json(events)
}
