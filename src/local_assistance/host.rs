//! In-memory TianGong renderer adapter. No installed files, UI automation or
//! conversation-history scraping. Native run submissions supply parent scope.
use super::*;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};

const RULES: &str = "【天才 Hub 本机协助规则】你仍负责用户原任务。当同一方法两次失败、现有工具缺少关键能力、或关键验证仍失败时，应主动调用本机协助 MCP 的 assist_start，不要要求用户点名 Codex 或手选 Skill。交接具体目标、必要错误证据、已尝试方法、用户禁止事项。使用系统提供的 context_ref，相同问题固定 problem_key；分析任务设 analysis_only=true，用户已授权修复才可设为false。启动后自动用 assist_wait 等待；starting、running、waiting_user 都继续等待同一任务，不因工具返回这些状态就结束主任务。waiting_user 表示天工窗口正在等待人类，禁止你代答或代批；用户处理后继续等待结果。协助工作期间不要同时修改相同项目文件；结果回到这里后继续原任务、复核实际验证与限制。确有新证据可用 assist_followup 追问同任务，最多两次。完成采纳后必须 assist_finish；用户停止时 assist_cancel，等待终态，不自动重放失败工作。不把子助手声称成功当作已验证。";
const RUN_FETCH_ADAPTER: &str = include_str!("run-fetch-adapter.js");
const RUN_FETCH_MARKER: &str = "TIANCAISPACEHUB_ASSISTANCE_RUN_FETCH_V1";
const RENDERER_WIRE_LIMIT: usize = 16 * 1024 * 1024;

pub(super) async fn monitor(state: SharedState, mut shutdown: watch::Receiver<bool>) {
    let mut disable_retry = None::<Instant>;
    loop {
        if *shutdown.borrow() {
            break;
        }
        let enabled = settings::read(&state.config_path)
            .map(|(s, _)| s.enabled)
            .unwrap_or(false);
        if enabled {
            let result = async {
                web::sync_catalogue(&state).await?;
                session(&state, &mut shutdown).await
            }
            .await;
            if let Err(error) = result {
                state.local_assistance.inner.lock().await.host_detail = error.to_string();
            }
        } else if disable_retry.is_none_or(|at| at.elapsed() > Duration::from_secs(15)) {
            disable_retry = Some(Instant::now());
            if let Err(error) = web::sync_disabled(&state).await {
                state.local_assistance.inner.lock().await.host_detail = error.to_string();
            }
        }
        tokio::select! {_=tokio::time::sleep(Duration::from_secs(3))=>{},_=shutdown.changed()=>{if *shutdown.borrow(){break;}}}
    }
    let host = state.local_assistance.inner.lock().await.host.clone();
    if let Some(host) = host {
        disconnect(&state, &host).await;
    }
}

// A healthy renderer connection can stay open for hours. Catalogue maintenance
// must therefore not depend on that connection ending or the user toggling it.
pub(super) async fn catalogue_monitor(state: SharedState, mut shutdown: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(15)) => {},
            _ = shutdown.changed() => { if *shutdown.borrow() { break; } }
        }
        if *shutdown.borrow() {
            break;
        }
        if settings::read(&state.config_path).is_ok_and(|(s, _)| s.enabled) {
            if let Err(error) = web::sync_catalogue(&state).await {
                state.local_assistance.inner.lock().await.host_detail =
                    format!("本机协助同步未完成：{error}");
            }
        }
    }
}

pub(super) fn rules() -> &'static str {
    RULES
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn send(socket: &mut Socket, id: u64, method: &str, params: Value) -> Result<()> {
    tokio::time::timeout(
        Duration::from_secs(5),
        socket.send(Message::Text(
            json!({"id":id,"method":method,"params":params}).to_string(),
        )),
    )
    .await
    .context("天工协助窗口发送超时")??;
    Ok(())
}
async fn call(socket: &mut Socket, id: u64, method: &str, params: Value) -> Result<Value> {
    send(socket, id, method, params).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        for _ in 0..128 {
            let message = socket.next().await.context("天工协助窗口已关闭")??;
            let Message::Text(text) = message else {
                continue;
            };
            let v: Value = serde_json::from_str(&text)?;
            if v["id"].as_u64() == Some(id) {
                ensure!(v.get("error").is_none(), "天工协助窗口协议不兼容");
                return Ok(v["result"].clone());
            }
        }
        anyhow::bail!("天工协助窗口响应过多")
    })
    .await
    .context("天工协助窗口响应超时")?
}

