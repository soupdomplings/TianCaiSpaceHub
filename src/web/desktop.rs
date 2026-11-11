use crate::{
    app_state::SharedState,
    desktop::{
        mcp,
        resources::{Library, UseOutcome},
        skills,
    },
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

pub(super) fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/desktop/activity", get(activity))
        .route("/api/desktop/client/open", post(launch_client))
        .route("/api/desktop/usage", get(usage))
        .route("/api/desktop/requests", get(requests))
        .route("/api/desktop/library", get(library))
        .route("/api/desktop/library/discover", post(discover))
        .route("/api/desktop/mcp/save", post(save_mcp))
        .route("/api/desktop/mcp/read", post(read_mcp))
        .route("/api/desktop/skill/import", post(import_skill))
        .route(
            "/api/desktop/skills/market",
            get(skill_market_cache).post(skill_market_refresh),
        )
        .route("/api/desktop/skills/market/use", post(skill_market_use))
        .route("/api/desktop/resource/use", post(use_resource))
        .route("/api/desktop/resource/selection", post(apply_selection))
        .route("/api/desktop/resource/delete", post(delete_resource))
        .route("/api/desktop/resource/restore", post(restore_resource))
        .route("/api/desktop/targets/save", post(save_target))
        .route("/api/desktop/components", get(components))
        .route("/api/desktop/components/install", post(install_component))
        .route("/api/desktop/webstorm/plugin", post(export_plugin))
}
static SKILL_MARKET_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
async fn skill_market_cache(State(state): State<SharedState>) -> Response {
    response(
        crate::desktop::skill_market::cached(&state.config_path)
            .map(|catalog| json!({"catalog":catalog})),
    )
}
#[derive(Deserialize)]
struct MarketRefresh {
    repository: String,
}
async fn skill_market_refresh(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<MarketRefresh>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let _guard = SKILL_MARKET_GATE.lock().await;
    response(
        async {
            let config = state.config.lock().await.clone();
            let http = crate::external_import::client::build_client(
                &config.outbound_proxy,
                config.local_listen_port(),
            )
            .map_err(anyhow::Error::msg)?;
            let catalog = crate::desktop::skill_market::catalog(&http, &request.repository).await?;
            let path = crate::desktop::skill_market::cache_path(&state.config_path);
            crate::desktop::transaction::atomic_write(&path, &serde_json::to_vec(&catalog)?)?;
            Ok(json!({"catalog":catalog}))
        }
        .await,
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketUse {
    commit: String,
    path: String,
    revision: String,
    targets: Vec<String>,
}
async fn skill_market_use(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<MarketUse>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let _guard = SKILL_MARKET_GATE.lock().await;
    response(
        async {
            anyhow::ensure!(
                !request.targets.is_empty() && request.targets.len() <= 128,
                "请先选择使用客户端"
            );
            let catalog = crate::desktop::skill_market::cached(&state.config_path)?
                .ok_or_else(|| anyhow::anyhow!("请先刷新 Skill 广场"))?;
            anyhow::ensure!(
                catalog.commit == request.commit,
                "广场仓库已变化，请刷新后重新选择"
            );
            // Validate revision/targets before downloading or writing a package.
            let lib = Library::new(&state.config_path)?;
            let local = lib.load()?;
            anyhow::ensure!(
                local.revision == request.revision,
                "客户端资源已变化，请返回重新选择"
            );
            let targets = lib.targets(&local)?;
            for id in &request.targets {
                anyhow::ensure!(
                    targets.iter().any(|t| t.id == *id && t.present),
                    "客户端位置已变化，请重新选择"
                );
            }
            let config = state.config.lock().await.clone();
            let http = crate::external_import::client::build_client(
                &config.outbound_proxy,
                config.local_listen_port(),
            )
            .map_err(anyhow::Error::msg)?;
            let package =
                crate::desktop::skill_market::download(&http, &catalog, &request.path).await?;
            let source = format!(
                "{}/tree/{}/{}",
                catalog.repository, catalog.commit, request.path
            );
            let id = blocking(state.clone(), move |lib| {
                Ok(lib.import_skill(package, source, &request.revision)?.id)
            })
            .await?;
            let outcomes = apply_targets(state.clone(), &id, &request.targets, true, false).await;
            Ok(json!({"view":library_view(state).await?,"outcomes":outcomes,"resourceId":id}))
        }
        .await,
    )
}
async fn components() -> Response {
    response(
        tokio::task::spawn_blocking(crate::desktop::components::status)
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("组件状态读取中断"))),
    )
}
async fn install_component(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let proxy = state.config.lock().await.outbound_proxy.clone();
    response(crate::desktop::components::install(&proxy).await)
}
#[derive(Deserialize)]
struct ExportPlugin {
    path: PathBuf,
}
async fn export_plugin(headers: HeaderMap, Json(request): Json<ExportPlugin>) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    response(
        tokio::task::spawn_blocking(move || {
            anyhow::ensure!(
                request.path.extension().is_some_and(|v| v == "jar"),
                "请保存为 .jar 插件文件"
            );
            crate::desktop::webstorm_native::export_plugin(&request.path)?;
            Ok(json!({"exported":true}))
        })
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("插件导出中断"))),
    )
}
#[derive(Deserialize)]
struct Target {
    client: crate::desktop::clients::ClientId,
    root: String,
}
async fn save_target(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<Target>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    response(
        blocking(state, move |lib| {
            use crate::desktop::{
                clients::ClientId,
                targets::{self, McpFormat},
            };
            let root = PathBuf::from(request.root.trim());
            targets::validate_override(&root)?;
            anyhow::ensure!(root.is_dir(), "请选择已经存在的客户端配置根目录");
            anyhow::ensure!(
                request.client != ClientId::Tiangong,
                "天工使用本机授权服务的实际配置目录，请在客户端内设置后重新读取"
            );
            let (skills, mcp, format, mode) = match request.client {
                ClientId::Codex => (
                    root.join("skills"),
                    root.join("config.toml"),
                    McpFormat::Codex,
                    "codex",
                ),
                ClientId::WorkBuddy => (
                    root.join("skills"),
                    root.join("mcp.json"),
                    McpFormat::McpServers,
                    "workbuddy",
                ),
                ClientId::VsCode => (
                    targets::home()?.join(".copilot/skills"),
                    root.join("mcp.json"),
                    McpFormat::Servers,
                    "chat",
                ),
                ClientId::DeepSeekHarness => (
                    root.join("skills"),
                    root.join("cordis.patch.yml"),
                    McpFormat::Harness,
                    "harness",
                ),
                ClientId::ZCode => (
                    root.join("skills"),
                    root.join("cli/config.json"),
                    McpFormat::ZCode,
                    "zcode",
                ),
                ClientId::WebStorm => (
                    targets::home()?.join(".config/opencode/skills"),
                    root.join("tiancai-opencode.json"),
                    McpFormat::OpenCode,
                    "acp",
                ),
                _ => unreachable!(),
            };
            let target = targets::target(request.client, "自定义", root, skills, mcp, format, mode);
            let catalog = lib.load()?;
            let mut overrides = catalog.target_overrides.clone();
            anyhow::ensure!(
                !lib.targets(&catalog)?.iter().any(|t| t.id == target.id),
                "此配置位置已存在，请直接在客户端页面选择"
            );
            overrides.push(target);
            lib.save_targets(catalog, overrides)?;
            lib.view()
        })
        .await,
    )
}
#[derive(Default, Deserialize)]
struct ActivityQuery {
    #[serde(default)]
    live: bool,
}
async fn activity(
    State(state): State<SharedState>,
    Query(query): Query<ActivityQuery>,
) -> Json<Value> {
    let config = state.config.lock().await.clone();
    let path = state.config_path.clone();
    let runtime_config = config.clone();
    // The route graph only needs local process/configuration evidence. Do not
    // put remote execution, IM or client bridge queries in its refresh path.
    let runtime = tokio::task::spawn_blocking(move || {
        crate::desktop::runtime::snapshot(&path, &runtime_config, &[])
    });
    if query.live {
        let clients = runtime.await.unwrap_or_default();
        let requests = state.desktop_activity.snapshot();
        let recent = state.desktop_activity.recently_completed();
        let mcp_calls = state.desktop_mcp_activity.snapshot();
        let clients = crate::desktop::runtime::attach_requests(clients, &requests);
        return Json(
            json!({"clients":clients,"requests":requests,"recentRequests":recent,"mcpCalls":mcp_calls,"gatewayEnabled":config.ai_gateway.enabled,"at":crate::ai_gateway::request_log::now_ms()}),
        );
    }
    let (clients, overview, remote, chat) = tokio::join!(
        runtime,
        crate::client_overview::snapshot(config.clone()),
        crate::remote_control_backend::status_snapshot(&state),
        super::im_api::im_accounts_snapshot(&state)
    );
    // Both client rows and model routes use one snapshot taken after the slow
    // status reads, including requests that started while those reads ran.
    let requests = state.desktop_activity.snapshot();
    let recent = state.desktop_activity.recently_completed();
    let mcp_calls = state.desktop_mcp_activity.snapshot();
    let clients = crate::desktop::runtime::attach_requests(clients.unwrap_or_default(), &requests);
    let execution = json!({"codex":{"connected":remote.connected,"initialized":remote.initialized,"healthy":remote.healthy,"stale":remote.stale}});
    Json(
        json!({"clients":clients,"requests":requests,"recentRequests":recent,"mcpCalls":mcp_calls,"overview":overview,"execution":execution,"chat":chat,"gatewayEnabled":config.ai_gateway.enabled,"at":crate::ai_gateway::request_log::now_ms()}),
    )
}
#[derive(Deserialize)]
struct LaunchClient {
    client: crate::desktop::clients::ClientId,
    path: Option<PathBuf>,
    enhanced: Option<bool>,
}
async fn launch_client(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(input): Json<LaunchClient>,
) -> Response {
    response(
        async {
            trusted(&headers)?;
            let path = state.config_path.clone();
            if let Some(executable) = input.path {
                tokio::task::spawn_blocking(move || {
                    crate::desktop::launcher::save(&path, input.client, &executable)?;
                    Ok(json!({"saved":true}))
                })
                .await?
            } else {
                let config = state.config.lock().await.clone();
                crate::desktop::launcher::open_with_config(
                    &path,
                    config,
                    input.client,
                    input.enhanced.unwrap_or(true),
                )
                .await
            }
        }
        .await,
    )
}
async fn usage(
    State(state): State<SharedState>,
    Query(filter): Query<crate::ai_gateway::request_log::desktop::LogFilter>,
) -> Response {
    response(
        tokio::task::spawn_blocking(move || {
            state
                .ai_gateway_request_logs
                .desktop_usage(&filter)
                .map_err(anyhow::Error::from)
        })
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("统计读取中断，请重试"))),
    )
}
async fn requests(
    State(state): State<SharedState>,
    Query(filter): Query<crate::ai_gateway::request_log::desktop::LogFilter>,
) -> Response {
    response(
        tokio::task::spawn_blocking(move || {
            state
                .ai_gateway_request_logs
                .desktop_page(&filter)
                .map_err(anyhow::Error::from)
        })
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("日志读取中断，请重试"))),
    )
}

