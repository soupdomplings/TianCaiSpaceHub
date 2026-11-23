use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    ai_gateway::catalog::configured_models_response_with_etag,
    app_state::SharedState,
    codex_app_config::{self, ConfigureCodexAppOptions},
    codex_app_enhanced, codex_session_history,
    config::LocalConnectionMode,
    remote_control_backend,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConfigureCodexAppRequest {
    codex_home: Option<String>,
    connection_mode: Option<LocalConnectionMode>,
    provider_name: Option<String>,
    provider_base_url: Option<String>,
    provider_key: Option<String>,
    activate: Option<bool>,
    #[allow(dead_code)]
    image_generation_enabled: Option<bool>,
    supports_websockets: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeleteCodexAppProviderRequest {
    provider_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SetCodexAppProviderWebSocketRequest {
    provider_name: String,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MoveCodexAppSessionProviderRequest {
    thread_id: String,
    rollout_path: Option<String>,
    target_provider: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexAppSessionsResponse {
    ok: bool,
    threads: Vec<serde_json::Value>,
    providers: Vec<String>,
    total: usize,
}

pub(super) async fn configure_codex_app(
    State(state): State<SharedState>,
    payload: Option<Json<ConfigureCodexAppRequest>>,
) -> impl IntoResponse {
    let request = payload.map(|Json(value)| value);
    let config = state.config.lock().await.clone();
    let codex_home = request
        .as_ref()
        .and_then(|value| value.codex_home.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from);
    let provider_base_url = request
        .as_ref()
        .and_then(|value| value.provider_base_url.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let provider_name = request
        .as_ref()
        .and_then(|value| value.provider_name.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let provider_key = request
        .as_ref()
        .and_then(|value| value.provider_key.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let activate_provider = request
        .as_ref()
        .and_then(|value| value.activate)
        .unwrap_or(true);
    let provider_supports_websockets = request.as_ref().and_then(|value| value.supports_websockets);
    let connection_mode = request
        .as_ref()
        .and_then(|value| value.connection_mode)
        .unwrap_or(config.local_connection_mode);

    let backend_url = config.remote_control_base_url();
    state
        .push_event(
            "info",
            "codex_app_configure_start",
            format!(
                "provider={} activate_provider={}",
                provider_name.as_deref().unwrap_or_default(),
                activate_provider
            ),
        )
        .await;
    match codex_app_config::configure_codex_app(ConfigureCodexAppOptions {
        codex_home,
        backend_url: backend_url.clone(),
        connection_mode,
        account_id: "acct_codexhub_local".to_string(),
        user_id: "user_codexhub_local".to_string(),
        email: "codexhub-local@example.local".to_string(),
        plan_type: "pro".to_string(),
        provider_name,
        provider_base_url,
        provider_key,
        activate_provider,
        image_generation_enabled: None,
        provider_supports_websockets,
    }) {
        Ok(report) => {
            let gui_api_base = codex_app_config::inspect_gui_api_base_url(&backend_url);
            let remote_control_switch = report.remote_control_switch.clone();
            state
                .push_event(
                    "info",
                    "codex_app_configured",
                    format!(
                        "codex_home={} config={} auth={} gui_api_base={} remote_control_switch={}",
                        report.codex_home.display(),
                        report.config_path.display(),
                        report.auth_path.display(),
                        gui_api_base.value.as_deref().unwrap_or_default(),
                        remote_control_switch.configured
                    ),
                )
                .await;
            (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "codexHome": report.codex_home.to_string_lossy().to_string(),
                    "configPath": report.config_path.to_string_lossy().to_string(),
                    "authPath": report.auth_path.to_string_lossy().to_string(),
                    "backendUrl": report.backend_url,
                    "guiApiBase": gui_api_base,
                    "projectHistory": report.project_history,
                    "projectSidebar": report.project_sidebar,
                    "remoteControlSwitch": remote_control_switch,
                })),
            )
        }
        Err(err) => {
            state
                .push_event("error", "codex_app_configure_failed", err.to_string())
                .await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": err.to_string() })),
            )
        }
    }
}

pub(super) async fn set_codex_app_provider_websocket(
    State(state): State<SharedState>,
    Json(request): Json<SetCodexAppProviderWebSocketRequest>,
) -> impl IntoResponse {
    let provider_name = request.provider_name.trim();
    if provider_name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "provider_name is required" })),
        );
    }

    let config = state.config.lock().await.clone();
    let backend_url = config.remote_control_base_url();
    match codex_app_config::set_codex_app_provider_websocket(None, provider_name, request.enabled) {
        Ok(config_path) => {
            let status =
                codex_app_config::inspect_codex_app_config_for_mode(None, &backend_url, true);
            state
                .push_event(
                    "info",
                    "codex_app_provider_websocket_set",
                    format!(
                        "config={} provider={} supports_websockets={}",
                        config_path.display(),
                        provider_name,
                        request.enabled
                    ),
                )
                .await;
            (
                StatusCode::OK,
                Json(
                    json!({ "ok": true, "configPath": config_path.to_string_lossy().to_string(), "status": status }),
                ),
            )
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": err.to_string() })),
        ),
    }
}