/// The verified page may already contain drafts or a running task. Update only
/// its existing runFetch body; never reload it or replace its frozen IPC bridge.
async fn native_transport(socket: &mut Socket, context: i64, page: &str) -> Result<()> {
    let probe = call(socket, 100, "Runtime.evaluate", json!({
        "contextId":context,"returnByValue":true,
        "expression":"({ipc:typeof window.electronAPI?.chatRunOpen==='function',ready:window.__tiancaiAssistanceTransportV1===true})"
    })).await?;
    ensure!(
        probe.get("exceptionDetails").is_none(),
        "无法核对天工原生任务通道"
    );
    if probe["result"]["value"]["ipc"] != true || probe["result"]["value"]["ready"] == true {
        return Ok(());
    }
    let outcome = async {
        let script = native_transport_script(socket, context, page).await?;
        let original = call(socket, 102, "Debugger.getScriptSource", json!({"scriptId":script})).await?;
        let source = original["scriptSource"].as_str().context("天工原生任务脚本暂不可读")?;
        ensure!(source.len() <= 8 * 1024 * 1024, "天工原生任务脚本超过适配范围");
        let adapted = call(socket, 103, "Runtime.evaluate", json!({
            "contextId":context,"returnByValue":true,"silent":true,
            "expression":format!("({RUN_FETCH_ADAPTER})({})",serde_json::to_string(source)?)
        })).await?;
        ensure!(adapted.get("exceptionDetails").is_none() && adapted["result"]["value"]["adapted"] == true,
            "当前天工原生任务结构尚未适配，本机协助未就绪；原任务和草稿已保留");
        if adapted["result"]["value"]["present"] != true {
            let changed = adapted["result"]["value"]["source"].as_str().context("天工任务适配结果缺失")?;
            let mut may_have_changed = true;
            let apply = async {
                let result = call(socket, 104, "Debugger.setScriptSource", json!({"scriptId":script,"scriptSource":changed})).await?;
                may_have_changed = result["status"] == "Ok";
                ensure!(result["status"] == "Ok", "天工任务通道暂时无法接入，请待原任务结束后重试；未重新加载页面");
                let readback = call(socket, 105, "Debugger.getScriptSource", json!({"scriptId":script})).await?;
                ensure!(readback["scriptSource"].as_str() == Some(changed), "天工任务通道改写尚未生效，本机协助未就绪");
                Ok::<_,anyhow::Error>(())
            }.await;
            if let Err(error) = apply {
                // A passive wrapper is also transparent without the assistant,
                // but restore the original body whenever LiveEdit permits it.
                if may_have_changed {
                    let restored = call(socket, 106, "Debugger.setScriptSource", json!({"scriptId":script,"scriptSource":source})).await;
                    ensure!(restored.is_ok_and(|value|value["status"]=="Ok"),
                        "天工本机协助未就绪，任务通道恢复未获确认；原任务未重发，可正常退出天工后重试");
                }
                return Err(error);
            }
        } else {
            ensure!(source.contains(RUN_FETCH_MARKER) || source.contains("async function __tchNativeRunFetch(path, options = {}) {"),
                "天工协助任务通道标记无法核对");
        }
        let ready = call(socket, 107, "Runtime.evaluate", json!({
            "contextId":context,"returnByValue":true,
            "expression":"window.__tiancaiAssistanceTransportV1=true"
        })).await?;
        ensure!(ready.get("exceptionDetails").is_none() && ready["result"]["value"]==true,
            "天工协助任务通道尚未就绪");
        Ok(())
    }.await;
    let _ = call(socket, 108, "Debugger.disable", json!({})).await;
    outcome
}

