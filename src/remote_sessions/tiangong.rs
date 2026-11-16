//! Browser adapter for native Work tasks. Reads/writes use the authenticated
//! DataServer; execution uses the native run ID and its resumable event cursor.
use super::{
    files,
    service::ApprovalSource,
    types::{ApprovalDecision, CreateRequest, PageQuery, Scope, TurnRequest},
};
use crate::{
    app_state::SharedState,
    gmclaw_desktop::{DesktopClient, DesktopMessage, DesktopSessionMetadata, DesktopTask},
    gmclaw_runtime,
};
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use reqwest::{Client, Method, header::HeaderValue};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const CLIENT_ID: &str = "tiangong";
#[derive(Clone)]
struct RunClient {
    http: Client,
    root: url::Url,
    authorization: HeaderValue,
    identity: String,
}
impl RunClient {
    fn new(endpoint: &str, authorization: HeaderValue) -> Result<Self> {
        let mut root = crate::gmclaw_executor::validate_endpoint(endpoint)?;
        root.set_path("/v2/chat/runs");
        Ok(Self {
            http: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .read_timeout(Duration::from_secs(300))
                .build()?,
            root,
            identity: gmclaw_runtime::runtime_authorization_identity(&authorization),
            authorization,
        })
    }
    fn url(&self, path: &str) -> Result<url::Url> {
        let mut url = self.root.clone();
        if !path.is_empty() {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("天工运行地址无效"))?;
            for part in path.split('/') {
                segments.push(part);
            }
        }
        Ok(url)
    }
    async fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut request = self
            .http
            .request(method, self.url(path)?)
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .timeout(Duration::from_secs(15));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .context("无法连接天工运行接口，请检查目标客户端；操作不会自动重试")?;
        if !response.status().is_success() {
            let message = format!(
                "天工运行接口返回 HTTP {}，操作未获确认",
                response.status().as_u16()
            );
            if response.status().is_client_error() {
                return Err(anyhow::Error::new(super::NativeRejected(message)));
            }
            anyhow::bail!(message);
        }
        let bytes = response.bytes().await?;
        ensure!(bytes.len() <= 8 * 1024 * 1024, "天工运行状态超过读取范围");
        serde_json::from_slice(&bytes).context("天工运行接口格式不兼容")
    }
    async fn active(&self, session: &str) -> Result<Vec<Value>> {
        let mut url = self.root.clone();
        url.query_pairs_mut()
            .append_pair("session_id", session)
            .append_pair("active_only", "true");
        let response = self
            .http
            .get(url)
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        ensure!(
            response.status().is_success(),
            "当前天工版本未提供可用的运行管理接口，请升级后使用远程执行"
        );
        let data: Value = response.json().await?;
        let runs = data["runs"].as_array().context("天工运行列表格式不兼容")?;
        ensure!(
            runs.iter()
                .all(|r| r["session_id"] == session && r["run_id"].as_str().is_some()),
            "天工返回其他会话的运行状态"
        );
        Ok(runs
            .iter()
            .filter(|r| !terminal(r["status"].as_str().unwrap_or("unknown")))
            .cloned()
            .collect())
    }
}
#[derive(Clone)]
struct ContextData {
    desktop: DesktopClient,
    run: RunClient,
    task: DesktopTask,
    metadata: DesktopSessionMetadata,
    directory: String,
}
async fn connection(state: &SharedState) -> Result<(DesktopClient, RunClient)> {
    let config = state.config.lock().await.gmclaw_bridge.clone();
    ensure!(config.enabled, "请先在目标电脑通过 Hub 接入天工 Work");
    let status = gmclaw_runtime::overview_status(&config).await;
    ensure!(
        status.state == gmclaw_runtime::GmClawConnectionState::Connected,
        "天工尚未就绪：{}",
        status.detail
    );
    let auth = gmclaw_runtime::connection_authorization(&config).await?;
    Ok((
        DesktopClient::with_authorization(&config.endpoint, auth.clone())?,
        RunClient::new(&config.endpoint, auth)?,
    ))
}
fn default_workspace() -> String {
    crate::gmclaw_config::native_config_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("workspace")
        .to_string_lossy()
        .into_owned()
}
fn directory(task: &DesktopTask) -> String {
    if task.work_dir.is_empty() && matches!(task.scenario_id.as_str(), "default" | "0") {
        default_workspace()
    } else {
        task.work_dir.clone()
    }
}
async fn context(state: &SharedState, session: &str) -> Result<ContextData> {
    let (desktop, run) = connection(state).await?;
    let task = desktop
        .list_tasks()
        .await?
        .into_iter()
        .find(|t| t.session_id == session)
        .context("天工原会话已不存在或当前账号无法访问")?;
    let metadata = desktop
        .get_session_metadata(session)
        .await?
        .context("原会话缺少账号和项目元数据，请先在天工打开该会话")?;
    ensure!(
        task.owner_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .is_none_or(|owner| owner == metadata.user_id),
        "天工任务与原会话账号不一致，请先在目标客户端核对"
    );
    ensure!(
        !metadata.user_id.is_empty(),
        "无法确认原会话所属账号，未修改归属或创建替代会话"
    );
    let directory = directory(&task);
    ensure!(
        Path::new(&directory).is_absolute() && Path::new(&directory).is_dir(),
        "原会话项目目录不存在，请在目标电脑核对"
    );
    let revision = desktop.current_task_revision(&task.task_id).await?;
    let desktop = desktop.with_task_revision(&task.task_id, revision.as_deref())?;
    Ok(ContextData {
        desktop,
        run,
        task,
        metadata,
        directory,
    })
}
async fn model_catalog() -> Result<Vec<Value>> {
    tokio::task::spawn_blocking(|| -> Result<_> {
        Ok(crate::gmclaw_config::model_choices()?
            .0
            .into_iter()
            .map(|m| json!({"id":m.value,"name":m.label,"efforts":[]}))
            .collect())
    })
    .await?
}
pub(super) async fn client(state: &SharedState) -> Value {
    match connection(state).await {
        Ok((desktop, run)) => {
            // Probe a read-only run list to distinguish a legacy /v2/chat-only
            // host. Unsupported capabilities are never advertised as working.
            let native = desktop.list_tasks().await;
            let first = native
                .as_ref()
                .ok()
                .and_then(|tasks| tasks.first())
                .map(|t| t.session_id.clone())
                .unwrap_or_else(|| "hub-capability-probe".into());
            let runs_ready = run.active(&first).await.is_ok();
            let models = model_catalog().await.unwrap_or_default();
            let create = runs_ready && gmclaw_runtime::gateway_workspace_active().await;
            json!({"id":CLIENT_ID,"kind":"tiangong","name":"天工 Work","ready":native.is_ok(),
                "reason":native.err().map(|e|e.to_string()),"models":models,"capabilities":{
                    "history":true,"create":create,"send":runs_ready,"stop":runs_ready,"approvals":runs_ready,
                    "questions":false,"attachments":runs_ready,"artifacts":true,"modelSelection":false,"effortSelection":false}})
        }
        Err(error) => {
            json!({"id":CLIENT_ID,"kind":"tiangong","name":"天工 Work","ready":false,"reason":error.to_string(),"capabilities":{},"models":[]})
        }
    }
}
pub(super) async fn projects(state: &SharedState, page: &PageQuery) -> Result<Value> {
    let (desktop, _) = connection(state).await?;
    let projects = project_rows(&desktop).await?;
    let offset = page.offset()?.min(projects.len());
    let end = (offset + page.limit()).min(projects.len());
    Ok(
        json!({"projects":&projects[offset..end],"nextCursor":(end<projects.len()).then(||end.to_string())}),
    )
}
async fn project_rows(desktop: &DesktopClient) -> Result<Vec<Value>> {
    let rows = desktop.list_projects().await?;
    Ok(rows
        .into_iter()
        .filter_map(|p| {
            let path = if p.work_dir.is_empty() && matches!(p.scenario_id.as_str(), "default" | "0")
            {
                default_workspace()
            } else {
                p.work_dir
            };
            if !Path::new(&path).is_absolute() {
                return None;
            }
            let name = Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            Some(json!({"id":p.scenario_id,"name":name,"path":path}))
        })
        .collect())
}
fn session_row(task: &DesktopTask) -> Value {
    json!({"id":task.session_id,"title":task.title,"projectId":task.scenario_id,"cwd":directory(task),"updatedAt":task.updated_at,
        "status":if task.completed_at.is_some() {"completed"} else {"idle"},"model":task.model_id})
}
pub(super) async fn sessions(state: &SharedState, page: &PageQuery) -> Result<Value> {
    let (desktop, _) = connection(state).await?;
    let mut tasks = desktop.list_tasks().await?;
    tasks.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    let offset = page.offset()?.min(tasks.len());
    let end = (offset + page.limit()).min(tasks.len());
    Ok(
        json!({"sessions":tasks[offset..end].iter().map(session_row).collect::<Vec<_>>(),"nextCursor":(end<tasks.len()).then(||end.to_string())}),
    )
}
pub(super) async fn create(state: &SharedState, options: &CreateRequest) -> Result<Value> {
    ensure!(
        gmclaw_runtime::gateway_workspace_active().await,
        "请先在目标电脑使用天工增强启动，再新建 AI Gateway 会话；普通登录会话仍可远程续聊"
    );
    let (desktop, _) = connection(state).await?;
    ensure!(
        desktop
            .conversation_account()
            .await?
            .as_deref()
            .is_none_or(|owner| owner == crate::tiangong_session_history::GATEWAY_USER_ID),
        "天工当前仍是普通账号工作区，请先使用增强启动再新建 AI Gateway 会话"
    );
    let projects = project_rows(&desktop).await?;
    let project = selected_project(&projects, options)?;
    let scenario_id = project["id"].as_str().context("项目标识无效")?;
    let cwd = project["path"].as_str().context("项目目录无效")?;
    ensure!(
        Path::new(cwd).is_dir(),
        "项目目录不存在，请先在目标电脑准备目录"
    );
    let model = options
        .model
        .as_deref()
        .filter(|m| !m.is_empty())
        .context("请选择天工已配置的模型")?;
    let models = model_catalog().await?;
    ensure!(
        models.iter().any(|m| m["id"] == model),
        "所选模型已不在天工模型目录中"
    );
    let model_name = tokio::task::spawn_blocking(crate::gmclaw_config::session_model_metadata)
        .await??
        .into_iter()
        .find(|m| m.0 == model)
        .map(|m| m.1)
        .context("所选天工模型已移除")?;
    let session = uuid::Uuid::new_v4().to_string();
    let task_id = format!("tiancaispacehub-{}", uuid::Uuid::new_v4());
    let title = options
        .title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or("新会话");
    let task = desktop
        .create_task_in_scenario(scenario_id, cwd, title, &session, &task_id, model)
        .await?;
    let project_id = if task.scenario_id == "default" {
        "0".to_owned()
    } else {
        task.scenario_id.clone()
    };
    desktop.create_session_metadata(&DesktopSessionMetadata {
        session_id:session.clone(),title:title.into(),user_id:crate::tiangong_session_history::GATEWAY_USER_ID.into(),
        project_id:project_id.clone(),model_name,
        max_steps:crate::gmclaw_executor::MAX_SESSION_STEPS,
        extra_data:json!({"desktop":true,"project":{"id":project_id,"name":task.scenario_name,"path":directory(&task)}}),
        ..Default::default()
    }).await?;
    state
        .remote_sessions
        .watch(&Scope::new(CLIENT_ID, &session), None)
        .await;
    gmclaw_runtime::queue_desktop_refresh(&task_id, &session, None).await;
    Ok(json!({"session":session_row(&task)}))
}
fn selected_project<'a>(projects: &'a [Value], options: &CreateRequest) -> Result<&'a Value> {
    if let Some(id) = options.project_id.as_deref() {
        return projects
            .iter()
            .find(|project| project["id"] == id)
            .context("所选天工项目已不存在，请重新选择；不会改用同目录的其他项目");
    }
    let cwd = options
        .cwd
        .as_deref()
        .context("请选择目标天工中已有的项目")?;
    let key = crate::gmclaw_desktop::project_path_key(cwd);
    let mut matches = projects.iter().filter(|project| {
        project["path"]
            .as_str()
            .is_some_and(|path| crate::gmclaw_desktop::project_path_key(path) == key)
    });
    let selected = matches
        .next()
        .context("所选目录没有对应的天工项目，请重新选择")?;
    ensure!(
        matches.next().is_none(),
        "此目录对应多个天工项目，请明确选择项目后新建会话"
    );
    Ok(selected)
}
fn native_project_id(project_id: &str) -> &str {
    if project_id == "default" {
        "0"
    } else {
        project_id
    }
}
pub(super) async fn history(state: &SharedState, scope: &Scope, page: &PageQuery) -> Result<Value> {
    let context = context(state, &scope.session).await?;
    state.remote_sessions.watch(scope, None).await;
    let snapshot_cursor = state.remote_sessions.snapshot(scope).await.3;
    let newest = page.cursor.is_none();
    let before = page
        .cursor
        .as_deref()
        .map(str::parse::<i64>)
        .transpose()
        .context("历史分页位置无效")?;
    let page = context
        .desktop
        .messages(&context.task.task_id, before)
        .await?;
    let next = page
        .has_more
        .then(|| {
            page.messages
                .iter()
                .map(|m| m.id)
                .min()
                .map(|id| id.to_string())
        })
        .flatten();
    let mut items = Vec::new();
    for message in &page.messages {
        let mut attachments = Vec::new();
        for file in &message.files {
            if let Some(path) = file["file_path"].as_str() {
                let path = resolve_path(&context.directory, path);
                if let Some(file) = files::associate(
                    state,
                    scope,
                    &path,
                    if message.role == "user" {
                        "attachment"
                    } else {
                        "artifact"
                    },
                )
                .await?
                {
                    attachments.push(file);
                }
            }
        }
        items.push(json!({"id":message.id.to_string(),"role":if message.role == "system" {"assistant"} else {message.role.as_str()},
            "type":"message","text":message.content,"files":attachments}));
        let steps = history_steps(message);
        for (index, step) in steps.iter().enumerate() {
            items.push(json!({"id":step["id"].as_str().or_else(||step["call_id"].as_str()).map(str::to_owned).unwrap_or_else(||format!("{}-step-{index}",message.id)),"role":"tool","type":"tool",
                "text":step["content"].as_str().or_else(||step["result"].as_str()).unwrap_or_default(),"data":sanitize(step)}));
        }
    }
    if newest && let Ok(runs) = context.run.active(&scope.session).await {
        if let Some(run) = runs.first() {
            let row = page
                .messages
                .last()
                .filter(|m| m.role == "system")
                .map_or(0, |m| m.id);
            let baseline = run["last_seq"]
                .as_u64()
                .context("当前天工运行缺少事件续读序号，请升级客户端后观察在途任务")?;
            observe_run(
                state,
                scope,
                &context,
                run.clone(),
                Some(OwnedReply {
                    row,
                    text: String::new(),
                    steps: Vec::new(),
                    artifacts: Vec::new(),
                    last_saved: Instant::now(),
                    persist_to_native: false,
                    replay_until: baseline,
                    replaying: baseline > 0,
                    replay_pending: None,
                }),
            )
            .await?;
        }
    }
    let (status, turn_id, approvals, _) = state.remote_sessions.snapshot(scope).await;
    Ok(
        json!({"items":items,"nextCursor":next,"status":status,"turnId":turn_id,"approvals":approvals,"cursor":snapshot_cursor}),
    )
}
fn history_steps(message: &DesktopMessage) -> Vec<Value> {
    if let Some(steps) = &message.thinking_steps
        && !steps.is_empty()
    {
        return steps.clone();
    }
    message
        .execution
        .as_ref()
        .and_then(|execution| execution["blocks"].as_array())
        .into_iter()
        .flatten()
        .filter(|block| matches!(block["kind"].as_str(), Some("tool" | "reasoning")))
        .cloned()
        .collect()
}
pub(super) async fn send(state: &SharedState, scope: &Scope, input: &TurnRequest) -> Result<Value> {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let state = state.clone();
    let scope = scope.clone();
    let input = input.clone();
    tokio::spawn(async move {
        // This is the same semaphore IM uses. The worker, not the browser
        // request lifetime, retains admission through streaming and approvals.
        let permit = match tokio::time::timeout(
            Duration::from_secs(15),
            state.gmclaw_im.acquire_remote_execution(),
        )
        .await
        {
            Ok(Ok(p)) => p,
            _ => {
                let _ = ready_tx.send(Err(anyhow::anyhow!("天工正在执行其他任务，请稍后再试")));
                return;
            }
        };
        let prepared = prepare_turn(&state, &scope, &input).await;
        let (context, run, owned) = match prepared {
            Ok(value) => value,
            Err(error) => {
                let _ = ready_tx.send(Err(error));
                return;
            }
        };
        let turn_id = run["run_id"].as_str().unwrap_or_default().to_owned();
        let response = json!({"turnId":turn_id,"status":"running"});
        state
            .remote_sessions
            .publish(&scope, "turn.updated", None, response.clone())
            .await;
        {
            let mut feeds = state.remote_sessions.feeds.lock().await;
            feeds.entry(scope.clone()).or_default().observer_running = true;
        }
        let _ = ready_tx.send(Ok(response));
        run_worker(&state, &scope, context, run, Some(owned)).await;
        drop(permit);
    });
    ready_rx
        .await
        .context("天工执行准备已中断，请刷新核对结果")
        .map_err(super::unconfirmed)?
}
struct OwnedReply {
    row: i64,
    text: String,
    steps: Vec<Value>,
    artifacts: Vec<Value>,
    last_saved: Instant,
    persist_to_native: bool,
    replay_until: u64,
    replaying: bool,
    replay_pending: Option<(u64, Value)>,
}
async fn prepare_turn(
    state: &SharedState,
    scope: &Scope,
    input: &TurnRequest,
) -> Result<(ContextData, Value, OwnedReply)> {
    let mut context = context(state, &scope.session).await?;
    ensure!(
        context.run.active(&scope.session).await?.is_empty(),
        "天工原会话仍在执行或等待审批，请先完成或停止"
    );
    ensure!(
        input.effort.as_deref().is_none_or(|e| e.is_empty()),
        "当前天工运行接口未提供独立思考强度选择"
    );
    let model = context
        .task
        .model_id
        .as_deref()
        .filter(|m| !m.is_empty())
        .context("此天工任务缺少模型绑定，请先在原客户端选择模型")?;
    ensure!(
        input
            .model
            .as_deref()
            .filter(|m| !m.is_empty())
            .is_none_or(|m| m == model),
        "远程续聊沿用原会话模型，请先在天工切换模型后再续聊"
    );
    let attached = files::selected(state, scope, &input.attachment_ids).await?;
    let descriptors = attached
        .iter()
        .map(files::native_descriptor)
        .collect::<Vec<_>>();
    let chat_id = uuid::Uuid::new_v4().to_string();
    context.desktop = context.desktop.with_turn_context(&chat_id);
    files::mark_sent(state, scope, &input.attachment_ids).await?;
    let user_row = context
        .desktop
        .append_message_with_files(
            &context.task.task_id,
            "user",
            &input.text,
            &[],
            &descriptors,
        )
        .await
        .map_err(super::unconfirmed)?;
    state.remote_sessions.publish(scope,"item.updated",Some(user_row.to_string()),json!({"item":{
        "id":user_row.to_string(),"role":"user","type":"message","text":input.text,"files":attached.iter().map(|f|&f.public).collect::<Vec<_>>()}})).await;
    let assistant_row = context
        .desktop
        .append_message(&context.task.task_id, "system", "", &[])
        .await
        .map_err(super::unconfirmed)?;
    let original_options = context
        .task
        .model_options
        .get(model)
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut body = json!({"session_id":scope.session,"chat_id":chat_id,
        "user_id":context.metadata.user_id,"agent_id":context.metadata.agent_id,"project_id":context.metadata.project_id,"task_id":context.task.task_id,
        "query":input.text,"files":descriptors,"model_id":model,"model_options":original_options,
        "project":{"id":native_project_id(&context.metadata.project_id),"name":context.task.scenario_name,"path":context.directory},
        "max_steps":context.metadata.max_steps,"request_id":input.request_id});
    if let Some(revision) = context.desktop.scoped_revision() {
        body["conversation_revision"] = revision.into();
    }
    if context.task.scenario_id == "default" && !context.task.task_work_dir.is_empty() {
        body["project"]["task_workspace_id"] = context.task.task_id.as_str().into();
    }
    if let Some(mode) = context
        .metadata
        .extra_data
        .get("tool_execution_mode")
        .and_then(Value::as_str)
    {
        body["tool_execution_mode"] = mode.into();
    }
    let run = context.run.request(Method::POST, "", Some(body)).await;
    let run = match run {
        Ok(run) => run,
        Err(error) => {
            let _ = context
                .desktop
                .update_message(
                    &context.task.task_id,
                    assistant_row,
                    "请求结果未确认，请刷新运行状态后继续。",
                    &[],
                )
                .await;
            return Err(super::unconfirmed(error));
        }
    };
    validate_run(&run, &scope.session, None).map_err(super::unconfirmed)?;
    gmclaw_runtime::queue_desktop_refresh(
        &context.task.task_id,
        &scope.session,
        Some(assistant_row),
    )
    .await;
    Ok((
        context,
        run,
        OwnedReply {
            row: assistant_row,
            text: String::new(),
            steps: Vec::new(),
            artifacts: Vec::new(),
            last_saved: Instant::now(),
            persist_to_native: true,
            replay_until: 0,
            replaying: false,
            replay_pending: None,
        },
    ))
}
async fn observe_run(
    state: &SharedState,
    scope: &Scope,
    context: &ContextData,
    run: Value,
    owned: Option<OwnedReply>,
) -> Result<()> {
    validate_run(&run, &scope.session, None)?;
    let should_start = {
        let mut feeds = state.remote_sessions.feeds.lock().await;
        let feed = feeds.entry(scope.clone()).or_default();
        if feed.observer_running {
            false
        } else {
            feed.observer_running = true;
            true
        }
    };
    if !should_start {
        return Ok(());
    }
    let status = normalize_status(run["status"].as_str().unwrap_or("running"));
    state
        .remote_sessions
        .publish(
            scope,
            "turn.updated",
            None,
            json!({"status":status,"turnId":run["run_id"]}),
        )
        .await;
    let state = state.clone();
    let scope = scope.clone();
    let context = context.clone();
    tokio::spawn(async move {
        run_worker(&state, &scope, context, run, owned).await;
    });
    Ok(())
}
fn validate_run(run: &Value, session: &str, expected: Option<&str>) -> Result<()> {
    ensure!(
        run["run_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && expected.is_none_or(|original| id == original))
            && run["session_id"] == session,
        "天工运行返回的会话标识不一致，未订阅其他任务"
    );
    Ok(())
}
async fn run_worker(
    state: &SharedState,
    scope: &Scope,
    context: ContextData,
    mut run: Value,
    mut owned: Option<OwnedReply>,
) {
    let run_id = run["run_id"].as_str().unwrap_or_default().to_owned();
    let item_id = owned
        .as_ref()
        .filter(|o| o.row > 0)
        .map(|o| o.row.to_string())
        .unwrap_or_else(|| format!("run-{run_id}"));
    let mut after = 0u64;
    let mut failures = 0;
    loop {
        match stream_once(
            state, scope, &context, &run_id, &item_id, &mut after, &mut owned,
        )
        .await
        {
            Ok(done) if done => break,
            Ok(_) => {}
            Err(error) => {
                failures += 1;
                if failures >= 3 {
                    state.remote_sessions.publish(scope,"error",None,json!({"code":"event_connection_lost",
                        "message":format!("天工事件连接中断：{error}。任务可能仍在执行，重新连接只会续读事件。定向停止仍可使用。")})).await;
                }
            }
        }
        match context.run.request(Method::GET, &run_id, None).await {
            Ok(current) => {
                if validate_run(&current, &scope.session, Some(&run_id)).is_err() {
                    break;
                }
                run = current;
                let status = run["status"].as_str().unwrap_or("unknown");
                if terminal(status) {
                    state
                        .remote_sessions
                        .publish(
                            scope,
                            "turn.updated",
                            None,
                            json!({"status":normalize_status(status),"turnId":run_id}),
                        )
                        .await;
                    break;
                }
            }
            Err(_) if failures >= 5 => {
                state
                    .remote_sessions
                    .publish(
                        scope,
                        "turn.updated",
                        None,
                        json!({"status":"unknown","turnId":run_id}),
                    )
                    .await;
                break;
            }
            Err(_) => {}
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    if let Some(owned) = &mut owned {
        if let Err(error) = persist(&context, owned).await {
            state.remote_sessions.publish(scope,"error",None,json!({"code":"native_history_save_failed","message":format!("回复未能保存到天工原会话：{error}")})).await;
        }
        let completed = !super::types::busy(&state.remote_sessions.snapshot(scope).await.0);
        state.remote_sessions.publish(scope,"item.updated",Some(item_id.clone()),json!({"item":{"id":item_id,"role":"assistant",
            "type":"message","text":owned.text,"status":if completed {"completed"} else {"running"}},"completed":completed})).await;
        if owned.persist_to_native {
            gmclaw_runtime::queue_desktop_refresh(
                &context.task.task_id,
                &scope.session,
                Some(owned.row),
            )
            .await;
        }
    }
    if let Some(feed) = state.remote_sessions.feeds.lock().await.get_mut(scope) {
        feed.observer_running = false;
    }
    state
        .remote_sessions
        .publish(
            scope,
            "reset",
            None,
            json!({"reason":"native_history_updated"}),
        )
        .await;
}
async fn stream_once(
    state: &SharedState,
    scope: &Scope,
    context: &ContextData,
    run_id: &str,
    item_id: &str,
    after: &mut u64,
    owned: &mut Option<OwnedReply>,
) -> Result<bool> {
    let mut url = context.run.url(&format!("{run_id}/events"))?;
    url.query_pairs_mut()
        .append_pair("after_seq", &after.to_string());
    let response = context
        .run
        .http
        .get(url)
        .header(
            reqwest::header::AUTHORIZATION,
            context.run.authorization.clone(),
        )
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "天工事件接口返回 HTTP {}",
        response.status().as_u16()
    );
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    while let Some(chunk) = stream.next().await {
        buffer.extend_from_slice(&chunk?);
        ensure!(buffer.len() <= 4 * 1024 * 1024, "天工事件超过读取范围");
        while let Some(position) = buffer.iter().position(|b| *b == b'\n') {
            let line = buffer.drain(..=position).collect::<Vec<_>>();
            if process_line(state, scope, context, run_id, item_id, &line, after, owned).await? {
                return Ok(true);
            }
        }
    }
    if !buffer.is_empty()
        && process_line(
            state, scope, context, run_id, item_id, &buffer, after, owned,
        )
        .await?
    {
        return Ok(true);
    }
    Ok(false)
}
async fn process_line(
    state: &SharedState,
    scope: &Scope,
    context: &ContextData,
    run_id: &str,
    item_id: &str,
    line: &[u8],
    after: &mut u64,
    owned: &mut Option<OwnedReply>,
) -> Result<bool> {
    let line = std::str::from_utf8(line)?.trim();
    if line.is_empty() || line.starts_with(':') {
        return Ok(false);
    }
    let (native_kind, payload) = line.split_once('|').context("天工事件格式不兼容")?;
    let data: Value = serde_json::from_str(payload)?;
    let Some(seq) = checked_event(&data, run_id, &scope.session, *after)? else {
        return Ok(false);
    };
    // Work 1.1 wraps child frames in the parent's run/session/seq identity.
    // Consume their sequence without treating child text or confirmations as
    // the parent's reply or approval.
    let child = ["subagent_run_id", "child_session_id"].iter().any(|key| {
        data.get(*key)
            .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
    });
    let kind = if child { "subagent" } else { native_kind };
    if let Some(observed) = owned.as_mut().filter(|o| o.replaying) {
        if seq <= observed.replay_until {
            // An already-running desktop task is replayed only into this
            // in-memory prefix, not appended again to native rows or the UI.
            match kind {
                "text" => observed
                    .text
                    .push_str(data["content"].as_str().unwrap_or_default()),
                "over" => {
                    if let Some(answer) = data["answer"].as_str().filter(|a| !a.is_empty()) {
                        observed.text = answer.into();
                    }
                }
                "confirm" => observed.replay_pending = Some((seq, data.clone())),
                "run_status" if data["status"] == "waiting" => {
                    observed.replay_pending = Some((seq, data.clone()))
                }
                "run_status" if data["status"] != "waiting_confirmation" => {
                    observed.replay_pending = None
                }
                _ => {}
            }
            *after = seq;
            if seq >= observed.replay_until {
                observed.replaying = false;
                if let Some((pending_seq, pending)) = observed.replay_pending.take() {
                    if pending["status"] == "waiting" {
                        publish_waiting(state, scope, context, run_id, pending_seq).await;
                    } else {
                        publish_confirmation(state, scope, context, run_id, pending_seq, &pending)
                            .await?;
                    }
                }
                state
                    .remote_sessions
                    .publish(
                        scope,
                        "item.updated",
                        Some(item_id.into()),
                        json!({"item":{"id":item_id,"role":"assistant",
                    "type":"message","text":observed.text,"status":"running"},"completed":false}),
                    )
                    .await;
            }
            return Ok(false);
        }
        observed.replaying = false;
    }
    // Consume a sequence only after its effect has been applied. Reconnects
    // subscribe after this exact cursor and never call create/resume again.
    match kind {
        "text"=>{
            let text = data["content"].as_str().unwrap_or_default();
            if let Some(owned) = owned.as_mut() {owned.text.push_str(text);}
            state.remote_sessions.publish(scope,"message.delta",Some(item_id.into()),json!({"text":text,"role":"assistant","type":"message"})).await;
        }
        "over"=>{
            if let Some(owned) = owned.as_mut() {
                if let Some(answer) = data["answer"].as_str().filter(|a|!a.is_empty()) {owned.text = answer.into();}
            }
            if let Some(error) = data["error"].as_str().filter(|e|!e.is_empty()) {
                state.remote_sessions.publish(scope,"error",None,json!({"code":"native_run_error","message":error})).await;
            }
        }
        "confirm"=>{
            publish_confirmation(state,scope,context,run_id,seq,&data).await?;
        }
        "subagent" if native_kind == "confirm"=>{
            state.remote_sessions.publish(scope,"error",None,json!({"code":"child_approval_on_desktop","message":"子 Agent 需要确认，请在目标天工窗口处理。"})).await;
        }
        "run_status"=>{
            let native_status = data["status"].as_str().unwrap_or("unknown");
            let status = normalize_status(native_status);
            if terminal(native_status) {
                *after=seq;
                if let Some(owned) = owned.as_mut() {
                    persist(context,owned).await?;
                    state.remote_sessions.publish(scope,"item.updated",Some(item_id.into()),json!({"item":{"id":item_id,"role":"assistant",
                        "type":"message","text":owned.text,"status":"completed"},"completed":true})).await;
                }
                state.remote_sessions.publish(scope,"turn.updated",None,json!({"status":status,"turnId":run_id})).await;
                return Ok(true);
            }
            if native_status == "running" {
                let ids=state.remote_sessions.feeds.lock().await.get(scope).map(|f|f.approvals.iter().filter_map(|(id,p)|
                    matches!(&p.source,ApprovalSource::Tiangong {run_id:original,..} if original == run_id).then(||id.clone())).collect::<Vec<_>>()).unwrap_or_default();
                for id in ids {state.remote_sessions.resolve_approval(scope,&id).await;}
            }
            if native_status == "waiting" {publish_waiting(state,scope,context,run_id,seq).await;}
            state.remote_sessions.publish(scope,"turn.updated",None,json!({"status":status,"turnId":run_id})).await;
        }
        "think"|"tool_start"|"tool_input"|"tool_end"|"tool_error"|"file_read"|"file_write"|"cmd_end"=>{
            let id = data["call_id"].as_str().or_else(||data["tool_id"].as_str()).map(str::to_owned).unwrap_or_else(||format!("{run_id}-{seq}"));
            let mut attached = Vec::new();
            if kind == "file_write" && data["success"] != false && let Some(path) = data["path"].as_str() {
                let path = resolve_path(&context.directory,path);
                if let Some(file) = files::associate(state,scope,&path,"artifact").await? {
                    if let Some(owned) = owned.as_mut() {
                        let stored = files::lookup(state,scope,&file.id).await?;
                        let descriptor = files::native_descriptor(&stored);
                        if !owned.artifacts.iter().any(|f|f["file_path"] == descriptor["file_path"]) {owned.artifacts.push(descriptor);}
                    }
                    attached.push(file);
                }
            }
            let text = data["content"].as_str().or_else(||data["output"].as_str()).or_else(||data["result"].as_str()).unwrap_or_default();
            let public = sanitize(&data);
            if let Some(owned) = owned.as_mut() {
                let step = json!({"id":id,"type":kind,"content":text,"tool_name":data["tool_name"],"data":public});
                if let Some(previous) = owned.steps.iter_mut().find(|s|s["id"] == id) {*previous=step;} else {owned.steps.push(step);}
            }
            state.remote_sessions.publish(scope,"item.updated",Some(id.clone()),json!({"item":{"id":id,"role":"tool",
                "type":if kind == "file_write" {"fileChange"} else if kind == "think" {"reasoning"} else {"tool"},
                "text":text,"files":attached,"status":if matches!(kind,"tool_start"|"tool_input") {"running"} else {"completed"},"data":public},
                "completed":!matches!(kind,"tool_start"|"tool_input")})).await;
        }
        "error"=>state.remote_sessions.publish(scope,"error",None,json!({"code":"native_error",
            "message":data["message"].as_str().or_else(||data["error"].as_str()).unwrap_or("天工执行出错，请查看原会话")})).await,
        _=>{}
    }
    *after = seq;
    if let Some(owned) = owned.as_mut() {
        if owned.last_saved.elapsed() >= Duration::from_secs(1) {
            persist(context, owned).await?;
        }
    }
    Ok(false)
}
fn checked_event(data: &Value, run: &str, session: &str, after: u64) -> Result<Option<u64>> {
    // Child runs remain distinct. Never approve or append their events to the
    // parent just because they share a browser subscription.
    if data["run_id"].as_str() != Some(run) {
        return Ok(None);
    }
    ensure!(
        data["session_id"].as_str().is_none_or(|id| id == session),
        "天工事件所属会话已变化"
    );
    let seq = data["seq"].as_u64().context("天工事件缺少续读序号")?;
    Ok((seq > after).then_some(seq))
}
async fn persist(context: &ContextData, owned: &mut OwnedReply) -> Result<()> {
    if !owned.persist_to_native {
        owned.last_saved = Instant::now();
        return Ok(());
    }
    context
        .desktop
        .update_message_with_files(
            &context.task.task_id,
            owned.row,
            &owned.text,
            &owned.steps,
            &owned.artifacts,
        )
        .await?;
    owned.last_saved = Instant::now();
    Ok(())
}
async fn publish_confirmation(
    state: &SharedState,
    scope: &Scope,
    context: &ContextData,
    run_id: &str,
    seq: u64,
    data: &Value,
) -> Result<()> {
    let call_ids = data["pending_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["call_id"].as_str())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ensure!(!call_ids.is_empty(), "天工确认事件缺少工具调用标识");
    let id = format!("tiangong-{run_id}-{seq}");
    let public = json!({"id":id,"type":"approval","title":"天工需要确认",
        "message":data["prompt"].as_str().or_else(||data["content"].as_str()).unwrap_or("允许执行当前工具操作？"),
        "choices":[{"value":"accept","label":"允许"},{"value":"decline","label":"拒绝"}]});
    state
        .remote_sessions
        .add_approval(
            scope,
            id,
            public,
            ApprovalSource::Tiangong {
                run_id: run_id.into(),
                call_ids,
                authorization_identity: context.run.identity.clone(),
            },
        )
        .await;
    Ok(())
}
async fn publish_waiting(
    state: &SharedState,
    scope: &Scope,
    context: &ContextData,
    run: &str,
    seq: u64,
) {
    let id = format!("tiangong-{run}-waiting-{seq}");
    state.remote_sessions.add_approval(scope,id.clone(),json!({"id":id,"type":"approval","title":"任务已暂停",
        "message":"继续原任务，或停止这次执行？","choices":[{"value":"accept","label":"继续"},{"value":"decline","label":"停止"}]}),
        ApprovalSource::Tiangong {run_id:run.into(),call_ids:Vec::new(),authorization_identity:context.run.identity.clone()}).await;
}
fn normalize_status(status: &str) -> &str {
    if status == "waiting" {
        "waiting_approval"
    } else {
        super::codex::normalize_status(status)
    }
}
fn terminal(status: &str) -> bool {
    matches!(
        status,
        "completed" | "stopped" | "failed" | "interrupted" | "cancelled"
    )
}
fn resolve_path(directory: &str, path: &str) -> PathBuf {
    if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        Path::new(directory).join(path)
    }
}
fn sanitize(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.to_ascii_lowercase().as_str(),
                        "authorization"
                            | "auth_token"
                            | "token"
                            | "access_token"
                            | "refresh_token"
                            | "api_key"
                            | "apikey"
                            | "password"
                            | "secret"
                    )
                })
                .map(|(key, value)| (key.clone(), sanitize(value)))
                .collect(),
        ),
        Value::Array(values) => values.iter().map(sanitize).collect(),
        other => other.clone(),
    }
}
pub(super) async fn stop(
    state: &SharedState,
    scope: &Scope,
    requested: Option<&str>,
) -> Result<Value> {
    let context = context(state, &scope.session).await?;
    let runs = context.run.active(&scope.session).await?;
    let run = runs
        .iter()
        .find(|run| requested.is_none_or(|id| run["run_id"] == id))
        .context("当前会话没有匹配的运行任务")?;
    let id = run["run_id"].as_str().context("天工运行标识无效")?;
    let mut result = context
        .run
        .request(Method::POST, &format!("{id}/stop"), Some(json!({})))
        .await?;
    for _ in 0..20 {
        validate_run(&result, &scope.session, Some(id))?;
        let status = result["status"].as_str().unwrap_or("unknown");
        if terminal(status) {
            state
                .remote_sessions
                .publish(
                    scope,
                    "turn.updated",
                    None,
                    json!({"status":normalize_status(status),"turnId":id}),
                )
                .await;
            return Ok(json!({"status":normalize_status(status)}));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        result = context.run.request(Method::GET, id, None).await?;
    }
    state
        .remote_sessions
        .publish(
            scope,
            "turn.updated",
            None,
            json!({"status":"stopping","turnId":id}),
        )
        .await;
    Ok(json!({"status":"stopping"}))
}
pub(super) async fn approve(
    state: &SharedState,
    scope: &Scope,
    id: &str,
    decision: &ApprovalDecision,
    run_id: &str,
    call_ids: &[String],
    identity: &str,
) -> Result<()> {
    let context = context(state, &scope.session).await?;
    ensure!(
        context.run.identity == identity,
        "天工运行授权已变化，原确认不再可用，请刷新会话"
    );
    let run = context.run.request(Method::GET, run_id, None).await?;
    validate_run(&run, &scope.session, Some(run_id))?;
    let (operation, body) = approval_control(&run, decision, call_ids)?;
    let result = context
        .run
        .request(Method::POST, &format!("{run_id}/{operation}"), Some(body))
        .await
        .map_err(super::unconfirmed)?;
    validate_run(&result, &scope.session, Some(run_id)).map_err(super::unconfirmed)?;
    state.remote_sessions.resolve_approval(scope, id).await;
    state.remote_sessions.publish(scope,"turn.updated",None,json!({"status":normalize_status(result["status"].as_str().unwrap_or("running")),"turnId":run_id})).await;
    Ok(())
}
fn approval_control(
    run: &Value,
    decision: &ApprovalDecision,
    call_ids: &[String],
) -> Result<(&'static str, Value)> {
    let action = match decision.decision.as_str() {
        "accept" => "approve",
        "decline" | "cancel" => "deny",
        _ => anyhow::bail!("确认选项无效"),
    };
    let control = if call_ids.is_empty() {
        ensure!(
            run["status"] == "waiting",
            "原天工任务已不在等待接续，请刷新会话"
        );
        if action == "approve" {
            ("resume", json!({"resume_action":"continue","query":""}))
        } else {
            ("stop", json!({}))
        }
    } else {
        ensure!(
            run["status"] == "waiting_confirmation",
            "天工任务已不在等待这项确认，请刷新会话"
        );
        let current = run["pending_calls"]
            .as_array()
            .context("天工未返回当前待确认工具，请刷新任务")?;
        ensure!(
            call_ids
                .iter()
                .all(|id| current.iter().any(|call| call["call_id"] == *id)),
            "原工具确认已变化，请刷新任务；本次未提交"
        );
        (
            "resume",
            json!({"confirm_action":action,"approved_call_ids":call_ids,"approval_scope":"operation"}),
        )
    };
    Ok(control)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn work_v110_project_selection_keeps_id_and_rejects_ambiguous_directories() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_string_lossy().into_owned();
        let projects = vec![
            json!({"id":"first-shell","path":path}),
            json!({"id":"chosen-shell","path":path}),
        ];
        let mut options = CreateRequest {
            project_id: Some("chosen-shell".into()),
            cwd: Some(path),
            ..Default::default()
        };
        assert_eq!(
            selected_project(&projects, &options).unwrap()["id"],
            "chosen-shell"
        );
        options.project_id = Some("removed".into());
        assert!(selected_project(&projects, &options).is_err());
        options.project_id = None;
        assert!(selected_project(&projects, &options).is_err());
        assert_eq!(
            selected_project(&projects[1..], &options).unwrap()["id"],
            "chosen-shell"
        );
        assert_eq!(native_project_id("default"), "0");
        assert_eq!(native_project_id("chosen-shell"), "chosen-shell");
    }

    #[test]
    fn work_v110_controls_continue_original_checkpoint_and_reject_stale_tool_ids() {
        let decision = ApprovalDecision {
            decision: "accept".into(),
            answers: Value::Null,
        };
        let (path, body) = approval_control(&json!({"status":"waiting"}), &decision, &[]).unwrap();
        assert_eq!(path, "resume");
        assert_eq!(body, json!({"resume_action":"continue","query":""}));
        let deny = ApprovalDecision {
            decision: "decline".into(),
            answers: Value::Null,
        };
        assert_eq!(
            approval_control(&json!({"status":"waiting"}), &deny, &[])
                .unwrap()
                .0,
            "stop"
        );
        let approval =
            json!({"status":"waiting_confirmation","pending_calls":[{"call_id":"current"}]});
        assert!(approval_control(&approval, &decision, &["old".into()]).is_err());
        let (_, body) = approval_control(&approval, &decision, &["current".into()]).unwrap();
        assert_eq!(body["approval_scope"], "operation");
    }

    #[test]
    fn work_v110_history_reads_native_execution_blocks() {
        let message:DesktopMessage=serde_json::from_value(json!({"id":1,"task_id":"task","role":"system","content":"answer",
            "thinking_steps":null,"execution":{"version":1,"blocks":[{"id":"tool-one","kind":"tool","content":"done"},{"id":"text-one","kind":"text","content":"answer"}]}})).unwrap();
        let steps = history_steps(&message);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0]["id"], "tool-one");
    }

    #[tokio::test]
    async fn work_v110_wrapped_child_frames_do_not_replace_parent_or_approve_child() {
        let (directory, state) = super::super::test_state();
        let context = context_fixture(directory.path());
        let scope = Scope::new("tiangong", "session");
        state.remote_sessions.watch(&scope, None).await;
        let mut owned = Some(observed(12, 0));
        let mut after = 0;
        for (seq, kind, payload) in [
            (1, "text", json!({"content":"parent"})),
            (
                2,
                "over",
                json!({"answer":"child","subagent_run_id":"child-run"}),
            ),
            (
                3,
                "confirm",
                json!({"pending_calls":[{"call_id":"child-call"}],"child_session_id":"child"}),
            ),
            (4, "run_status", json!({"status":"waiting"})),
        ] {
            let mut data = payload;
            data["run_id"] = "run".into();
            data["session_id"] = "session".into();
            data["seq"] = seq.into();
            process_line(
                &state,
                &scope,
                &context,
                "run",
                "12",
                format!("{kind}|{data}").as_bytes(),
                &mut after,
                &mut owned,
            )
            .await
            .unwrap();
        }
        assert_eq!(owned.unwrap().text, "parent");
        assert_eq!(after, 4);
        let (status, _, approvals, _) = state.remote_sessions.snapshot(&scope).await;
        assert_eq!(status, "waiting_approval");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0]["choices"][0]["label"], "继续");
        let feeds = state.remote_sessions.feeds.lock().await;
        assert!(
            feeds[&scope]
                .events
                .iter()
                .any(|e| e.kind == "error" && e.data["code"] == "child_approval_on_desktop")
        );
    }
    #[test]
    fn native_cursor_replay_and_child_run_do_not_duplicate_parent() {
        let event = json!({"run_id":"run-a","session_id":"session-a","seq":4});
        assert_eq!(
            checked_event(&event, "run-a", "session-a", 3).unwrap(),
            Some(4)
        );
        assert_eq!(
            checked_event(&event, "run-a", "session-a", 4).unwrap(),
            None
        );
        assert_eq!(
            checked_event(&event, "parent", "session-a", 0).unwrap(),
            None
        );
        assert!(checked_event(&event, "run-a", "other", 0).is_err());
    }
    #[test]
    fn progress_boundary_removes_runtime_credentials() {
        let public = sanitize(
            &json!({"tool_name":"run","arguments":{"command":"echo hi","api_key":"fixture"},"authorization":"fixture"}),
        );
        assert_eq!(public["arguments"]["command"], "echo hi");
        assert!(public.get("authorization").is_none());
        assert!(public["arguments"].get("api_key").is_none());
    }
    fn context_fixture(directory: &Path) -> ContextData {
        ContextData {
            desktop: DesktopClient::fixture("http://127.0.0.1:1"),
            run: RunClient::new(
                crate::gmclaw_executor::DEFAULT_ENDPOINT,
                HeaderValue::from_static("Bearer synthetic-fixture"),
            )
            .unwrap(),
            task: DesktopTask {
                task_id: "task".into(),
                session_id: "session".into(),
                scenario_id: "project".into(),
                scenario_name: "project".into(),
                work_dir: directory.to_string_lossy().into_owned(),
                title: "fixture".into(),
                created_at: String::new(),
                updated_at: String::new(),
                completed_at: None,
                model_id: Some("model".into()),
                owner_id: Some("original-account".into()),
                task_work_dir: String::new(),
                model_options: json!({}),
            },
            metadata: DesktopSessionMetadata {
                session_id: "session".into(),
                user_id: "original-account".into(),
                project_id: "project".into(),
                ..Default::default()
            },
            directory: directory.to_string_lossy().into_owned(),
        }
    }
    fn observed(row: i64, baseline: u64) -> OwnedReply {
        OwnedReply {
            row,
            text: String::new(),
            steps: Vec::new(),
            artifacts: Vec::new(),
            last_saved: Instant::now(),
            persist_to_native: false,
            replay_until: baseline,
            replaying: baseline > 0,
            replay_pending: None,
        }
    }
    #[tokio::test]
    async fn opening_inflight_native_task_does_not_append_old_deltas_or_write_native_rows() {
        let (directory, state) = super::super::test_state();
        let context = context_fixture(directory.path());
        let scope = Scope::new("tiangong", "session");
        state.remote_sessions.watch(&scope, None).await;
        let mut owned = Some(observed(12, 3));
        let mut after = 0;
        for (seq, text) in [(1, "hello"), (2, " world")] {
            process_line(
                &state,
                &scope,
                &context,
                "run",
                "12",
                format!(
                    "text|{}",
                    json!({"run_id":"run","session_id":"session","seq":seq,"content":text})
                )
                .as_bytes(),
                &mut after,
                &mut owned,
            )
            .await
            .unwrap();
        }
        assert_eq!(state.remote_sessions.snapshot(&scope).await.3, 0);
        process_line(
            &state,
            &scope,
            &context,
            "run",
            "12",
            br#"run_status|{"run_id":"run","session_id":"session","seq":3,"status":"running"}"#,
            &mut after,
            &mut owned,
        )
        .await
        .unwrap();
        let events = state.remote_sessions.feeds.lock().await[&scope]
            .events
            .clone();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "item.updated");
        assert_eq!(events[0].data["item"]["text"], "hello world");
        process_line(
            &state,
            &scope,
            &context,
            "run",
            "12",
            br#"text|{"run_id":"run","session_id":"session","seq":4,"content":"!"}"#,
            &mut after,
            &mut owned,
        )
        .await
        .unwrap();
        assert!(process_line(&state,&scope,&context,"run","12",br#"run_status|{"run_id":"run","session_id":"session","seq":5,"status":"completed"}"#,&mut after,&mut owned).await.unwrap());
        let feeds = state.remote_sessions.feeds.lock().await;
        let final_item = feeds[&scope]
            .events
            .iter()
            .find(|e| e.kind == "item.updated" && e.data["completed"] == true)
            .unwrap();
        assert_eq!(final_item.data["item"]["text"], "hello world!");
        assert_eq!(feeds[&scope].status, "completed");
        assert_eq!(context.metadata.user_id, "original-account");
    }
    #[tokio::test]
    async fn replay_restores_only_live_native_confirmation_and_original_run() {
        let (directory, state) = super::super::test_state();
        let context = context_fixture(directory.path());
        let scope = Scope::new("tiangong", "session");
        state.remote_sessions.watch(&scope, None).await;
        let mut owned = Some(observed(12, 2));
        let mut after = 0;
        process_line(&state,&scope,&context,"run","12",br#"confirm|{"run_id":"run","session_id":"session","seq":1,"pending_calls":[{"call_id":"call"}]}"#,&mut after,&mut owned).await.unwrap();
        assert!(state.remote_sessions.snapshot(&scope).await.2.is_empty());
        process_line(&state,&scope,&context,"run","12",br#"run_status|{"run_id":"run","session_id":"session","seq":2,"status":"waiting_confirmation"}"#,&mut after,&mut owned).await.unwrap();
        assert_eq!(state.remote_sessions.snapshot(&scope).await.2.len(), 1);
        process_line(
            &state,
            &scope,
            &context,
            "run",
            "12",
            br#"run_status|{"run_id":"run","session_id":"session","seq":3,"status":"running"}"#,
            &mut after,
            &mut owned,
        )
        .await
        .unwrap();
        assert!(state.remote_sessions.snapshot(&scope).await.2.is_empty());
    }
}