pub(super) async fn delete_codex_app_provider(
    State(state): State<SharedState>,
    Json(request): Json<DeleteCodexAppProviderRequest>,
) -> impl IntoResponse {
    let config = state.config.lock().await.clone();
    let backend_url = config.remote_control_base_url();
    match codex_app_config::delete_codex_app_provider(None, request.provider_name.trim()) {
        Ok(config_path) => {
            let status =
                codex_app_config::inspect_codex_app_config_for_mode(None, &backend_url, true);
            state
                .push_event(
                    "info",
                    "codex_app_provider_deleted",
                    format!(
                        "config={} provider={}",
                        config_path.display(),
                        request.provider_name.trim()
                    ),
                )
                .await;
            (
                StatusCode::OK,
                Json(
                    json!({ "ok": true, "configPath": config_path.to_string_lossy().to_string(), "status": status }),
                ),
            )
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": err.to_string() })),
        ),
    }
}

pub(super) async fn uninstall_codex_app(State(state): State<SharedState>) -> impl IntoResponse {
    let config = state.config.lock().await.clone();
    let backend_url = config.remote_control_base_url();
    match codex_app_config::uninstall_codex_app(None, &backend_url) {
        Ok(report) => {
            state
                .push_event(
                    "info",
                    "codex_app_uninstalled",
                    format!(
                        "codex_home={} config={} auth={} removed_chatgpt_base_url={} removed_model_provider={} removed_auth={} gui_api_base={}",
                        report.codex_home.display(),
                        report.config_path.display(),
                        report.auth_path.display(),
                        report.removed_chatgpt_base_url,
                        report.removed_model_provider,
                        report.removed_auth,
                        report.gui_api_base.value.as_deref().unwrap_or_default()
                    ),
                )
                .await;
            (
                StatusCode::OK,
                Json(json!({ "ok": true, "report": report })),
            )
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": err.to_string() })),
        ),
    }
}

pub(super) async fn refresh_codex_app_models(
    State(state): State<SharedState>,
) -> impl IntoResponse {
    refresh_models_in_home(&state, None).await
}

async fn refresh_models_in_home(
    state: &SharedState,
    home: Option<&std::path::Path>,
) -> Json<serde_json::Value> {
    let skipped = |cache_removed: bool, reason: &str, error: &str| {
        Json(
            json!({ "ok": true, "cacheRemoved": cache_removed, "modelListRefreshed": false,
            "reason": reason, "error": error }),
        )
    };
    let (backend_url, selected) = {
        let config = state.config.lock().await;
        (
            config.remote_control_base_url(),
            config.ai_gateway.codex_visible_models.clone(),
        )
    };
    use codex_app_config::CatalogConnection;
    match codex_app_config::prepare_model_catalog(home, &backend_url) {
        Ok(CatalogConnection::CcSwitch) => {
            let message = "当前 Codex 仍使用 CC Switch 模型目录。请到接入设置更新 Hub 配置。";
            state
                .push_event("warn", "codex_app_models_cc_switch_catalog", message)
                .await;
            return skipped(false, "ccSwitchCatalog", message);
        }
        Ok(CatalogConnection::NeedsSetup) => return skipped(false, "needsSetup", ""),
        Ok(CatalogConnection::CustomCatalog) => return skipped(false, "customCatalog", ""),
        Err(err) => return skipped(false, "catalogConfigFailed", &err.to_string()),
        Ok(CatalogConnection::Ready) => {}
    }
    let cache_removed =
        match codex_app_config::clear_codex_models_cache(home.map(ToOwned::to_owned)) {
            Ok(removed) => removed,
            Err(err) => {
                state
                    .push_event(
                        "warn",
                        "codex_app_models_cache_clear_failed",
                        err.to_string(),
                    )
                    .await;
                return skipped(false, "cacheClearFailed", &err.to_string());
            }
        };

    if !state.remote_control.inner.lock().await.connected {
        return skipped(
            cache_removed,
            "disconnected",
            "No Codex runtime is connected.",
        );
    }
    let model_list_result = remote_control_backend::model_list_for_client(
        state,
        remote_control_backend::default_remote_client_key(),
        true,
        Some(200),
    )
    .await;

    match model_list_result {
        Ok(value) => {
            let Some(models) = value.get("data").and_then(|value| value.as_array()) else {
                return skipped(
                    cache_removed,
                    "invalidModelList",
                    "Codex did not return a model list.",
                );
            };
            let count = models.len();
            let missing = match check_runtime_selection(&selected, models, &value["nextCursor"]) {
                RuntimeSelection::Incomplete => return skipped(cache_removed, "incompleteModelList", ""),
                RuntimeSelection::Missing(id) => Some(id),
                RuntimeSelection::Confirmed => None,
            };
            state
                .push_event(
                    "info",
                    "codex_app_models_refreshed",
                    format!("cache_removed={cache_removed} count={count}"),
                )
                .await;
            Json(
                json!({ "ok": true, "cacheRemoved": cache_removed, "modelListRefreshed": true,
                "count": count, "missingModel": missing }),
            )
        }
        Err(err) => {
            state
                .push_event(
                    "warn",
                    "codex_app_models_refresh_skipped",
                    format!("cache_removed={cache_removed} err={err}"),
                )
                .await;
            skipped(cache_removed, "queryFailed", &err.to_string())
        }
    }
}