async fn native_transport_script(socket: &mut Socket, context: i64, page: &str) -> Result<String> {
    send(socket, 101, "Debugger.enable", json!({})).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut acknowledged = false;
        let mut script = None;
        loop {
            let message = socket.next().await.context("天工协助窗口已关闭")??;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(&text)?;
            if value["id"].as_u64() == Some(101) {
                ensure!(value.get("error").is_none(), "天工任务通道不支持内存接入");
                acknowledged = true;
            }
            if value["method"] == "Debugger.scriptParsed"
                && value["params"]["executionContextId"].as_i64() == Some(context)
                && value["params"]["url"]
                    .as_str()
                    .is_some_and(|url| native_main_script(page, url))
            {
                ensure!(script.is_none(), "天工任务主脚本不唯一，本机协助未接入");
                script = value["params"]["scriptId"].as_str().map(str::to_owned);
            }
            if acknowledged && let Some(script) = script.take() {
                return Ok(script);
            }
        }
    })
    .await
    .context("天工任务主脚本尚未就绪，本机协助未接入")?
}

fn native_main_script(page: &str, candidate: &str) -> bool {
    let (Ok(page), Ok(candidate)) = (url::Url::parse(page), url::Url::parse(candidate)) else {
        return false;
    };
    if page.scheme() != "file"
        || candidate.scheme() != "file"
        || candidate.query().is_some()
        || candidate.fragment().is_some()
    {
        return false;
    }
    let Some(name) = candidate
        .path_segments()
        .and_then(|segments| segments.last())
    else {
        return false;
    };
    if !name.starts_with("main-")
        || !name.ends_with(".js")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return false;
    }
    let (Ok(expected), Ok(actual)) = (page.join("assets/"), candidate.join(".")) else {
        return false;
    };
    if cfg!(windows) {
        expected.as_str().eq_ignore_ascii_case(actual.as_str())
    } else {
        expected == actual
    }
}

