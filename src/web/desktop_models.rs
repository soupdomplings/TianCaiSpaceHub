use super::desktop::{response, trusted};
use crate::{
    ai_gateway::config::{ProviderConfig, provider_route_id},
    app_state::SharedState,
    config::AppConfig,
    desktop::{
        model_operations,
        models::{self, Binding},
        resources::Library,
    },
};
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    response::Response,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
pub(super) fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/desktop/models", get(list))
        .route("/api/desktop/models/use", post(apply))
        .route("/api/desktop/models/copy", post(copy_legacy))
        .route("/api/desktop/models/read", post(read))
        .route("/api/desktop/models/restore", post(restore))
        .route(
            "/api/desktop/tiangong-models/read",
            post(read_tiangong_builtins),
        )
        .route(
            "/api/desktop/tiangong-models/enable",
            post(enable_tiangong_builtins),
        )
        .route(
            "/api/desktop/tiangong-models/connect",
            post(connect_tiangong_builtins),
        )
        .route(
            "/api/desktop/tiangong-models/share",
            post(share_tiangong_builtin),
        )
        .route(
            "/api/desktop/tiangong-models/test-identity",
            post(test_tiangong_identity),
        )
}
async fn test_tiangong_identity(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<crate::desktop::tiangong_models::IdentityTestRequest>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let path = state.config_path.clone();
    response(
        tokio::task::spawn_blocking(move || {
            crate::desktop::tiangong_models::test_identity(&path, request)
        })
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("测试中断，未自动重发"))),
    )
}
async fn read_tiangong_builtins(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let config = state.config.lock().await.clone();
    let path = state.config_path.clone();
    response(
        tokio::task::spawn_blocking(move || {
            crate::desktop::tiangong_models::account_view(&path, &config)
        })
        .await
        .map_err(|_| anyhow::anyhow!("读取中断，请重新读取天工内置模型")),
    )
}
async fn connect_tiangong_builtins(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<crate::desktop::tiangong_models::ConnectRequest>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let path = state.config_path.clone();
    // Network login does not hold the live config lock. The final save checks
    // the original revision and uses the same atomic configuration writer.
    let outcome = tokio::task::spawn_blocking(move || {
        crate::desktop::tiangong_models::connect(&path, request)
    })
    .await;
    let result =
        outcome.unwrap_or_else(|_| Err(anyhow::anyhow!("天工登录中断，请重新读取状态后重试")));
    if result.is_ok() {
        let mut config = state.config.lock().await;
        if let Ok(latest) = AppConfig::load_or_default(&state.config_path) {
            *config = latest;
        }
    }
    response(result)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShareBuiltin {
    revision: String,
    model_id: String,
    targets: Vec<String>,
}

async fn share_tiangong_builtin(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<ShareBuiltin>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let config = state.config.lock().await.clone();
    if config.revision.as_deref() != Some(request.revision.as_str())
        || request.targets.is_empty()
        || request.targets.len() > 32
    {
        return response::<Value>(Err(anyhow::anyhow!("模型或选择已变化，请重新打开分享页面")));
    }
    let Some(source) = config
        .ai_gateway
        .providers
        .iter()
        .find(|p| {
            p.enabled
                && !p.is_client_reserved()
                && p.desktop_binding.is_none()
                && p.tiangong_builtin
                    .as_ref()
                    .is_some_and(|b| b.model_id == request.model_id)
        })
        .cloned()
    else {
        return response::<Value>(Err(anyhow::anyhow!("请先在天工 Work 的模型页读取内置模型")));
    };
    share_builtin_source(state, headers, request.revision, source, request.targets,
        crate::desktop::clients::ClientId::Tiangong).await
}

/// Both built-in hosts share the same copy/native writers and reuse rules.
pub(super) async fn share_builtin_source(
    state: SharedState,
    headers: HeaderMap,
    initial_revision: String,
    source: ProviderConfig,
    selected_targets: Vec<String>,
    source_client: crate::desktop::clients::ClientId,
) -> Response {
    let config = state.config.lock().await.clone();
    if config.revision.as_deref() != Some(initial_revision.as_str()) || selected_targets.is_empty() || selected_targets.len() > 32 {
        return response::<Value>(Err(anyhow::anyhow!("模型或选择已变化，请重新打开分享页面")));
    }
    let targets = match Library::new(&state.config_path).and_then(|lib| lib.targets(&lib.load()?)) {
        Ok(v) => v,
        Err(e) => return response::<Value>(Err(e)),
    };
    if selected_targets.iter().enumerate().any(|(i, id)| {
        selected_targets[..i].contains(id)
            || !targets.iter().any(|t| {
                t.id == *id
                    && t.client != source_client
                    && !t.models_inherited
            })
    }) {
        return response::<Value>(Err(anyhow::anyhow!("客户端目标已变化，请重新选择")));
    }
    let model = source
        .model_aliases
        .keys()
        .next()
        .cloned()
        .unwrap_or_default();
    let route = provider_route_id(&source);
    let mut revision = initial_revision;
    let mut outcomes = Vec::new();
    for id in selected_targets {
        let target = targets.iter().find(|t| t.id == id).unwrap();
        let latest = state.config.lock().await.clone();
        if latest.revision.as_deref() != Some(revision.as_str()) {
            outcomes.push(json!({"targetId":id,"ok":false,"message":"配置已在其他位置更新，请重新打开分享页面后继续"}));
            break;
        }
        // Reuse the same copy. Codex removal affects its visible catalog;
        // an enabled source alone must never be mistaken for current use.
        let existing = latest
            .ai_gateway
            .providers
            .iter()
            .find(|p| {
                (p.tiangong_builtin.as_ref().is_some_and(|b| source.tiangong_builtin.as_ref().is_some_and(|s| s.model_id == b.model_id))
                    || p.workbuddy_builtin.as_ref().is_some_and(|b| source.workbuddy_builtin.as_ref().is_some_and(|s| s.account_id == b.account_id && s.model_id == b.model_id)))
                    && (p
                        .desktop_binding
                        .as_ref()
                        .is_some_and(|b| b.target_id == id)
                        || (target.client == crate::desktop::clients::ClientId::WorkBuddy
                            && crate::ai_gateway::config::is_workbuddy_provider_name(&p.name))
                        || (target.client == crate::desktop::clients::ClientId::Tiangong && p.is_gmclaw())
                        || (target.client == crate::desktop::clients::ClientId::Codex
                            && p.name.starts_with("Codex · ")))
            })
            .cloned();
        if existing.as_ref().is_some_and(|p| {
            p.enabled
                && (target.client != crate::desktop::clients::ClientId::Codex
                    || p.model_aliases
                        .keys()
                        .any(|m| latest.ai_gateway.codex_visible_models.contains(m)))
        }) {
            outcomes.push(json!({"targetId":id,"ok":true,"message":"已使用，沿用原客户端模型参数"}));
            continue;
        }
        use crate::desktop::clients::ClientId;
        let result = if let Some(existing) = existing {
            if target.client == ClientId::Codex {
                use_existing_codex_copy(&state, &revision, &existing.name).await
            } else if let Some(binding) = existing.desktop_binding {
                // Re-enable the existing copy through its native writer without
                // replacing independently edited model parameters.
                let body = json!({"revision":revision,"targetId":id,"id":binding.id,"using":true});
                apply(
                    State(state.clone()),
                    headers.clone(),
                    Json(serde_json::from_value(body).unwrap()),
                )
                .await
            } else {
                outcomes.push(json!({"targetId":id,"ok":false,"message":format!("{}已有停用的内置模型，请到该客户端模型页启用原条目；独立参数保持不变",target.client.name())}));
                break;
            }
        } else if matches!(target.client, ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong) {
            copy_legacy(
                State(state.clone()),
                headers.clone(),
                Json(CopyLegacy {
                    client: target.client,
                    source_route: route.clone(),
                    model: model.clone(),
                    revision: revision.clone(),
                    target_id: Some(id.clone()),
                    workbuddy: None,
                    tiangong: None,
                }),
            )
            .await
        } else {
            let body = json!({"revision":revision,"targetId":id,"sourceRoute":route,"model":model,"using":true,"label":model});
            match serde_json::from_value(body) {
                Ok(req) => apply(State(state.clone()), headers.clone(), Json(req)).await,
                Err(_) => {
                    outcomes
                        .push(json!({"targetId":id,"ok":false,"message":"客户端模型配置不兼容"}));
                    continue;
                }
            }
        };
        let status = result.status();
        let bytes = axum::body::to_bytes(result.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap_or_default();
        let body: Value = serde_json::from_slice(&bytes).unwrap_or_default();
        let ok = status.is_success() && body.get("error").is_none();
        if let Some(next) = body["revision"].as_str() {
            revision = next.into();
        } else if ok {
            revision = state
                .config
                .lock()
                .await
                .revision
                .clone()
                .unwrap_or(revision);
        }
        outcomes.push(json!({"targetId":id,"ok":ok,"message":if ok {body["status"]["message"].as_str().or_else(||body["message"].as_str()).unwrap_or("已加入客户端").to_string()}else{body["error"].as_str().unwrap_or("分享未完成，请重新读取此客户端后重试").to_string()}}));
        if !ok {
            break;
        }
    }
    response(Ok(json!({"outcomes":outcomes,"revision":revision})))
}

async fn use_existing_codex_copy(state: &SharedState, revision: &str, name: &str) -> Response {
    let mut current = state.config.lock().await;
    let (path, revision, name) = (
        state.config_path.clone(),
        revision.to_string(),
        name.to_string(),
    );
    let outcome = tokio::task::spawn_blocking(move || -> Result<AppConfig> {
        let mut config = AppConfig::load_or_default(&path)?;
        ensure!(
            config.revision.as_deref() == Some(&revision),
            "模型列表已变化，请重新打开分享页面"
        );
        let provider = config
            .ai_gateway
            .providers
            .iter_mut()
            .find(|p| {
                p.name == name && p.name.starts_with("Codex · ") && (p.tiangong_builtin.is_some() || p.workbuddy_builtin.is_some())
            })
            .context("原模型已不存在，请重新读取")?;
        provider.enabled = true;
        let aliases = provider.model_aliases.keys().cloned().collect::<Vec<_>>();
        ensure!(!aliases.is_empty(), "原模型标识已变化，请重新读取");
        for alias in aliases {
            if !config.ai_gateway.codex_visible_models.contains(&alias) {
                config.ai_gateway.codex_visible_models.push(alias);
            }
        }
        config.save(&path)?;
        Ok(config)
    })
    .await;
    match outcome {
        Ok(Ok(config)) => {
            *current = config;
            response(Ok(
                json!({"revision":current.revision,"message":"已重新加入 Codex 模型列表"}),
            ))
        }
        Ok(Err(e)) => response::<Value>(Err(e)),
        Err(_) => response::<Value>(Err(anyhow::anyhow!("恢复模型中断，请重新读取后核对"))),
    }
}
async fn enable_tiangong_builtins(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<crate::desktop::tiangong_models::EnableRequest>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let mut current = state.config.lock().await;
    let path = state.config_path.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let result = crate::desktop::tiangong_models::enable(&path, request);
        (result, AppConfig::load_or_default(&path))
    })
    .await;
    match outcome {
        Ok((result, latest)) => {
            if let Ok(latest) = latest {
                *current = latest;
            }
            response(result)
        }
        Err(_) => response::<Value>(Err(anyhow::anyhow!(
            "操作中断，请重新读取状态，不会自动重复保存"
        ))),
    }
}
async fn list(State(state): State<SharedState>) -> Response {
    let config = state.config.lock().await.clone();
    let path = state.config_path.clone();
    response(
        tokio::task::spawn_blocking(move || -> Result<Value> {
            let mut view = models::public(&config.ai_gateway);
            view["codexVisibleModels"] =
                serde_json::to_value(&config.ai_gateway.codex_visible_models)?;
            view["revision"] = config.revision.clone().into();
            let lib = Library::new(&path)?;
            let targets = lib.targets(&lib.load()?)?;
            if let Some(rows) = view["copies"].as_array_mut() {
                for row in rows {
                    let binding = config
                        .ai_gateway
                        .providers
                        .iter()
                        .filter_map(|p| p.desktop_binding.as_ref())
                        .find(|b| row["id"] == b.id);
                    if let Some(binding) = binding {
                        if let Some(target) = targets.iter().find(|t| t.id == binding.target_id) {
                            let state = if target.client
                                == crate::desktop::clients::ClientId::WebStorm
                                && target.model_mode == "native"
                            {
                                crate::desktop::webstorm_native::using(
                                    binding,
                                    target,
                                    config.local_listen_port().unwrap_or(3210),
                                )
                            } else {
                                models::using(binding, target)
                            };
                            row["using"] = state
                                .as_ref()
                                .ok()
                                .copied()
                                .map(Value::Bool)
                                .unwrap_or(Value::Null);
                            if let Err(error) = state {
                                row["stateError"] = error.to_string().into();
                            }
                        }
                    }
                }
            }
            view["targets"] = serde_json::to_value(targets)?;
            view["operations"] = model_operations::pending(&path)?.into();
            Ok(view)
        })
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("客户端模型状态读取中断，请重新读取"))),
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Change {
    revision: String,
    target_id: String,
    source_route: Option<String>,
    model: Option<String>,
    label: Option<String>,
    id: Option<String>,
    using: bool,
    provider: Option<ProviderConfig>,
    #[serde(default)]
    replace_conflict: bool,
    #[serde(default)]
    sync_source: bool,
    wire_protocol: Option<String>,
    capabilities: Option<Value>,
}
async fn apply(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Change>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let mut current = state.config.lock().await;
    let path = state.config_path.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let result = mutate(&path, request);
        let latest = AppConfig::load_or_default(&path).map(|mut c| {
            crate::normalize_config_paths(&mut c, &path);
            c
        });
        (result, latest)
    })
    .await;
    match outcome {
        Ok((result, latest)) => {
            if let Ok(latest) = latest {
                *current = latest;
            }
            let view = models::public(&current.ai_gateway);
            response(result.map(|(id,undo)|json!({"id":id,"undo":undo,"view":view,"revision":current.revision,"message":"已配置；当前会话尚未读取新模型时，请新建会话或重开客户端"})))
        }
        Err(_) => response::<Value>(Err(anyhow::anyhow!(
            "操作中断，请重新读取配置；不会自动重复保存"
        ))),
    }
}
fn mutate(path: &std::path::PathBuf, request: Change) -> Result<(String, String)> {
    let mut config = AppConfig::load_or_default(path)?;
    ensure!(
        config.revision.as_deref() == Some(&request.revision),
        "模型来源或客户端已变化，请重新读取"
    );
    let before = config.clone();
    let library = Library::new(path)?;
    let target = library
        .targets(&library.load()?)?
        .into_iter()
        .find(|t| t.id == request.target_id)
        .context("客户端配置位置已变化")?;
    let id;
    if let Some(existing_id) = &request.id {
        let index = config
            .ai_gateway
            .providers
            .iter()
            .position(|p| {
                p.desktop_binding
                    .as_ref()
                    .is_some_and(|b| b.id == *existing_id && b.target_id == target.id)
            })
            .context("客户端模型已被删除")?;
        let old = config.ai_gateway.providers[index].clone();
        let binding = old.desktop_binding.as_ref().unwrap();
        if !request.replace_conflict {
            models::verify_later_changes(binding, &target)?;
        }
        id = binding.id.clone();
        if request.using {
            if request.sync_source {
                let source = config
                    .ai_gateway
                    .providers
                    .iter()
                    .find(|p| provider_route_id(p) == binding.source_route)
                    .context("原来源已不存在，请选择新的来源")?
                    .clone();
                let mut copy = source.clone();
                copy.import_source = None;
                copy.name = old.name.clone();
                copy.desktop_binding = Some(Binding {
                    source_fingerprint: models::fingerprint(&source)?,
                    ..binding.clone()
                });
                copy.models = old.models.clone();
                copy.model_aliases = old.model_aliases.clone();
                config.ai_gateway.providers[index] = copy;
            }
            if let Some(label) = request.label {
                config.ai_gateway.providers[index]
                    .desktop_binding
                    .as_mut()
                    .unwrap()
                    .label = label;
            }
            if let Some(mut provider) = request.provider {
                let visible = binding.model.clone();
                let upstream = provider
                    .resolve_upstream_model(&visible)
                    .context("请保留此条目的模型名称；新增其他模型请使用“添加模型”")?
                    .to_string();
                provider.name = old.name;
                provider.desktop_binding =
                    config.ai_gateway.providers[index].desktop_binding.clone();
                provider.import_source = None;
                provider.gmclaw_parameters = None;
                provider.models = vec![upstream.clone()];
                provider.model_aliases.clear();
                provider.model_aliases.insert(visible, upstream);
                ensure!(
                    url::Url::parse(&provider.base_url)
                        .is_ok_and(
                            |u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some()
                        ),
                    "上游地址无效"
                );
                config.ai_gateway.providers[index] = provider;
            }
            config.ai_gateway.providers[index].enabled = true;
        } else {
            config.ai_gateway.providers.remove(index);
        }
    } else {
        ensure!(request.using, "请选择已有客户端模型");
        let source = config
            .ai_gateway
            .providers
            .iter()
            .find(|p| Some(provider_route_id(p)) == request.source_route)
            .context("请选择模型来源")?
            .clone();
        let model = request.model.context("请选择模型")?;
        ensure!(
            !config.ai_gateway.providers.iter().any(|p| p
                .desktop_binding
                .as_ref()
                .is_some_and(|b| b.target_id == target.id && b.model == model)),
            "此客户端已经有同名模型，请编辑原条目或使用不同别名"
        );
        let mut copy = models::copy(
            &source,
            &target,
            &model,
            request.label.as_deref().unwrap_or(""),
        )?;
        if target.client == crate::desktop::clients::ClientId::WebStorm
            && target.model_mode == "native"
        {
            if let Some(old) = config
                .ai_gateway
                .providers
                .iter()
                .filter_map(|p| p.desktop_binding.as_ref())
                .find(|b| b.target_id == target.id)
            {
                copy.desktop_binding.as_mut().unwrap().local_token = old.local_token.clone();
            }
        }
        id = copy.desktop_binding.as_ref().unwrap().id.clone();
        config.ai_gateway.providers.push(copy);
    }
    config.ai_gateway.enabled = true;
    let native_parameters = config
        .ai_gateway
        .providers
        .iter()
        .find(|p| p.desktop_binding.as_ref().is_some_and(|b| b.id == id))
        .and_then(crate::desktop::tiangong_models::parameters_for_provider);
    if let Some(binding) = config
        .ai_gateway
        .providers
        .iter_mut()
        .filter_map(|p| p.desktop_binding.as_mut())
        .find(|b| b.id == id)
    {
        if let Some(protocol) = request.wire_protocol {
            ensure!(
                matches!(
                    protocol.as_str(),
                    "openai-completions" | "openai-responses" | "anthropic-messages"
                ),
                "请选择支持的客户端协议"
            );
            binding.wire_protocol = protocol;
        }
        if let Some(capabilities) = native_parameters
            .map(|p| p.capabilities())
            .or(request.capabilities)
        {
            ensure!(capabilities.is_object(), "模型能力需要为对象");
            if let Some(value) = capabilities.get("defaultReasoningEffort") {
                ensure!(
                    value.as_str().is_some_and(|s| !s.is_empty()
                        && s.len() <= 64
                        && s.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')),
                    "默认思考强度无效"
                );
            }
            if let Some(value) = capabilities.get("preferStream") {
                ensure!(value.is_boolean(), "流式偏好无效");
            }
            for key in ["contextWindow", "maxTokens"] {
                if let Some(value) = capabilities.get(key) {
                    ensure!(
                        value.as_u64().is_some_and(|v| v > 0 && v <= 100_000_000),
                        "模型上下文或输出上限无效"
                    );
                }
            }
            binding.capabilities = capabilities;
        }
    }
    let undo = model_operations::commit(
        path,
        &before,
        &mut config,
        &target,
        request.replace_conflict,
    )?;
    Ok((id, undo))
}
#[derive(Deserialize)]
struct Restore {
    undo: String,
}
async fn restore(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Restore>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let mut current = state.config.lock().await;
    let path = state.config_path.clone();
    let result =
        tokio::task::spawn_blocking(move || model_operations::restore(&path, &request.undo)).await;
    response(match result {
        Ok(Ok(config)) => {
            *current = config;
            Ok(
                json!({"restored":true,"view":models::public(&current.ai_gateway),"revision":current.revision}),
            )
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(anyhow::anyhow!("恢复中断，请重新读取")),
    })
}
#[derive(Deserialize)]
struct Read {
    id: String,
}
async fn read(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Read>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let config = state.config.lock().await;
    response(
        config
            .ai_gateway
            .providers
            .iter()
            .find(|p| {
                p.desktop_binding
                    .as_ref()
                    .is_some_and(|b| b.id == request.id)
            })
            .context("客户端模型不存在")
            .and_then(|p| Ok(serde_json::to_value(p)?)),
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopyLegacy {
    client: crate::desktop::clients::ClientId,
    source_route: String,
    model: String,
    revision: String,
    #[serde(default)]
    target_id: Option<String>,
    #[serde(default)]
    workbuddy: Option<crate::workbuddy_config::WorkBuddySaveRequest>,
    #[serde(default)]
    tiangong: Option<crate::gmclaw_config::GmClawSaveRequest>,
}
impl CopyLegacy {
    fn validate_editor(&self, source: &ProviderConfig) -> Result<()> {
        use crate::desktop::clients::ClientId;
        ensure!(
            self.workbuddy.is_none() || self.client == ClientId::WorkBuddy,
            "模型参数不属于所选客户端"
        );
        ensure!(
            self.tiangong.is_none() || self.client == ClientId::Tiangong,
            "模型参数不属于所选客户端"
        );
        if let Some(request) = &self.workbuddy {
            ensure!(
                !request.revision.trim().is_empty()
                    && request.model.provider_model == self.model
                    && request.model.upstream_provider == source.name,
                "来源或模型参数已变化，请重新读取"
            );
        }
        if let Some(request) = &self.tiangong {
            ensure!(
                !request.revision.trim().is_empty()
                    && request.model == self.model
                    && request.source_provider == source.name,
                "来源或模型参数已变化，请重新读取"
            );
        }
        Ok(())
    }
}
async fn copy_legacy(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<CopyLegacy>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let mut current = state.config.lock().await;
    let path = state.config_path.clone();
    let outcome=tokio::task::spawn_blocking(move||{
        let result=(||->Result<Value>{
            use crate::desktop::clients::ClientId;
            use crate::ai_gateway::config::ProviderType;
            let mut config=AppConfig::load_or_default(&path)?;
            ensure!(config.revision.as_deref()==Some(request.revision.as_str()),"来源已变化，请重新读取");
            if let Some(id)=&request.target_id{
                let defaults=crate::desktop::targets::discover()?;
                ensure!(defaults.iter().any(|t|t.client==request.client&&t.id==*id),"此客户端模型仅支持其当前实际配置位置；自定义目录可用于 Skill / MCP，请在接入设置中调整模型目录");
            }
            let source=config.ai_gateway.providers.iter().find(|p|provider_route_id(p)==request.source_route&&p.enabled).cloned().context("来源已停用或删除")?;
            ensure!(source.matches_model(&request.model),"来源不再提供这个模型");
            request.validate_editor(&source)?;
            let status=match request.client{
                ClientId::WorkBuddy=>{
                    let protocol=match source.provider_type{ProviderType::AnthropicMessages=>"anthropic-messages",ProviderType::ChatCompletions=>"openai-chat",_=>"openai-responses"};
                    let model=crate::workbuddy_config::WorkBuddyModelConfig{upstream_provider:source.name.clone(),provider_model:request.model.clone(),upstream_protocol:protocol.into(),..Default::default()};
                    let editor = match request.workbuddy { Some(editor) => editor, None => crate::workbuddy_config::WorkBuddySaveRequest{entry_id:None,revision:crate::workbuddy_config::load_selected(&config,None)?.revision,model} };
                    serde_json::to_value(crate::workbuddy_config::save_copy(&editor,&source,&mut config,&path)?)?
                },
                ClientId::Tiangong=>{
                    let editor = match request.tiangong { Some(editor) => editor, None => crate::gmclaw_config::GmClawSaveRequest{source_provider:source.name.clone(),model:request.model,revision:crate::gmclaw_config::load_selected(&config,None)?.revision,..Default::default()} };
                    serde_json::to_value(crate::gmclaw_config::save_copy(&editor,&source,&mut config,&path)?)?
                },
                ClientId::Codex=>{
                    // Codex's ordinary model catalog is retained. A copy has a
                    // unique visible alias so an existing same-name route is not borrowed.
                    let upstream=source.resolve_upstream_model(&request.model).context("模型已变化")?.to_string();
                    let id=uuid::Uuid::new_v4().simple().to_string();let alias=format!("{}-{}",request.model,&id[..6]);
                    let mut provider=source;provider.name=format!("Codex · {} · {}",request.model,&id[..6]);provider.desktop_binding=None;provider.import_source=None;provider.gmclaw_parameters=None;
                    provider.models=vec![];provider.model_aliases.clear();provider.model_aliases.insert(alias.clone(),upstream);
                    config.ai_gateway.providers.push(provider);config.ai_gateway.codex_visible_models.push(alias.clone());config.ai_gateway.enabled=true;config.save(&path)?;
                    json!({"model":alias,"message":"已加入 Codex 模型；首次使用请完成接入设置，已有会话请重新选择模型"})
                },
                _=>anyhow::bail!("请选择此客户端的原生目标"),
            };Ok(status)
        })();
        (result,AppConfig::load_or_default(&path))
    }).await;
    match outcome {
        Ok((result, latest)) => {
            if let Ok(latest) = latest {
                *current = latest;
            }
            response(result.map(|status|json!({"status":status,"view":models::public(&current.ai_gateway),"revision":current.revision})))
        }
        Err(_) => response::<Value>(Err(anyhow::anyhow!("复制中断，请重新读取；不会重复执行"))),
    }
}

#[cfg(test)]
mod copy_contract_tests {
    use super::*;
    #[tokio::test]
    async fn codex_builtin_removed_from_visible_list_is_reused_on_the_next_share() {
        use crate::desktop::{
            clients::ClientId,
            tiangong_models::{BuiltinBinding, ENDPOINT},
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("hub.toml");
        let lib = Library::new(&path).unwrap();
        let target = lib
            .targets(&lib.load().unwrap())
            .unwrap()
            .into_iter()
            .find(|t| t.client == ClientId::Codex)
            .unwrap();
        let source = ProviderConfig {
            name: "天工内置 · 外网".into(),
            base_url: ENDPOINT.trim_end_matches("/chat/completions").into(),
            provider_type: crate::ai_gateway::config::ProviderType::ChatCompletions,
            model_aliases: std::collections::BTreeMap::from([(
                "天工内置-外网".into(),
                "fixture-model".into(),
            )]),
            tiangong_builtin: Some(BuiltinBinding {
                model_id: "tiangong-builtin".into(),
                upstream_model: "fixture-model".into(),
                parameters: None,
                credential_ref: uuid::Uuid::new_v4().to_string(),
            }),
            ..Default::default()
        };
        let mut copy = source.clone();
        copy.name = "Codex · 天工内置-外网 · abc123".into();
        copy.model_aliases = std::collections::BTreeMap::from([(
            "天工内置-外网-abc123".into(),
            "fixture-model".into(),
        )]);
        let mut config = AppConfig::default();
        config.state_path = temp.path().join("state.json");
        config.ai_gateway.providers = vec![source, copy];
        config.ai_gateway.codex_visible_models.clear();
        config.save(&path).unwrap();
        let state = crate::app_state::AppState::new(
            path.clone(),
            AppConfig::load_or_default(&path).unwrap(),
            None,
            None,
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-tiancai-desktop",
            axum::http::HeaderValue::from_static("1"),
        );
        for _ in 0..2 {
            let rev = state.config.lock().await.revision.clone().unwrap();
            let result = share_tiangong_builtin(
                State(state.clone()),
                headers.clone(),
                Json(ShareBuiltin {
                    revision: rev,
                    model_id: "tiangong-builtin".into(),
                    targets: vec![target.id.clone()],
                }),
            )
            .await;
            assert!(result.status().is_success());
            let body: Value = serde_json::from_slice(
                &axum::body::to_bytes(result.into_body(), 1024 * 1024)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(body["outcomes"][0]["ok"], true, "{body}");
            let current = AppConfig::load_or_default(&path).unwrap();
            assert_eq!(current.ai_gateway.providers.len(), 2);
            assert_eq!(
                current.ai_gateway.codex_visible_models,
                vec!["天工内置-外网-abc123"]
            );
        }
    }
    #[tokio::test]
    async fn builtin_share_is_idempotent_keeps_client_edits_and_rejects_stale_revision() {
        use crate::desktop::{
            clients::ClientId,
            tiangong_models::{BuiltinBinding, ENDPOINT},
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("hub.toml");
        let library = Library::new(&path).unwrap();
        let target = library
            .targets(&library.load().unwrap())
            .unwrap()
            .into_iter()
            .find(|t| t.client == ClientId::VsCode)
            .unwrap();
        let source = ProviderConfig {
            name: "天工内置 · 外网".into(),
            enabled: true,
            provider_type: crate::ai_gateway::config::ProviderType::ChatCompletions,
            base_url: ENDPOINT.trim_end_matches("/chat/completions").into(),
            model_aliases: std::collections::BTreeMap::from([(
                "天工内置-外网".into(),
                "fixture-model".into(),
            )]),
            tiangong_builtin: Some(BuiltinBinding {
                model_id: "tiangong-builtin".into(),
                upstream_model: "fixture-model".into(),
                parameters: None,
                credential_ref: uuid::Uuid::new_v4().to_string(),
            }),
            ..Default::default()
        };
        let mut copy = models::copy(&source, &target, "天工内置-外网", "独立名称").unwrap();
        copy.desktop_binding.as_mut().unwrap().capabilities = json!({"maxTokens":1234});
        let mut config = AppConfig::default();
        config.state_path = temp.path().join("state.json");
        config.ai_gateway.providers = vec![source, copy.clone()];
        config.save(&path).unwrap();
        let revision = AppConfig::load_or_default(&path).unwrap().revision.unwrap();
        let before = std::fs::read(&path).unwrap();
        let state = crate::app_state::AppState::new(
            path.clone(),
            AppConfig::load_or_default(&path).unwrap(),
            None,
            None,
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-tiancai-desktop",
            axum::http::HeaderValue::from_static("1"),
        );
        for rev in [revision.clone(), revision.clone(), "stale-revision".into()] {
            let result = share_tiangong_builtin(
                State(state.clone()),
                headers.clone(),
                Json(ShareBuiltin {
                    revision: rev.clone(),
                    model_id: "tiangong-builtin".into(),
                    targets: vec![target.id.clone()],
                }),
            )
            .await;
            assert_eq!(result.status().is_success(), rev == revision);
            let body = axum::body::to_bytes(result.into_body(), 1024 * 1024)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            if rev == revision {
                assert_eq!(body["outcomes"][0]["ok"], true);
            }
            assert_eq!(std::fs::read(&path).unwrap(), before);
            let latest = state.config.lock().await;
            assert_eq!(latest.ai_gateway.providers.len(), 2);
            assert_eq!(
                serde_json::to_value(&latest.ai_gateway.providers[1]).unwrap(),
                serde_json::to_value(&copy).unwrap()
            );
        }
        // Sharing a disabled copy must really enable its native configuration,
        // keeping the user's independent capabilities and the same binding ID.
        let mut disabled = AppConfig::load_or_default(&path).unwrap();
        disabled.ai_gateway.providers[1].enabled = false;
        disabled.save(&path).unwrap();
        let disabled = AppConfig::load_or_default(&path).unwrap();
        let rev = disabled.revision.clone().unwrap();
        *state.config.lock().await = disabled;
        let result = share_tiangong_builtin(
            State(state.clone()),
            headers,
            Json(ShareBuiltin {
                revision: rev,
                model_id: "tiangong-builtin".into(),
                targets: vec![target.id.clone()],
            }),
        )
        .await;
        assert!(result.status().is_success());
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(result.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["outcomes"][0]["ok"], true, "{body}");
        let latest = AppConfig::load_or_default(&path).unwrap();
        assert_eq!(latest.ai_gateway.providers.len(), 2);
        assert!(latest.ai_gateway.providers[1].enabled);
        assert_eq!(
            latest.ai_gateway.providers[1]
                .desktop_binding
                .as_ref()
                .unwrap()
                .capabilities,
            json!({"maxTokens":1234})
        );
        assert_eq!(
            latest.ai_gateway.providers[1]
                .desktop_binding
                .as_ref()
                .unwrap()
                .id,
            copy.desktop_binding.as_ref().unwrap().id
        );
    }
    #[test]
    fn complete_native_editors_are_bound_to_the_explicit_client_source_and_model() {
        let source = ProviderConfig {
            name: "source-entry".into(),
            ..Default::default()
        };
        let old = json!({"client":"workbuddy", "sourceRoute":"source-route", "model":"gpt-6", "revision":"source-revision"});
        assert!(
            serde_json::from_value::<CopyLegacy>(old.clone())
                .unwrap()
                .validate_editor(&source)
                .is_ok()
        );
        for (client, field, payload) in [
            (
                "workbuddy",
                "workbuddy",
                json!({"revision":"original-native-revision", "entryId":null,"model":{"upstreamProvider":"source-entry", "providerModel":"gpt-6", "upstreamProtocol":"openai-chat", "upstreamDefaultReasoningEffort":"max"}}),
            ),
            (
                "tiangong",
                "tiangong",
                json!({"revision":"original-native-revision", "entryId":null, "sourceProvider":"source-entry", "model":"gpt-6", "maxTokens":32768, "temperature":0.35, "parameters":{"reasoningEffort":"max", "temperatureMode":"preserve"}}),
            ),
        ] {
            let mut body = old.clone();
            body["client"] = client.into();
            body[field] = payload;
            let request: CopyLegacy = serde_json::from_value(body.clone()).unwrap();
            request.validate_editor(&source).unwrap();
            for mutation in ["client", "model", "source", "revision"] {
                let mut wrong = body.clone();
                match mutation {
                    "client" => wrong["client"] = "codex".into(),
                    "model" => wrong["model"] = "same-name-other-model".into(),
                    "source" if client == "workbuddy" => {
                        wrong[field]["model"]["upstreamProvider"] = "other-source".into()
                    }
                    "source" => wrong[field]["sourceProvider"] = "other-source".into(),
                    _ => wrong[field]["revision"] = "".into(),
                }
                assert!(
                    serde_json::from_value::<CopyLegacy>(wrong)
                        .unwrap()
                        .validate_editor(&source)
                        .is_err(),
                    "{client}: {mutation}"
                );
            }
        }
    }
}