pub(super) fn trusted(headers: &HeaderMap) -> anyhow::Result<()> {
    anyhow::ensure!(
        headers
            .get("x-tiancai-desktop")
            .and_then(|v| v.to_str().ok())
            == Some("1")
            && headers.get("origin").is_none(),
        "请从 Hub 界面操作客户端配置"
    );
    Ok(())
}
pub(super) fn response<T: serde::Serialize>(result: anyhow::Result<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::CONFLICT,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
    }
}
async fn blocking<T: Send + 'static>(
    state: SharedState,
    operation: impl FnOnce(Library) -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(move || operation(Library::new(&state.config_path)?))
        .await
        .map_err(|_| anyhow::anyhow!("配置操作被中断，请重新读取状态"))?
}
async fn library_view(
    state: SharedState,
) -> anyhow::Result<crate::desktop::resources::LibraryView> {
    let path = state.config_path.clone();
    let mut view = blocking(state.clone(), |lib| lib.view()).await?;
    if view.resources.iter().any(|r| !r.targets.is_empty()) {
        let config = state.config.lock().await.clone();
        crate::desktop::tiangong_resources::enrich(
            &Library::new(&path)?,
            &mut view,
            crate::desktop::tiangong_resources::client(&config.gmclaw_bridge).await,
            config.local_listen_port().unwrap_or(3210),
        )
        .await;
    }
    Ok(view)
}
async fn library(State(state): State<SharedState>) -> Response {
    response(library_view(state).await)
}
async fn discover(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    response(
        async {
            let mut issues = blocking(state.clone(), |lib| lib.discover_issues()).await?;
            let config = state.config.lock().await.gmclaw_bridge.clone();
            if let Ok(client) = crate::desktop::tiangong_resources::client(&config).await {
                let warnings=crate::desktop::tiangong_resources::discover(
                        &Library::new(&state.config_path)?,
                        &client,
                    )
                    .await?;
                issues.extend(warnings.into_iter().map(|reason|crate::desktop::resources::DiscoveryIssue {client:crate::desktop::clients::ClientId::Tiangong,kind:crate::desktop::resources::ResourceKind::Mcp,name:"天工 Work 工具服务".into(),location:PathBuf::new(),reason,guidance:"请在天工 Work 中核对服务是否存在，再回到 Hub 重新读取；原客户端内容不会自动删除。".into()}));
            }
            let warnings=issues.iter().map(|i|format!("{} · {}：{}",i.client.name(),i.name,i.reason)).collect::<Vec<_>>();
            Ok(json!({"view":library_view(state).await?,"warnings":warnings,"issues":issues}))
        }
        .await,
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveMcp {
    id: Option<String>,
    name: String,
    description: Option<String>,
    json: String,
    revision: String,
    #[serde(default)]
    targets: Vec<String>,
}
async fn save_mcp(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<SaveMcp>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    if request.targets.is_empty() {
        return response::<Value>(Err(anyhow::anyhow!("请至少选择一个客户端")));
    }
    let targets = request.targets.clone();
    let saved = blocking(state.clone(), move |lib| {
        let mut servers = mcp::parse_import(&request.json, &request.name)?;
        let server = if servers.len() == 1 {
            servers.remove(0)
        } else {
            let index = servers
                .iter()
                .position(|s| s.name == request.name)
                .ok_or_else(|| anyhow::anyhow!("JSON 包含多个服务，请明确选择本次添加的一项"))?;
            servers.remove(index)
        };
        Ok(lib
            .save_mcp(
                request.id.as_deref(),
                server,
                request.description.unwrap_or_default(),
                &request.revision,
            )?
            .id)
    })
    .await;
    match saved {
        Ok(id) => {
            let outcomes = apply_targets(state.clone(), &id, &targets, true, false).await;
            response(
                library_view(state)
                    .await
                    .map(|view| json!({"view":view,"outcomes":outcomes,"resourceId":id})),
            )
        }
        Err(e) => response::<Value>(Err(e)),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceId {
    resource_id: String,
}
async fn read_mcp(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<ResourceId>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    response(
        blocking(state, move |lib| {
            let catalog = lib.load()?;
            let resource = catalog
                .resources
                .iter()
                .find(|r| r.id == request.resource_id)
                .ok_or_else(|| anyhow::anyhow!("所选 MCP 已不存在"))?;
            Ok(lib.mcp_config(resource)?)
        })
        .await,
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportSkill {
    path: String,
    #[serde(default)]
    subdirectory: Option<String>,
    revision: String,
    #[serde(default)]
    targets: Vec<String>,
}
async fn import_skill(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<ImportSkill>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let mut temp = None;
    let path = if request.path.starts_with("https://") {
        match download_skill_archive(&request.path).await {
            Ok(value) => {
                let path = value.path().to_path_buf();
                temp = Some(value);
                path
            }
            Err(e) => return response::<Value>(Err(e)),
        }
    } else {
        PathBuf::from(&request.path)
    };
    if request.targets.is_empty() {
        return response::<Value>(Err(anyhow::anyhow!("请至少选择一个客户端")));
    }
    let targets = request.targets.clone();
    let saved = blocking(state.clone(), move |lib| {
        let _temp = temp;
        let package = if path.is_dir() {
            skills::from_directory(&path)?
        } else {
            skills::from_zip(&path, request.subdirectory.as_deref())?
        };
        Ok(lib
            .import_skill(
                package,
                if request.path.starts_with("https://") {
                    request.path
                } else {
                    "本地导入".into()
                },
                &request.revision,
            )?
            .id)
    })
    .await;
    match saved {
        Ok(id) => {
            let outcomes = apply_targets(state.clone(), &id, &targets, true, false).await;
            response(
                library_view(state)
                    .await
                    .map(|view| json!({"view":view,"outcomes":outcomes,"resourceId":id})),
            )
        }
        Err(e) => response::<Value>(Err(e)),
    }
}
async fn download_skill_archive(input: &str) -> anyhow::Result<tempfile::NamedTempFile> {
    use futures_util::StreamExt;
    use std::io::Write;
    let mut url = url::Url::parse(input)?;
    anyhow::ensure!(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "请使用不包含凭据的 HTTPS 仓库或 ZIP 地址"
    );
    if url.host_str() == Some("github.com") && !url.path().ends_with(".zip") {
        let parts = url
            .path()
            .trim_matches('/')
            .trim_end_matches(".git")
            .split('/')
            .collect::<Vec<_>>();
        anyhow::ensure!(
            parts.len() == 2,
            "仓库地址请使用 https://github.com/作者/仓库，或完整 ZIP 下载地址"
        );
        url = url::Url::parse(&format!(
            "https://codeload.github.com/{}/{}/zip/HEAD",
            parts[0], parts[1]
        ))?;
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()?;
    let result = client.get(url).send().await?;
    anyhow::ensure!(
        result.status().is_success(),
        "无法下载 Skill，请检查仓库是否公开或改用本地 ZIP"
    );
    anyhow::ensure!(
        result.content_length().is_none_or(|n| n <= 8 * 1024 * 1024),
        "Skill 下载包超过 8 MiB，请选择本地文件夹"
    );
    let mut temp = tempfile::NamedTempFile::new()?;
    let mut bytes = 0;
    let mut stream = result.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        bytes += chunk.len();
        anyhow::ensure!(bytes <= 8 * 1024 * 1024, "Skill 下载包过大");
        temp.write_all(&chunk)?;
    }
    Ok(temp)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UseResource {
    resource_id: String,
    targets: Vec<String>,
    using: bool,
    #[serde(default)]
    replace_conflict: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplySelection {
    resource_id: String,
    revision: String,
    changes: Vec<crate::desktop::resources::SelectionChange>,
}
async fn apply_selection(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<ApplySelection>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    response(async {
        let view=library_view(state.clone()).await?;
        crate::desktop::resources::validate_selection(&view, &request.resource_id, &request.revision, &request.changes)?;
        let mut outcomes=Vec::new();
        for change in &request.changes {
            let applied=apply_targets(state.clone(), &request.resource_id, std::slice::from_ref(&change.target_id), change.using, false).await;
            for outcome in applied {
                let mut value=serde_json::to_value(outcome)?;
                value["requestedUsing"]=change.using.into();
                outcomes.push(value);
            }
        }
        if outcomes.iter().any(|o| o["success"] == true && o["requestedUsing"] == false && view.targets.iter().any(|t| t.client == crate::desktop::clients::ClientId::Tiangong && o["targetId"].as_str() == Some(t.id.as_str()))) {
            crate::desktop::mcp_bridge::stop_resource(&state.config_path, &request.resource_id).await;
        }
        Ok(json!({"view":library_view(state).await?,"outcomes":outcomes,"resourceId":request.resource_id}))
    }.await)
}
async fn apply_targets(
    state: SharedState,
    resource: &str,
    targets: &[String],
    using: bool,
    replace_conflict: bool,
) -> Vec<UseOutcome> {
    let mut outcomes = Vec::new();
    let config = state.config.lock().await.clone();
    let lib = match Library::new(&state.config_path) {
        Ok(lib) => lib,
        Err(e) => {
            return targets
                .iter()
                .map(|id| UseOutcome {
                    target_id: id.clone(),
                    success: false,
                    using: None,
                    message: e.to_string(),
                    undo: None,
                })
                .collect();
        }
    };
    let native = lib.load().and_then(|c| lib.targets(&c)).unwrap_or_default();
    let mut seen = std::collections::BTreeSet::new();
    for target in targets.iter().take(128) {
        if !seen.insert(target.clone()) {
            continue;
        }
        let result = if native
            .iter()
            .any(|t| t.id == *target && t.client == crate::desktop::clients::ClientId::Tiangong)
        {
            match crate::desktop::tiangong_resources::client(&config.gmclaw_bridge).await {
                Ok(client) => {
                    crate::desktop::tiangong_resources::apply(
                        &lib,
                        &client,
                        resource,
                        target,
                        using,
                        replace_conflict,
                        config.local_listen_port().unwrap_or(3210),
                    )
                    .await
                }
                Err(e) => Err(e),
            }
        } else {
            let (resource, target) = (resource.to_string(), target.clone());
            blocking(state.clone(), move |lib| {
                lib.use_files(&resource, &target, using, replace_conflict)
            })
            .await
        };
        outcomes.push(match result {
            Ok(v) => v,
            Err(e) => UseOutcome {
                target_id: target.clone(),
                success: false,
                using: None,
                message: e.to_string(),
                undo: None,
            },
        });
    }
    outcomes
}
async fn use_resource(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<UseResource>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let outcomes = apply_targets(
        state.clone(),
        &request.resource_id,
        &request.targets,
        request.using,
        request.replace_conflict,
    )
    .await;
    if !request.using {
        let lib = Library::new(&state.config_path);
        if let Ok(targets) = lib.and_then(|l| l.load().and_then(|c| l.targets(&c))) {
            if outcomes.iter().any(|o| {
                o.success
                    && targets.iter().any(|t| {
                        t.id == o.target_id
                            && t.client == crate::desktop::clients::ClientId::Tiangong
                    })
            }) {
                crate::desktop::mcp_bridge::stop_resource(&state.config_path, &request.resource_id)
                    .await;
            }
        }
    }
    response(
        library_view(state)
            .await
            .map(|view| json!({"view":view,"outcomes":outcomes,"resourceId":request.resource_id})),
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteResource {
    resource_id: String,
    revision: String,
}
async fn delete_resource(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<DeleteResource>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    response(
        async {
            let view = library_view(state.clone()).await?;
            let row = view
                .resources
                .iter()
                .find(|r| r.resource.id == request.resource_id)
                .ok_or_else(|| anyhow::anyhow!("资源已不存在"))?;
            anyhow::ensure!(
                !row.targets.iter().any(|t| t.using == Some(true)),
                "请先停用此资源的所有使用端"
            );
            blocking(state, move |lib| {
                lib.remove(&request.resource_id, &request.revision)?;
                lib.view()
            })
            .await
        }
        .await,
    )
}
#[derive(Deserialize)]
struct RestoreResource {
    undo: String,
}
async fn restore_resource(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(request): Json<RestoreResource>,
) -> Response {
    if let Err(e) = trusted(&headers) {
        return response::<Value>(Err(e));
    }
    let result = async {
        anyhow::ensure!(uuid::Uuid::parse_str(&request.undo).is_ok(), "恢复身份无效");
        let lib = Library::new(&state.config_path)?;
        if crate::desktop::tiangong_resources::native_undo(&lib, &request.undo) {
            let config = state.config.lock().await.gmclaw_bridge.clone();
            let client = crate::desktop::tiangong_resources::client(&config).await?;
            crate::desktop::tiangong_resources::restore(&lib, &client, &request.undo).await?;
        } else {
            blocking(state.clone(), move |lib| lib.restore(&request.undo)).await?;
        }
        library_view(state).await
    }
    .await;
    response(result)
}