async fn session(state: &SharedState, shutdown: &mut watch::Receiver<bool>) -> Result<()> {
    let url = crate::gmclaw_runtime::assistance_socket().await?;
    let config = WebSocketConfig {
        max_message_size: Some(RENDERER_WIRE_LIMIT),
        max_frame_size: Some(RENDERER_WIRE_LIMIT),
        ..Default::default()
    };
    let (mut socket, _) = connect_async_with_config(url.as_str(), Some(config), false)
        .await
        .context("天工协助窗口无法连接")?;
    let host = settings::token();
    let binding = format!("__tch_assist_{}", uuid::Uuid::new_v4().simple());
    let frames = call(&mut socket, 1, "Page.getFrameTree", json!({})).await?;
    let frame = frames["frameTree"]["frame"]["id"]
        .as_str()
        .context("无法核对天工主页面")?;
    let context = default_context(&mut socket, frame).await?;
    let page = frames["frameTree"]["frame"]["url"]
        .as_str()
        .context("无法核对天工页面位置")?;
    native_transport(&mut socket, context, page).await?;
    call(
        &mut socket,
        3,
        "Runtime.addBinding",
        json!({"name":binding,"executionContextId":context}),
    )
    .await?;
    let expression = format!(
        "({})({})",
        include_str!("tiangong-host.js"),
        json!({"host":host,"binding":binding})
    );
    let installed=call(&mut socket,4,"Runtime.evaluate",json!({"contextId":context,"expression":expression,"returnByValue":true,"awaitPromise":true})).await?;
    ensure!(
        installed.get("exceptionDetails").is_none() && installed["result"]["value"] == "installed",
        "天工运行界面与协助适配不兼容，未接管任务或请求"
    );
    {
        let mut inner = state.local_assistance.inner.lock().await;
        inner.host = Some(host.clone());
        inner.host_detail = "天工已接入，等待新任务提供自动协助规则".into();
        inner.trigger_state = "waiting_task".into();
    }
    let result = session_loop(state, shutdown, &mut socket, &host, &binding, context).await;
    let _ = send(
        &mut socket,
        999_999,
        "Runtime.evaluate",
        json!({"contextId":context,"expression":"window.__tiancaiAssistance?.dispose()"}),
    )
    .await;
    disconnect(state, &host).await;
    result
}
async fn default_context(socket: &mut Socket, frame: &str) -> Result<i64> {
    send(socket, 2, "Runtime.enable", json!({})).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut context = None;
        let mut ack = false;
        for _ in 0..128 {
            let message = socket.next().await.context("天工协助窗口已关闭")??;
            let Message::Text(text) = message else {
                continue;
            };
            let v: Value = serde_json::from_str(&text)?;
            if v["id"] == 2 {
                ensure!(v.get("error").is_none(), "无法核对天工执行上下文");
                ack = true;
            }
            if v["method"] == "Runtime.executionContextCreated" {
                let c = &v["params"]["context"];
                if c["auxData"]["isDefault"] == true && c["auxData"]["frameId"] == frame {
                    context = c["id"].as_i64();
                }
            }
            if ack {
                if let Some(context) = context {
                    return Ok(context);
                }
            }
        }
        anyhow::bail!("未找到天工主页面的默认上下文")
    })
    .await
    .context("天工主页面上下文核对超时")?
}
pub(super) fn binding_event(v: &Value, binding: &str, context: i64) -> bool {
    v["method"] == "Runtime.bindingCalled"
        && v["params"]["name"] == binding
        && v["params"]["executionContextId"].as_i64() == Some(context)
}
pub(super) fn host_tasks(inner: &Inner, host: &str) -> Vec<Value> {
    let mut tasks: Vec<_> = inner
        .tasks
        .values()
        .filter(|t| t.host == host && t.phase != Phase::Cleared)
        .collect();
    tasks.sort_by_key(|t| (!busy(t), std::cmp::Reverse(t.updated)));
    tasks
        .into_iter()
        .take(8)
        .map(|t| {
            let mut v = t.view(true);
            v.as_object_mut().unwrap().remove("elapsedSeconds");
            v["result"] = t.result.chars().take(12000).collect::<String>().into();
            v["resultTruncated"] = json!(t.result.chars().count() > 12000);
            v["commands"] = json!(t.commands.iter().rev().take(6).cloned().collect::<Vec<_>>());
            v["files"] = json!(t.files.iter().take(32).collect::<Vec<_>>());
            v
        })
        .collect()
}
async fn session_loop(
    state: &SharedState,
    shutdown: &mut watch::Receiver<bool>,
    socket: &mut Socket,
    host: &str,
    binding: &str,
    context: i64,
) -> Result<()> {
    let mut seq = 10u64;
    let mut interval = tokio::time::interval(Duration::from_millis(700));
    let mut pending_update = None::<(u64, Instant)>;
    loop {
        tokio::select! {
            changed=shutdown.changed()=>{if changed.is_err()||*shutdown.borrow(){return Ok(());}}
            _=interval.tick()=>{
                let enabled=settings::read(&state.config_path).map(|(s,_)|s.enabled).unwrap_or(false);
                let inner=state.local_assistance.inner.lock().await;
                if !enabled || inner.host.as_deref()!=Some(host){return Ok(());}
                if let Some((_,sent))=pending_update{ensure!(sent.elapsed()<Duration::from_secs(6),"天工协助界面未响应，后台任务正在停止");continue;}
                let tasks=host_tasks(&inner,host);drop(inner);
                seq+=1;send(socket,seq,"Runtime.evaluate",json!({"contextId":context,"expression":format!("window.__tiancaiAssistance?.update({})",json!({"host":host,"tasks":tasks})),"returnByValue":true})).await?;pending_update=Some((seq,Instant::now()));
            }
            message=socket.next()=>{
                let message=message.context("天工协助窗口已关闭")??;
                let Message::Text(text)=message else{continue;};let v:Value=serde_json::from_str(&text)?;
                if pending_update.as_ref().is_some_and(|(id,_)|v["id"].as_u64()==Some(*id)){
                    ensure!(v.get("error").is_none() && v["result"].get("exceptionDetails").is_none(),"天工协助界面更新失败，后台任务正在停止");pending_update=None;continue;
                }
                if v["method"]=="Runtime.executionContextsCleared" {anyhow::bail!("天工页面已刷新，旧协助任务已停止；新任务将自动重新接入");}
                if v["method"]=="Runtime.executionContextDestroyed" && v["params"]["executionContextId"].as_i64()==Some(context){anyhow::bail!("天工主页面已关闭，协助正在停止");}
                if !binding_event(&v,binding,context) {continue;}
                let payload=v["params"]["payload"].as_str().context("天工协助事件无效")?;
                ensure!(payload.len()<=MAX_TEXT*2,"天工协助事件过长");let event:Value=serde_json::from_str(payload)?;
                if event["host"]!=host {continue;}
                let reply_id=event["id"].as_u64().unwrap_or(0);
                let response=async {
                    let catalog = if event["op"] == "register" {
                        Some(tokio::time::timeout(Duration::from_secs(3), catalogue::native(state, &event["parent"]))
                            .await.context("读取天工工具目录超时，原任务继续；请稍后新建任务重试协助")??)
                    } else { None };
                    let mut data=event_from_host(state,host,&event).await?;
                    if let Some(catalog)=catalog {
                        let context=data["contextRef"].as_str().context("协助任务上下文未生成")?;
                        data["systemPrompt"]=catalogue::instructions(context,&catalog).into();
                    }
                    Ok::<_,anyhow::Error>(data)
                }.await;
                if let Err(error)=&response {
                    let mut inner=state.local_assistance.inner.lock().await;
                    inner.host_detail=error.to_string();inner.trigger_state="error".into();
                }
                seq+=1;send(socket,seq,"Runtime.evaluate",json!({"contextId":context,"expression":format!("window.__tiancaiAssistance?.reply({})",match response{
                    Ok(data)=>json!({"host":host,"id":reply_id,"data":data}),Err(e)=>json!({"host":host,"id":reply_id,"error":e.to_string()})})})).await?;
            }
        }
    }
}
pub(super) async fn event_from_host(state: &SharedState, host: &str, e: &Value) -> Result<Value> {
    let mut inner = state.local_assistance.inner.lock().await;
    ensure!(inner.host.as_deref() == Some(host), "天工协助连接已更新");
    match e["op"].as_str() {
        Some("register") => {
            let active_contexts: std::collections::HashSet<_> = inner
                .tasks
                .values()
                .filter(|t| !t.worker_stopped)
                .map(|t| t.context.clone())
                .collect();
            inner.contexts.retain(|key, p| {
                p.at.elapsed() < Duration::from_secs(7200) || active_contexts.contains(key)
            });
            let known: std::collections::HashSet<_> = inner.contexts.keys().cloned().collect();
            inner
                .admitted
                .retain(|(context, _), _| known.contains(context));
            let p = &e["parent"];
            let session = p["session_id"].as_str().context("缺少天工会话")?;
            let request = p["request_id"].as_str().context("缺少天工请求身份")?;
            ensure!(valid_id(session) && valid_id(request), "天工任务标识无效");
            if let Some((key, _)) = inner
                .contexts
                .iter()
                .find(|(_, p)| p.host == host && p.session == session && p.request == request)
            {
                return Ok(json!({"contextRef":key}));
            }
            ensure!(
                inner.contexts.len() < 256,
                "天工任务上下文已满，请结束部分任务后重新接入"
            );
            let cwd = PathBuf::from(
                p["project"]["path"]
                    .as_str()
                    .context("此天工任务没有项目目录")?,
            );
            ensure!(
                cwd.is_absolute() && cwd.is_dir(),
                "请选择有效的天工项目目录再使用协助"
            );
            crate::desktop::transaction::reject_links(&cwd)?;
            let cwd = cwd.canonicalize()?;
            let query = p["query"].as_str().context("缺少天工用户任务")?;
            ensure!(
                !query.trim().is_empty() && query.len() <= MAX_TEXT,
                "本次任务过长，请缩小协助范围"
            );
            let context = settings::token();
            inner.contexts.insert(
                context.clone(),
                Parent {
                    host: host.into(),
                    session: session.into(),
                    request: request.into(),
                    run: None,
                    cwd,
                    query: query.into(),
                    writable: p["tool_execution_mode"] == "full_control",
                    at: Instant::now(),
                },
            );
            Ok(json!({"contextRef":context}))
        }
        Some("run") => {
            let reference = e["context"].as_str().unwrap_or("");
            let p = inner
                .contexts
                .get_mut(reference)
                .context("天工任务上下文已变化")?;
            ensure!(p.host == host, "天工上下文归属无效");
            let run = e["run"].as_str().context("缺少天工运行标识")?;
            ensure!(valid_id(run), "天工运行标识无效");
            p.run = Some(run.into());
            inner.host_detail = "最近任务已附带工具参数和自动协助规则，等待天工按需调用".into();
            inner.trigger_state = "rules_sent".into();
            Ok(json!({}))
        }
        Some("notice") => {
            inner.host_detail = match e["code"].as_str() {
                Some("excluded") => "本轮已限定其他 MCP，本机协助未加入；需要协助时在天工选择本机协助，或取消连接限定后新建任务",
                Some("unsupported") => "本轮任务格式与协助适配不兼容，未接入自动协助；原天工任务继续",
                Some("blocked") => "本轮未允许 mcp_call，自动协助未接入；原天工任务继续",
                Some("submit_failed") => "本轮天工任务提交未成功，协助上下文已撤销；未重复提交原任务",
                _ => "本轮协助规则未接入，请重新打开 Hub 本机协助设置查看原因后新建任务",
            }.into();
            inner.trigger_state = "error".into();
            Ok(json!({}))
        }
        Some("invalidate") | Some("abandon") => {
            let run = e["run"].as_str().unwrap_or("");
            let contexts: Vec<_> = inner
                .contexts
                .iter()
                .filter(|(_, p)| p.host == host && p.run.as_deref() == Some(run))
                .map(|(id, _)| id.clone())
                .collect();
            let contexts = if e["op"] == "abandon" {
                inner
                    .contexts
                    .iter()
                    .filter(|(id, p)| p.host == host && Some(id.as_str()) == e["context"].as_str())
                    .map(|(id, _)| id.clone())
                    .collect()
            } else {
                contexts
            };
            for id in &contexts {
                inner.contexts.remove(id);
            }
            for task in inner
                .tasks
                .values_mut()
                .filter(|t| contexts.contains(&t.context) && !t.worker_stopped)
            {
                task.closing = true;
                task.pending.clear();
                let _ = task.controller.try_send(Control::Cancel);
            }
            Ok(json!({}))
        }
        Some("decision") => {
            let id = e["task"].as_str().context("缺少协助任务")?;
            let nonce = e["nonce"].as_str().context("缺少本次询问身份")?;
            let task = inner.tasks.get(id).context("协助已结束")?;
            ensure!(
                task.host == host && !task.worker_stopped && live(&inner, task),
                "此协助不属于当前天工窗口或已停止"
            );
            let prompt = task
                .pending
                .iter()
                .find(|p| p.nonce == nonce)
                .context("此询问已处理或失效")?;
            ensure!(
                crate::types::now_ms().saturating_sub(prompt.at_ms) < 600_000,
                "此询问已过期，请停止协助后重新发起"
            );
            ensure!(
                !prompt
                    .params
                    .get("decisionSubmitted")
                    .is_some_and(|v| v == true),
                "此询问已提交，请等待协助继续"
            );
            protocol::decision(prompt, &e["response"])?;
            task.controller
                .try_send(Control::Decision {
                    nonce: nonce.into(),
                    response: e["response"].clone(),
                })
                .map_err(|_| anyhow::anyhow!("协助繁忙，请稍后重试"))?;
            // Reject duplicate clicks before the worker has consumed the first.
            inner
                .tasks
                .get_mut(id)
                .unwrap()
                .pending
                .iter_mut()
                .find(|p| p.nonce == nonce)
                .unwrap()
                .params["decisionSubmitted"] = true.into();
            Ok(json!({}))
        }
        Some("detail") => {
            let id = e["task"].as_str().context("缺少协助任务")?;
            let t = inner.tasks.get(id).context("协助已结束")?;
            ensure!(t.host == host, "此协助不属于当前天工窗口");
            let mut v = t.view(true);
            v.as_object_mut().unwrap().remove("pending");
            Ok(v)
        }
        Some("stop") | Some("delete") => {
            let id = e["task"].as_str().context("缺少协助任务")?;
            let task = inner.tasks.get(id).context("协助已结束")?;
            ensure!(task.host == host, "此协助不属于当前天工窗口");
            let context = task.context.clone();
            drop(inner);
            control(
                state,
                Some(&context),
                id,
                if e["op"] == "stop" {
                    Control::Cancel
                } else {
                    Control::Delete
                },
            )
            .await
        }
        _ => anyhow::bail!("未知天工协助事件"),
    }
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}