// Compare only a complete, identifiable page. Include both protocol ID forms;
// neither the runtime nor Hub can observe a desktop picker's rendered contents here.
#[derive(Debug, PartialEq, Eq)]
enum RuntimeSelection<'a> {
    Confirmed,
    Missing(&'a str),
    Incomplete,
}

fn check_runtime_selection<'a>(
    selected: &'a [String],
    models: &[serde_json::Value],
    cursor: &serde_json::Value,
) -> RuntimeSelection<'a> {
    use std::collections::HashSet;
    if !cursor.is_null() {
        return RuntimeSelection::Incomplete;
    }
    let mut known = HashSet::new();
    for row in models {
        let mut ids = ["id", "model"]
            .into_iter()
            .filter_map(|key| row[key].as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .peekable();
        if ids.peek().is_none() {
            return RuntimeSelection::Incomplete;
        }
        known.extend(ids.map(str::to_ascii_lowercase));
    }
    selected
        .iter()
        .find(|id| !known.contains(&id.to_ascii_lowercase()))
        .map_or(RuntimeSelection::Confirmed, |id| RuntimeSelection::Missing(id))
}

#[cfg(test)]
mod refresh_tests {
    use super::*;

    #[test]
    fn runtime_missing_models_require_a_complete_identifiable_page() {
        use RuntimeSelection::*;
        let selected = vec!["first".into(), "second".into()];
        let rows = [json!({"id":"opaque", "model":"FIRST"})];
        assert_eq!(
            check_runtime_selection(&selected, &rows, &json!(null)), Missing("second")
        );
        assert_eq!(check_runtime_selection(&selected, &rows, &json!("next")), Incomplete);
        assert_eq!(
            check_runtime_selection(
                &selected,
                &[rows[0].clone(), json!({"displayName":"Second"})],
                &json!(null)
            ),
            Incomplete
        );
        assert_eq!(
            check_runtime_selection(
                &selected,
                &[rows[0].clone(), json!({"id":"second"})],
                &json!(null)
            ),
            Confirmed
        );
        assert_eq!(check_runtime_selection(&selected, &[], &json!(null)), Missing("first"));
        assert_eq!(check_runtime_selection(&[], &[], &json!(null)), Confirmed);
    }

