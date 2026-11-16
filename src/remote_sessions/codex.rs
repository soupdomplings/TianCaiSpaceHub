use super::{
    files,
    service::ApprovalSource,
    types::{ApprovalDecision, CreateRequest, PageQuery, Scope, TurnRequest},
};
use crate::{
    app_state::{RemoteControlSourceKind, SharedState},
    remote_control_backend,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CodexTarget {
    pub id: String,
    pub epoch: u64,
    pub key: String,
    pub client_id: String,
    pub stream_id: String,
    pub name: String,
}

pub(super) async fn targets(state: &SharedState) -> Vec<CodexTarget> {
    let remote = state.remote_control.inner.lock().await;
    let mut targets = Vec::new();
    for connection in remote.connections.values().filter(|c| {
        c.connected
            && c.initialized
            && c.outbound_tx.is_some()
            && c.source_kind == RemoteControlSourceKind::CodexApp
    }) {
        if let Some(client) = remote
            .clients
            .get(&connection.default_client_key)
            .filter(|c| c.initialized)
        {
            targets.push(CodexTarget {
                id: format!("codex:{}", connection.connection_id),
                epoch: connection.connection_epoch,
                key: connection.default_client_key.clone(),
                client_id: client.client_id.clone(),
                stream_id: client.stream_id.clone(),
                name: connection
                    .server_name
                    .clone()
                    .unwrap_or_else(|| "Codex".into()),
            });
        }
    }
    if remote.connections.is_empty()
        && remote.connected
        && remote.initialized
        && remote.outbound_tx.is_some()
    {
        targets.push(CodexTarget {
            id: "codex:default".into(),
            epoch: remote.connection_epoch,
            key: "default".into(),
            client_id: remote.client_id.clone(),
            stream_id: remote.stream_id.clone(),
            name: "Codex".into(),
        });
    }
    targets.sort_by(|a, b| a.id.cmp(&b.id));
    targets
}
pub(super) async fn target(state: &SharedState, id: &str) -> Result<CodexTarget> {
    targets(state)
        .await
        .into_iter()
        .find(|t| t.id == id)
        .context("此 Codex 连接已断开，请在目标电脑启动并接入 Hub 后重试")
}
async fn request(
    state: &SharedState,
    target: &CodexTarget,
    method: &str,
    params: Value,
) -> Result<Value> {
    remote_control_backend::request_for_browser(state, target.epoch, &target.key, method, params)
        .await
}
pub(super) async fn models(state: &SharedState, target: &CodexTarget) -> Result<Vec<Value>> {
    let response = request(
        state,
        target,
        "model/list",
        json!({"includeHidden":false,"limit":100}),
    )
    .await?;
    Ok(response["data"].as_array().context("Codex 模型目录格式不兼容")?.iter().filter_map(|model| {
        let id = model["id"].as_str().or_else(||model["model"].as_str())?;
        let efforts = model["supportedReasoningEfforts"].as_array().into_iter().flatten()
            .filter_map(|e|e["reasoningEffort"].as_str().or_else(||e.as_str())).collect::<Vec<_>>();
        Some(json!({"id":id,"name":model["displayName"].as_str().unwrap_or(id),"efforts":efforts}))
    }).collect())
}
pub(super) async fn clients(state: &SharedState) -> Vec<Value> {
    let mut result = Vec::new();
    for target in targets(state).await {
        let (models, reason) = match models(state, &target).await {
            Ok(m) => (m, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        result.push(json!({"id":target.id,"kind":"codex","name":target.name,"ready":true,"reason":reason,
            "capabilities":{"history":true,"create":true,"send":true,"stop":true,"approvals":true,"questions":true,
                "attachments":true,"artifacts":true,"modelSelection":!models.is_empty(),"effortSelection":models.iter().any(|m|m["efforts"].as_array().is_some_and(|e|!e.is_empty()))},"models":models}));
    }
    if result.is_empty() {
        result.push(
            json!({"id":"codex","kind":"codex","name":"Codex","ready":false,
        "reason":"请在目标电脑启动 Codex 并接入 Hub","capabilities":{},"models":[]}),
        );
    }
    result
}
pub(super) async fn sessions(state: &SharedState, client: &str, page: &PageQuery) -> Result<Value> {
    let target = target(state, client).await?;
    let mut params =
        json!({"sortKey":"updated_at","archived":false,"limit":page.limit(),"useStateDbOnly":true});
    if let Some(cursor) = &page.cursor {
        params["cursor"] = cursor.as_str().into();
    }
    let response = request(state, &target, "thread/list", params).await?;
    let rows = response["data"]
        .as_array()
        .context("Codex 会话列表格式不兼容")?;
    Ok(
        json!({"sessions":rows.iter().map(session_row).collect::<Vec<_>>(),"nextCursor":response["nextCursor"]}),
    )
}
fn session_row(thread: &Value) -> Value {
    json!({"id":thread["id"],"title":thread["name"].as_str().or_else(||thread["preview"].as_str()).filter(|s|!s.is_empty()).unwrap_or("新会话"),
        "cwd":thread["cwd"],"projectId":thread["cwd"],"updatedAt":thread["updatedAt"],
        "model":thread["model"],"status":thread_status(thread)})
}
pub(super) async fn projects(
    state: &SharedState,
    client: &str,
    query: &PageQuery,
) -> Result<Value> {
    let page = sessions(state, client, query).await?;
    let mut seen = HashSet::new();
    Ok(
        json!({"projects":page["sessions"].as_array().into_iter().flatten().filter_map(|s| {
        let path = s["cwd"].as_str().filter(|s|!s.is_empty())?;
        seen.insert(path.to_owned()).then(|| json!({"id":path,"path":path,"name":Path::new(path).file_name().map(|s|s.to_string_lossy()).unwrap_or_else(||path.into())}))
    }).collect::<Vec<_>>(),"nextCursor":page["nextCursor"]}),
    )
}
pub(super) async fn create(
    state: &SharedState,
    client: &str,
    options: &CreateRequest,
) -> Result<Value> {
    let target = target(state, client).await?;
    let cwd = options
        .cwd
        .as_deref()
        .or(options.project_id.as_deref())
        .context("请选择目标电脑上的项目目录")?;
    ensure!(
        Path::new(cwd).is_absolute() && Path::new(cwd).is_dir(),
        "项目目录必须是目标电脑上已存在的绝对目录"
    );
    let mut params = remote_control_backend::ThreadStartOptions {
        cwd: Some(cwd.into()),
        ..Default::default()
    }
    .to_params();
    if let Some(model) = options.model.as_deref().filter(|m| !m.is_empty()) {
        ensure!(
            models(state, &target)
                .await?
                .iter()
                .any(|m| m["id"] == model),
            "所选模型不在 Codex 当前模型目录中"
        );
        params["model"] = model.into();
    }
    let response = request(state, &target, "thread/start", params).await?;
    let thread = response.get("thread").context("Codex 未返回新会话")?;
    let session = thread["id"].as_str().context("Codex 未返回新会话 ID")?;
    let scope = Scope::new(client, session);
    state.remote_sessions.watch(&scope, Some(target)).await;
    // Naming is a separate, idempotent native operation. Failure does not
    // replay thread/start or replace the successfully created conversation.
    if let Some(title) = options.title.as_deref().filter(|s| !s.trim().is_empty()) {
        let target = self::target(state, client).await?;
        let _ = request(
            state,
            &target,
            "thread/name/set",
            json!({"threadId":session,"name":title}),
        )
        .await;
    }
    Ok(json!({"session":session_row(thread)}))
}
async fn read_thread(state: &SharedState, target: &CodexTarget, session: &str) -> Result<Value> {
    let response = request(
        state,
        target,
        "thread/read",
        json!({"threadId":session,"includeTurns":false}),
    )
    .await?;
    let thread = response.get("thread").context("Codex 未返回会话内容")?;
    ensure!(
        thread["id"].as_str() == Some(session),
        "Codex 返回了其他会话，已停止读取"
    );
    Ok(thread.clone())
}
pub(super) async fn history(state: &SharedState, scope: &Scope, page: &PageQuery) -> Result<Value> {
    let target = target(state, &scope.client).await?;
    state
        .remote_sessions
        .watch(scope, Some(target.clone()))
        .await;
    let snapshot_cursor = state.remote_sessions.snapshot(scope).await.3;
    let thread = read_thread(state, &target, &scope.session).await?;
    let subscribe = state
        .remote_sessions
        .feeds
        .lock()
        .await
        .get(scope)
        .is_some_and(|f| !f.subscribed);
    if subscribe {
        // Native resume establishes subscriptions and replays live requests;
        // the pinned request deliberately does not set IM current_thread.
        request(
            state,
            &target,
            "thread/resume",
            json!({"threadId":scope.session,"excludeTurns":true}),
        )
        .await?;
        if let Some(feed) = state.remote_sessions.feeds.lock().await.get_mut(scope) {
            feed.subscribed = true;
        }
    }
    let (turns, next_cursor) = turn_page(state, &target, &scope.session, page).await?;
    let mut items = Vec::new();
    for turn in turns.iter().rev() {
        for item in turn["items"].as_array().into_iter().flatten() {
            items.push(normalize_item(state, scope, item).await?);
        }
    }
    let (mut status, mut turn_id, approvals, _) = state.remote_sessions.snapshot(scope).await;
    if page.cursor.is_none()
        && let Some(last) = turns.first()
    {
        let native_status = last["status"].as_str().unwrap_or("unknown");
        let native_turn = last["id"].as_str();
        if matches!(native_status, "inProgress" | "in_progress" | "running")
            || native_turn == turn_id.as_deref()
        {
            status = normalize_status(native_status).into();
            turn_id = native_turn.map(str::to_owned);
            state
                .remote_sessions
                .publish(
                    scope,
                    "turn.updated",
                    None,
                    json!({"status":status,"turnId":turn_id}),
                )
                .await;
        }
    }
    if status == "idle" && thread_status(&thread) == "running" {
        status = "running".into();
    }
    Ok(json!({"items":items,"nextCursor":next_cursor,
        "turnId":turn_id,"status":status,"approvals":approvals,"cursor":snapshot_cursor}))
}
async fn turn_page(
    state: &SharedState,
    target: &CodexTarget,
    session: &str,
    page: &PageQuery,
) -> Result<(Vec<Value>, Option<String>)> {
    let mut params =
        json!({"threadId":session,"limit":page.limit(),"sortDirection":"desc","itemsView":"full"});
    if let Some(cursor) = &page.cursor {
        params["cursor"] = cursor.clone().into();
    }
    match request(state, target, "thread/turns/list", params).await {
        Ok(response) => Ok((
            response["data"]
                .as_array()
                .context("Codex 历史分页格式不兼容")?
                .clone(),
            response["nextCursor"].as_str().map(str::to_owned),
        )),
        Err(error) if unsupported_method(&error) => {
            let response = request(
                state,
                target,
                "thread/read",
                json!({"threadId":session,"includeTurns":true}),
            )
            .await?;
            let turns = response
                .pointer("/thread/turns")
                .and_then(Value::as_array)
                .context("此 Codex 版本未提供可读取的历史")?;
            let before = page
                .cursor
                .as_deref()
                .map(str::parse::<usize>)
                .transpose()
                .context("旧版历史分页位置无效")?
                .unwrap_or(turns.len())
                .min(turns.len());
            let start = before.saturating_sub(page.limit());
            Ok((
                turns[start..before].iter().rev().cloned().collect(),
                (start > 0).then(|| start.to_string()),
            ))
        }
        Err(error) => Err(error),
    }
}
fn unsupported_method(error: &anyhow::Error) -> bool {
    let error = error.to_string().to_ascii_lowercase();
    error.contains("-32601")
        || error.contains("unknown method")
        || error.contains("method not found")
        || error.contains("unsupported-method")
}
pub(super) async fn send(state: &SharedState, scope: &Scope, input: &TurnRequest) -> Result<Value> {
    let target = target(state, &scope.client).await?;
    state
        .remote_sessions
        .watch(scope, Some(target.clone()))
        .await;
    ensure!(
        state
            .runtime
            .lock()
            .await
            .current_turn_id(&scope.session)
            .is_none(),
        "此会话正在通过聊天接入执行，请先完成当前任务"
    );
    let thread = read_thread(state, &target, &scope.session).await?;
    ensure!(
        thread_status(&thread) != "running",
        "此会话正在目标客户端执行，请先完成或停止"
    );
    if let Some(model) = input.model.as_deref().filter(|m| !m.is_empty()) {
        let available = models(state, &target).await?;
        let selected = available
            .iter()
            .find(|m| m["id"] == model)
            .context("所选模型已不在 Codex 模型目录中")?;
        if let Some(effort) = input.effort.as_deref() {
            ensure!(
                selected["efforts"]
                    .as_array()
                    .is_some_and(|e| e.iter().any(|v| v == effort)),
                "该模型不支持所选思考强度"
            );
        }
    }
    request(
        state,
        &target,
        "thread/resume",
        json!({"threadId":scope.session,"excludeTurns":true}),
    )
    .await?;
    let attachments = files::selected(state, scope, &input.attachment_ids).await?;
    let mut items = Vec::new();
    if !input.text.trim().is_empty() {
        items.push(json!({"type":"text","text":input.text,"text_elements":[]}));
    }
    for file in &attachments {
        if file.public.mime_type.starts_with("image/") && file.public.mime_type != "image/svg+xml" {
            items.push(json!({"type":"localImage","path":file.path}));
        } else {
            items.push(json!({"type":"text","text":format!("File: {}",file.path.display()),"text_elements":[]}));
        }
    }
    let mut params = json!({"threadId":scope.session,"input":items});
    if let Some(model) = input.model.as_deref().filter(|m| !m.is_empty()) {
        params["model"] = model.into();
    }
    if let Some(effort) = input.effort.as_deref().filter(|e| !e.is_empty()) {
        params["effort"] = effort.into();
    }
    let pending_files = files::mark_sent(state, scope, &input.attachment_ids).await?;
    let response = match request(state, &target, "turn/start", params).await {
        Ok(response) => response,
        Err(error) => {
            let error = super::unconfirmed(error);
            if error.downcast_ref::<super::UnconfirmedWrite>().is_none() {
                files::restore_pending(state, scope, &pending_files)
                    .await
                    .context("请求被拒绝，附件状态未能恢复")?;
            }
            return Err(error);
        }
    };
    let turn = response
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .context("Codex 未返回执行标识，请刷新核对结果，不会自动重发")
        .map_err(super::unconfirmed)?;
    let reply = json!({"turnId":turn,"status":"running"});
    state
        .remote_sessions
        .publish(scope, "turn.updated", None, reply.clone())
        .await;
    Ok(reply)
}
pub(super) async fn stop(
    state: &SharedState,
    scope: &Scope,
    requested: Option<&str>,
) -> Result<Value> {
    let target = target(state, &scope.client).await?;
    let (turns, _) = turn_page(
        state,
        &target,
        &scope.session,
        &PageQuery {
            cursor: None,
            limit: Some(1),
        },
    )
    .await?;
    let native_turn = turns
        .iter()
        .find(|t| {
            matches!(
                t["status"].as_str(),
                Some("inProgress" | "in_progress" | "running")
            )
        })
        .and_then(|t| t["id"].as_str());
    let turn = native_turn.context("当前会话没有可停止的运行任务")?;
    ensure!(
        requested.is_none_or(|id| id == turn),
        "运行任务已变化，请刷新后再停止"
    );
    request(
        state,
        &target,
        "turn/interrupt",
        json!({"threadId":scope.session,"turnId":turn}),
    )
    .await?;
    for _ in 0..20 {
        let (turns, _) = turn_page(
            state,
            &target,
            &scope.session,
            &PageQuery {
                cursor: None,
                limit: Some(1),
            },
        )
        .await?;
        let status = turns
            .iter()
            .find(|t| t["id"] == turn)
            .and_then(|t| t["status"].as_str())
            .unwrap_or("unknown");
        if !matches!(status, "inProgress" | "in_progress" | "running" | "unknown") {
            let status = normalize_status(status);
            state
                .remote_sessions
                .publish(
                    scope,
                    "turn.updated",
                    None,
                    json!({"status":status,"turnId":turn}),
                )
                .await;
            return Ok(json!({"status":status}));
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    state
        .remote_sessions
        .publish(
            scope,
            "turn.updated",
            None,
            json!({"status":"stopping","turnId":turn}),
        )
        .await;
    Ok(json!({"status":"stopping"}))
}
fn thread_status(thread: &Value) -> &str {
    if thread["turns"].as_array().into_iter().flatten().any(|t| {
        matches!(
            t["status"].as_str(),
            Some("inProgress" | "in_progress" | "running")
        )
    }) {
        return "running";
    }
    match thread["status"]["type"]
        .as_str()
        .or_else(|| thread["status"].as_str())
    {
        Some("active" | "running") => "running",
        _ => "idle",
    }
}
pub(super) fn normalize_status(status: &str) -> &str {
    match status {
        "inProgress" | "in_progress" => "running",
        "waiting_confirmation" => "waiting_approval",
        "interrupted" => "stopped",
        other => other,
    }
}

async fn normalize_item(state: &SharedState, scope: &Scope, item: &Value) -> Result<Value> {
    let id = item["id"].as_str().unwrap_or("native-item");
    let native = item["type"].as_str().unwrap_or("unknown");
    let (role, kind) = match native {
        "userMessage" => ("user", "message"),
        "agentMessage" => ("assistant", "message"),
        "reasoning" => ("assistant", "reasoning"),
        "fileChange" => ("tool", "fileChange"),
        _ => ("tool", "tool"),
    };
    let text = match native {
        "userMessage" => item["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        "agentMessage" => item["text"].as_str().unwrap_or_default().into(),
        "reasoning" => item["summary"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["text"].as_str().or_else(|| p.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        "commandExecution" => item["aggregatedOutput"].as_str().unwrap_or_default().into(),
        "mcpToolCall" => item["result"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                item["result"]
                    .as_object()
                    .map(|_| item["result"].to_string())
                    .unwrap_or_default()
            }),
        _ => item["text"]
            .as_str()
            .or_else(|| item["output"].as_str())
            .unwrap_or_default()
            .into(),
    };
    let mut attached = Vec::new();
    for part in item["content"].as_array().into_iter().flatten() {
        if part["type"] == "localImage"
            && let Some(path) = part["path"].as_str()
        {
            if let Some(file) =
                files::associate(state, scope, Path::new(path), "attachment").await?
            {
                attached.push(file);
            }
        }
    }
    for change in item["changes"].as_array().into_iter().flatten() {
        if let Some(path) = change["path"].as_str() {
            if let Some(file) = files::associate(state, scope, Path::new(path), "artifact").await? {
                attached.push(file);
            }
        }
    }
    Ok(
        json!({"id":id,"role":role,"type":kind,"text":text,"status":item["status"],"files":attached,
        "data":{"nativeType":native,"command":item["command"],"exitCode":item["exitCode"],"tool":item["tool"],
            "changes":item["changes"],"server":item["server"]}}),
    )
}

pub(crate) async fn observe(
    state: &SharedState,
    epoch: u64,
    client_id: &str,
    stream_id: &str,
    message: &Value,
) {
    let Some(method) = message["method"].as_str() else {
        return;
    };
    let params = &message["params"];
    if method == "serverRequest/resolved"
        && let Some(id) = params.get("requestId")
    {
        state
            .remote_sessions
            .resolve_native(epoch, client_id, stream_id, id)
            .await;
    }
    let Some(session) = params["threadId"]
        .as_str()
        .or_else(|| params["thread_id"].as_str())
        .or_else(|| params.pointer("/thread/id").and_then(Value::as_str))
    else {
        return;
    };
    if method == "turn/completed" {
        state
            .remote_sessions
            .clear_native(epoch, client_id, stream_id, session)
            .await;
    }
    if let Some(request_id) = message.get("id")
        && let Some(mut public) = public_approval(method, params)
    {
        if let Some(target) = targets(state)
            .await
            .into_iter()
            .find(|t| t.epoch == epoch && t.client_id == client_id && t.stream_id == stream_id)
        {
            let scope = Scope::new(&target.id, session);
            let id = format!("codex-{epoch}-{}", request_id.to_string().trim_matches('"'));
            public["id"] = id.clone().into();
            state
                .remote_sessions
                .remember_native(
                    scope,
                    id,
                    super::service::PendingApproval {
                        public,
                        source: ApprovalSource::Codex {
                            target,
                            request_id: request_id.clone(),
                            method: method.into(),
                            params: params.clone(),
                        },
                        claimed: false,
                    },
                )
                .await;
        }
    }
    let scopes = {
        let feeds = state.remote_sessions.feeds.lock().await;
        feeds
            .iter()
            .filter_map(|(scope, feed)| {
                let target = feed.source.as_ref()?;
                (scope.session == session
                    && target.epoch == epoch
                    && target.client_id == client_id
                    && target.stream_id == stream_id)
                    .then(|| (scope.clone(), target.clone()))
            })
            .collect::<Vec<_>>()
    };
    for (scope, target) in scopes {
        if let Some(request_id) = message.get("id") {
            let public = public_approval(method, params);
            if let Some(mut public) = public {
                let id = format!("codex-{epoch}-{}", request_id.to_string().trim_matches('"'));
                public["id"] = id.clone().into();
                state
                    .remote_sessions
                    .add_approval(
                        &scope,
                        id,
                        public,
                        ApprovalSource::Codex {
                            target,
                            request_id: request_id.clone(),
                            method: method.into(),
                            params: params.clone(),
                        },
                    )
                    .await;
            }
            continue;
        }
        match method {
            "item/agentMessage/delta"
            | "item/commandExecution/outputDelta"
            | "item/reasoning/summaryTextDelta" => {
                let item = params["itemId"].as_str().map(str::to_owned);
                state.remote_sessions.publish(&scope,"message.delta",item,json!({"text":params["delta"],
                    "role":if method.contains("commandExecution") {"tool"} else {"assistant"},
                    "type":if method.contains("reasoning") {"reasoning"} else if method.contains("commandExecution") {"tool"} else {"message"}})).await;
            }
            "item/started" | "item/completed" => {
                if let Ok(item) = normalize_item(state, &scope, &params["item"]).await {
                    state
                        .remote_sessions
                        .publish(
                            &scope,
                            "item.updated",
                            item["id"].as_str().map(str::to_owned),
                            json!({"item":item,"completed":method == "item/completed"}),
                        )
                        .await;
                }
            }
            "turn/started" | "turn/completed" => {
                let status = if method == "turn/started" {
                    "running"
                } else {
                    normalize_status(params["turn"]["status"].as_str().unwrap_or("completed"))
                };
                state
                    .remote_sessions
                    .publish(
                        &scope,
                        "turn.updated",
                        None,
                        json!({"status":status,"turnId":params["turn"]["id"]}),
                    )
                    .await;
                if let Some(error) = params["turn"]["error"]["message"].as_str() {
                    state
                        .remote_sessions
                        .publish(
                            &scope,
                            "error",
                            None,
                            json!({"code":"native_turn_failed","message":error}),
                        )
                        .await;
                }
            }
            "serverRequest/resolved" => {
                if let Some(id) = params.get("requestId") {
                    let id = format!("codex-{epoch}-{}", id.to_string().trim_matches('"'));
                    state.remote_sessions.resolve_approval(&scope, &id).await;
                }
            }
            "error" => {
                if let Some(error) = params["error"]["message"]
                    .as_str()
                    .or_else(|| params["message"].as_str())
                {
                    state
                        .remote_sessions
                        .publish(
                            &scope,
                            "error",
                            None,
                            json!({"code":"native_error","message":error}),
                        )
                        .await;
                }
            }
            _ => {}
        }
    }
}
fn notification(method: &str, params: &Value) -> crate::codex::CodexNotification {
    crate::codex::CodexNotification {
        method: method.into(),
        params: Some(params.clone()),
        request_id: None,
        remote_client_key: None,
        remote_client_id: None,
        remote_stream_id: None,
    }
}
fn public_approval(method: &str, params: &Value) -> Option<Value> {
    if method == "item/tool/requestUserInput" {
        let questions = params["questions"].as_array()?.iter().map(|q|json!({"id":q["id"],"prompt":q["question"],"required":true,
            "options":q["options"].as_array().map(|o|o.iter().map(|o|json!({"value":o["label"],"label":o["label"]})).collect::<Vec<_>>())})).collect::<Vec<_>>();
        return Some(json!({"type":"question","title":"需要你补充","questions":questions}));
    }
    let view = crate::codex::approval_request_view(&notification(method, params))?;
    Some(
        json!({"type":"approval","title":"需要确认","message":view.summary,
        "choices":view.decisions.iter().enumerate().map(|(i,d)|json!({"value":format!("option:{i}"),"label":d.label})).collect::<Vec<_>>()}),
    )
}
pub(super) async fn approve(
    state: &SharedState,
    scope: &Scope,
    id: &str,
    decision: &ApprovalDecision,
    target: &CodexTarget,
    request_id: &Value,
    method: &str,
    params: &Value,
) -> Result<()> {
    let current = self::target(state, &scope.client).await?;
    ensure!(
        current == *target,
        "原确认连接已变化，请刷新会话等待新的确认"
    );
    let response = if method == "item/tool/requestUserInput" {
        let mut answers = serde_json::Map::new();
        for question in params["questions"].as_array().context("追问格式不兼容")? {
            let key = question["id"].as_str().context("追问缺少标识")?;
            let value = &decision.answers[key];
            let entries = if let Some(text) = value.as_str() {
                vec![text.to_owned()]
            } else {
                value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            };
            ensure!(
                decision.decision != "accept" || !entries.is_empty(),
                "请填写或选择所有必填问题"
            );
            answers.insert(key.into(), json!({"answers":entries}));
        }
        json!({"answers":answers})
    } else {
        let view = crate::codex::approval_request_view(&notification(method, params))
            .context("此原生确认类型已不再可用")?;
        let selected = if let Some(index) = decision.decision.strip_prefix("option:") {
            view.decisions
                .get(index.parse::<usize>().context("确认选项无效")?)
                .context("确认选项已失效")?
        } else {
            let accept = decision.decision == "accept";
            ensure!(
                accept || matches!(decision.decision.as_str(), "decline" | "cancel"),
                "确认选项无效"
            );
            if accept {
                view.decisions.first()
            } else {
                view.decisions.iter().rev().find(|d| {
                    d.decision
                        .as_str()
                        .is_some_and(|v| matches!(v, "decline" | "denied" | "cancel"))
                        || d.decision
                            .pointer("/__codexRemotePermissionsResponse/permissions")
                            .and_then(Value::as_object)
                            .is_some_and(|o| o.is_empty())
                        || d.decision
                            .pointer("/__codexRemoteMcpElicitationResponse/action")
                            .and_then(Value::as_str)
                            == Some("decline")
                })
            }
            .context("没有匹配的原生确认选项")?
        };
        crate::codex::approval_response(selected.decision.clone())
    };
    remote_control_backend::send_response_for_stream(
        state,
        target.epoch,
        &target.client_id,
        &target.stream_id,
        request_id.clone(),
        response,
    )
    .await
    .map_err(super::unconfirmed)?;
    state.remote_sessions.resolve_approval(scope, id).await;
    state
        .remote_sessions
        .publish(scope, "turn.updated", None, json!({"status":"running"}))
        .await;
    Ok(())
}