#[cfg(test)]
mod native_transport_tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::{WebSocketStream, accept_async, connect_async};

    type ServerSocket = WebSocketStream<tokio::net::TcpStream>;

    async fn request(socket: &mut ServerSocket, method: &str) -> Value {
        let Message::Text(raw) = socket.next().await.unwrap().unwrap() else {
            panic!("expected CDP request");
        };
        let value: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["method"], method);
        value
    }
    async fn reply(socket: &mut ServerSocket, request: &Value, result: Value) {
        socket
            .send(Message::Text(
                json!({"id":request["id"],"result":result}).to_string(),
            ))
            .await
            .unwrap();
    }
    async fn parsed(socket: &mut ServerSocket, context: i64, url: &str, script: &str) {
        socket
            .send(Message::Text(
                json!({"method":"Debugger.scriptParsed","params":{
                    "executionContextId":context,"url":url,"scriptId":script
                }})
                .to_string(),
            ))
            .await
            .unwrap();
    }

    #[test]
    fn renderer_transport_script_stays_in_verified_installed_assets() {
        let windows = "file:///D:/Program%20Files/Work/resources/app.asar/out/renderer/index.html";
        let mac =
            "file:///Applications/Work.app/Contents/Resources/app.asar/out/renderer/index.html";
        for page in [windows, mac] {
            let expected = url::Url::parse(page)
                .unwrap()
                .join("assets/main-fixture.js")
                .unwrap();
            assert!(native_main_script(page, expected.as_str()));
            for candidate in [
                expected.join("../main-fixture.js").unwrap().to_string(),
                expected.join("vendor-fixture.js").unwrap().to_string(),
                format!("{expected}?other=1"),
                format!("{expected}#other"),
                "https://example.invalid/assets/main-fixture.js".into(),
            ] {
                assert!(!native_main_script(page, &candidate));
            }
        }
    }

    #[tokio::test]
    async fn renderer_transport_script_ignores_other_contexts_and_unrelated_notification_volume() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let enabled = request(&mut socket, "Debugger.enable").await;
            for index in 0..256 {
                parsed(
                    &mut socket,
                    7,
                    "file:///fixture/assets/vendor-noise.js",
                    &index.to_string(),
                )
                .await;
            }
            parsed(
                &mut socket,
                9,
                "file:///fixture/assets/main-current.js",
                "other-context",
            )
            .await;
            parsed(
                &mut socket,
                7,
                "file:///other/assets/main-current.js",
                "other-install",
            )
            .await;
            parsed(
                &mut socket,
                7,
                "file:///fixture/assets/main-current.js",
                "correct-main",
            )
            .await;
            reply(&mut socket, &enabled, json!({})).await;
        });
        let (mut socket, _) = connect_async(format!("ws://{address}")).await.unwrap();
        assert_eq!(
            native_transport_script(&mut socket, 7, "file:///fixture/index.html")
                .await
                .unwrap(),
            "correct-main"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn refused_live_edit_never_sets_ready_or_replays_an_edit_and_disables_debugger() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let probe = request(&mut socket, "Runtime.evaluate").await;
            reply(
                &mut socket,
                &probe,
                json!({"result":{"value":{"ipc":true,"ready":false}}}),
            )
            .await;
            let enabled = request(&mut socket, "Debugger.enable").await;
            parsed(
                &mut socket,
                7,
                "file:///fixture/assets/main-current.js",
                "main",
            )
            .await;
            reply(&mut socket, &enabled, json!({})).await;
            let source = request(&mut socket, "Debugger.getScriptSource").await;
            reply(
                &mut socket,
                &source,
                json!({"scriptSource":"original-source"}),
            )
            .await;
            let adapted = request(&mut socket, "Runtime.evaluate").await;
            reply(
                &mut socket,
                &adapted,
                json!({"result":{"value":{"adapted":true,"source":"adapted-source"}}}),
            )
            .await;
            let edit = request(&mut socket, "Debugger.setScriptSource").await;
            reply(
                &mut socket,
                &edit,
                json!({"status":"BlockedByActiveGenerator"}),
            )
            .await;
            // No marker evaluation or redundant rollback after a refused edit.
            let disabled = request(&mut socket, "Debugger.disable").await;
            reply(&mut socket, &disabled, json!({})).await;
        });
        let (mut socket, _) = connect_async(format!("ws://{address}")).await.unwrap();
        assert!(
            native_transport(&mut socket, 7, "file:///fixture/index.html")
                .await
                .unwrap_err()
                .to_string()
                .contains("待原任务结束")
        );
        server.await.unwrap();
    }
}