    #[tokio::test]
    async fn cc_switch_refresh_requires_explicit_setup_and_leaves_third_party_files_intact() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let config = crate::config::AppConfig {
            bind: "127.0.0.1:3847".into(),
            state_path: home.join("state.json"),
            ..Default::default()
        };
        let state = crate::app_state::AppState::new(home.join("hub.toml"), config, None, None);
        let raw = r#"model_provider = "ai-gateway"
model_catalog_json = "cc-switch-model-catalog.json"
[model_providers.ai-gateway]
name = "ai-gateway"
base_url = "http://127.0.0.1:3847/ai-gateway/v1"
wire_api = "responses"
requires_openai_auth = false
http_headers = { x-openai-actor-authorization = "codexhub-local" }
"#;
        let config_file = home.join("config.toml");
        let catalog = home.join("cc-switch-model-catalog.json");
        let cache = home.join("models_cache.json");
        std::fs::write(&config_file, raw).unwrap();
        std::fs::write(&catalog, "external list").unwrap();
        std::fs::write(&cache, "old cache").unwrap();
        let Json(result) = refresh_models_in_home(&state, Some(home)).await;
        assert_eq!(result["reason"], "ccSwitchCatalog");
        assert_eq!(result["modelListRefreshed"], false);
        assert_eq!(std::fs::read_to_string(&config_file).unwrap(), raw);
        assert!(cache.exists());

        std::fs::write(
            &config_file,
            raw.lines()
                .filter(|line| !line.starts_with("model_catalog_json"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let Json(result) = refresh_models_in_home(&state, Some(home)).await;
        assert_eq!(result["reason"], "disconnected");
        assert_eq!(result["cacheRemoved"], true);
        assert!(!cache.exists());
        assert_eq!(std::fs::read_to_string(catalog).unwrap(), "external list");
    }

    #[tokio::test]
    async fn saved_catalog_retries_failed_client_config_and_serves_selection_from_disk() {
        use axum::{body::{Body, to_bytes}, http::Request};
        use tower::ServiceExt;
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("client");
        std::fs::create_dir(&home).unwrap();
        let hub_path = fixture.path().join("hub.toml");
        let mut config = crate::config::AppConfig {
            bind: "127.0.0.1:3847".into(), state_path: fixture.path().join("state.json"),
            ..Default::default()
        };
        config.ai_gateway.enabled = true;
        config.ai_gateway.codex_visible_models = vec!["gpt-6.2-astra".into(), "gpt-6.2-sol".into()];
        config.save(&hub_path).unwrap();
        let state = crate::app_state::AppState::new(
            hub_path.clone(), crate::config::AppConfig::load_or_default(&hub_path).unwrap(), None, None
        );
        let client_file = home.join("config.toml");
        let backup = home.join("config.toml.bak");
        let original = r#"model_provider = "ai-gateway"
model = "keep-my-default"
[model_providers.ai-gateway]
name = "ai-gateway"
base_url = "http://127.0.0.1:3847/ai-gateway/v1"
wire_api = "responses"
requires_openai_auth = false
http_headers = { x-openai-actor-authorization = "codexhub-local" }
"#;
        std::fs::write(&client_file, original).unwrap();
        std::fs::create_dir(&backup).unwrap(); // Make the existing atomic backup writer fail.
        let Json(failed) = refresh_models_in_home(&state, Some(&home)).await;
        assert_eq!(failed["reason"], "catalogConfigFailed");
        assert_eq!(failed["modelListRefreshed"], false);
        assert_eq!(std::fs::read_to_string(&client_file).unwrap(), original);
        std::fs::remove_dir(&backup).unwrap();
        let Json(retried) = refresh_models_in_home(&state, Some(&home)).await;
        assert_eq!(retried["reason"], "disconnected");
        let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&client_file).unwrap()).unwrap();
        assert_eq!(saved["model"].as_str(), Some("keep-my-default"));
        assert_eq!(saved["features"]["api_key_model_discovery"].as_bool(), Some(true));
        let url = saved["model_providers"]["ai-gateway"]["model_catalog_url"].as_str().unwrap();
        let path = url.strip_prefix("http://127.0.0.1:3847").unwrap();
        let app = axum::Router::new().nest("/ai-gateway", crate::ai_gateway::router()).with_state(state);
        let response = app.oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert!(response.status().is_success());
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let catalog: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let ids: Vec<_> = catalog["models"].as_array().unwrap().iter().map(|m| m["slug"].as_str().unwrap()).collect();
        assert_eq!(ids, ["gpt-6.2-astra", "gpt-6.2-sol"]);
    }
}

pub(super) async fn launch_codex_app_enhanced(
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let (models, backend_url) = {
        let config = state.config.lock().await;
        let (response, _) = configured_models_response_with_etag(&config.ai_gateway);
        let models = response["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|model| model.get("slug").and_then(serde_json::Value::as_str))
            .map(str::to_string)
            .collect::<Vec<_>>();
        (models, config.remote_control_base_url())
    };
    state
        .push_event(
            "info",
            "codex_app_enhanced_launch_start",
            format!("models={}", models.len()),
        )
        .await;
    match codex_app_enhanced::launch_and_inject(models, &backend_url).await {
        Ok(report) => {
            state
                .push_event(
                    "info",
                    "codex_app_enhanced_launch_ready",
                    format!(
                        "launched={} port={} models={} gates={} i18n={}",
                        report.launched,
                        report.port,
                        report.available_models.len(),
                        report.key_gates_enabled,
                        report.i18n_enabled
                    ),
                )
                .await;
            (
                StatusCode::OK,
                Json(json!({ "ok": true, "report": report })),
            )
        }
        Err(err) => {
            state
                .push_event("error", "codex_app_enhanced_launch_failed", err.to_string())
                .await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "ok": false, "error": err.to_string() })),
            )
        }
    }
}

pub(super) async fn codex_app_enhanced_preflight() -> impl IntoResponse {
    match codex_app_enhanced::preflight().await {
        Ok(status) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "status": status })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": err.to_string() })),
        ),
    }
}

pub(super) async fn codex_app_sessions(State(state): State<SharedState>) -> impl IntoResponse {
    const PAGE_LIMIT: u32 = 100;
    const MAX_PAGES: usize = 20;
    let threads = match remote_control_backend::session_history_threads(
        &state,
        remote_control_backend::default_remote_client_key(),
        PAGE_LIMIT,
        MAX_PAGES,
        false,
    )
    .await
    {
        Ok(threads) => threads,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "ok": false, "error": err.to_string() })),
            );
        }
    };

    let mut providers = threads
        .iter()
        .filter_map(|thread| thread.get("modelProvider").and_then(|value| value.as_str()))
        .map(str::to_string)
        .collect::<Vec<_>>();
    providers.sort();
    providers.dedup();

    (
        StatusCode::OK,
        Json(json!(CodexAppSessionsResponse {
            ok: true,
            total: threads.len(),
            threads,
            providers,
        })),
    )
}

pub(super) async fn move_codex_app_session_provider(
    Json(request): Json<MoveCodexAppSessionProviderRequest>,
) -> impl IntoResponse {
    let rollout_path = request
        .rollout_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from);
    let target_provider = request
        .target_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let result = match target_provider {
        Some(provider) => codex_session_history::move_thread_to_provider(
            None,
            request.thread_id.as_str(),
            rollout_path,
            provider,
        ),
        None => codex_session_history::move_thread_to_ai_gateway(
            None,
            request.thread_id.as_str(),
            rollout_path,
        ),
    };
    match result {
        Ok(report) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "report": report })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": err.to_string() })),
        ),
    }
}

pub(super) async fn repair_codex_app_gui_environment(
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let config = state.config.lock().await.clone();
    let backend_url = config.remote_control_base_url();
    let status = codex_app_config::inspect_codex_app_config_for_mode(None, &backend_url, true);
    if !status.config_ok || !status.auth_ok {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "Codex App local config is not ready; write config first",
                "status": status,
            })),
        );
    }

    let remote_control_switch =
        match codex_app_config::enable_codex_app_remote_control_switch_for_backend(
            None,
            &backend_url,
        ) {
            Ok(status) => status,
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "ok": false,
                        "error": err.to_string(),
                        "status": status,
                    })),
                );
            }
        };
    let gui_api_base = codex_app_config::configure_gui_environment(&backend_url, true);
    state
        .push_event(
            "info",
            "codex_app_gui_environment_configured",
            format!(
                "gui_api_base={} login_issuer={} remote_control_switch={}",
                gui_api_base.value.as_deref().unwrap_or_default(),
                gui_api_base
                    .login_issuer_value
                    .as_deref()
                    .unwrap_or_default(),
                remote_control_switch.configured
            ),
        )
        .await;
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "guiApiBase": gui_api_base,
            "remoteControlSwitch": remote_control_switch,
        })),
    )
}

pub(super) async fn codex_app_status(
    State(state): State<SharedState>,
) -> Json<codex_app_config::CodexAppConfigStatus> {
    Json(codex_app_status_snapshot(&state).await)
}

pub(super) async fn codex_app_status_snapshot(
    state: &SharedState,
) -> codex_app_config::CodexAppConfigStatus {
    let config = state.config.lock().await.clone();
    codex_app_config::inspect_codex_app_config_for_mode(
        None,
        &config.remote_control_base_url(),
        true,
    )
}
