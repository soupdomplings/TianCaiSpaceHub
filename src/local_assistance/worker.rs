use super::*;
use anyhow::bail;
use std::{path::Path, process::Stdio};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

pub(super) fn run_dir(path: &Path, id: &str) -> Result<PathBuf> {
    ensure!(
        uuid::Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id),
        "协助记录标识无效"
    );
    let runs = settings::root(path).join("runs");
    let dir = runs.join(id);
    crate::desktop::transaction::reject_links(&dir)?;
    ensure!(dir.parent() == Some(runs.as_path()), "协助清理目录超出范围");
    Ok(dir)
}
fn write_owner(dir: &Path, id: &str, pid: Option<u32>) -> Result<()> {
    crate::desktop::transaction::atomic_write(
        &dir.join("owner.json"),
        &serde_json::to_vec(&json!({"owner":"tiancai-local-assistance-v1","task":id,"pid":pid}))?,
    )
}
pub(super) fn remove_owned(path: &Path, id: &str) -> Result<()> {
    let dir = run_dir(path, id)?;
    if !dir.exists() {
        return Ok(());
    }
    let marker = crate::desktop::transaction::read(&dir.join("owner.json"))?
        .context("协助记录缺少归属信息，未删除")?;
    let marker: Value = serde_json::from_slice(&marker)?;
    ensure!(
        marker["owner"] == "tiancai-local-assistance-v1" && marker["task"] == id,
        "协助记录归属不符，未删除"
    );
    if let Some(pid) = marker["pid"].as_u64() {
        ensure!(
            !super::process::alive(pid.try_into()?)?,
            "协助后台仍在运行或进程身份待核对，记录未删除"
        );
    }
    let mut queue = vec![dir.clone()];
    let mut count = 0;
    while let Some(p) = queue.pop() {
        crate::desktop::transaction::reject_links(&p)?;
        count += 1;
        ensure!(count <= 20_000, "协助记录目录过大，未删除");
        if p.is_dir() {
            for e in std::fs::read_dir(&p)? {
                queue.push(e?.path());
            }
        }
    }
    // Only this feature's UUID directory, after process exit and full link checks.
    std::fs::remove_dir_all(&dir).context("协助记录暂时无法清理，请稍后重试")?;
    Ok(())
}
pub(super) fn configuration(
    model: &str,
    effort: Option<&str>,
    port: u16,
    id: &str,
    token: &str,
    cwd: &Path,
    full_access: bool,
    writable: bool,
) -> Result<String> {
    let mut value = toml::Table::new();
    value.insert("model".into(), model.into());
    if let Some(effort) = effort {
        value.insert("model_reasoning_effort".into(), effort.into());
    }
    value.insert("model_provider".into(), "tiancai_assistance".into());
    value.insert(
        "approval_policy".into(),
        approval_policy(full_access).into(),
    );
    value.insert(
        "sandbox_mode".into(),
        sandbox_mode(full_access, writable).into(),
    );
    value.insert("web_search".into(), "disabled".into());
    value.insert(
        "features".into(),
        toml::Value::try_from(
            json!({"multi_agent":false,"apps":false,"plugins":false,"remote_control":false}),
        )?,
    );
    value.insert(
        "windows".into(),
        toml::Value::try_from(json!({"sandbox":"unelevated"}))?,
    );
    value.insert(
        "history".into(),
        toml::Value::try_from(json!({"persistence":"none"}))?,
    );
    // Untrusted project config cannot install another MCP or change this provider.
    let mut projects = toml::Table::new();
    projects.insert(
        cwd.to_string_lossy().into_owned(),
        toml::Value::try_from(json!({"trust_level":"untrusted"}))?,
    );
    value.insert("projects".into(), projects.into());
    value.insert("model_providers".into(),toml::Value::try_from(json!({"tiancai_assistance":{
        "name":"TianCaiSpaceHub local assistance","wire_api":"responses","requires_openai_auth":false,
        "base_url":format!("http://127.0.0.1:{port}/api/local-assistance/model/{id}"),
        "experimental_bearer_token":token,"supports_websockets":false,
        "http_headers":{"x-openai-actor-authorization":"Bearer codexhub"}}}))?);
    Ok(toml::to_string(&value)?)
}
fn approval_policy(full_access: bool) -> &'static str {
    if full_access { "never" } else { "on-request" }
}
fn sandbox_mode(full_access: bool, writable: bool) -> &'static str {
    if !writable {
        "read-only"
    } else if full_access {
        "danger-full-access"
    } else {
        "workspace-write"
    }
}
async fn send<W: tokio::io::AsyncWrite + Unpin>(stdin: &mut W, value: Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(&value)?;
    ensure!(bytes.len() <= MAX_WIRE, "协助协议消息过长");
    bytes.push(b'\n');
    tokio::time::timeout(Duration::from_secs(5), stdin.write_all(&bytes))
        .await
        .context("协助后台发送超时")??;
    Ok(())
}
/// Partial bytes belong to the connection, not to a cancelled select! future.
pub(super) struct WireReader<R> {
    reader: R,
    bytes: Vec<u8>,
}
impl<R: AsyncBufRead + Unpin> WireReader<R> {
    pub(super) fn new(reader: R) -> Self {
        Self {
            reader,
            bytes: Vec::new(),
        }
    }
    pub(super) async fn next(&mut self) -> Result<Option<Value>> {
        loop {
            let data = self.reader.fill_buf().await?;
            if data.is_empty() {
                ensure!(self.bytes.is_empty(), "协助后台消息不完整");
                return Ok(None);
            }
            let len = data
                .iter()
                .position(|b| *b == b'\n')
                .map_or(data.len(), |n| n + 1);
            ensure!(self.bytes.len() + len <= MAX_WIRE, "协助后台消息超过限制");
            self.bytes.extend_from_slice(&data[..len]);
            self.reader.consume(len);
            if self.bytes.last() == Some(&b'\n') {
                let value = serde_json::from_slice(&self.bytes).context("协助后台协议格式不兼容");
                self.bytes.clear();
                return value.map(Some);
            }
        }
    }
}
async fn request<W: tokio::io::AsyncWrite + Unpin, R: AsyncBufRead + Unpin>(
    state: &SharedState,
    task: &str,
    stdin: &mut W,
    reader: &mut WireReader<R>,
    id: u64,
    method: &str,
    params: Value,
    early: &mut Vec<Value>,
) -> Result<Value> {
    ensure_live(state, task).await?;
    send(stdin, json!({"id":id,"method":method,"params":params})).await?;
    tokio::time::timeout(Duration::from_secs(25),async {
        let mut tick=tokio::time::interval(Duration::from_millis(200));
        loop {
            let v=tokio::select!{v=reader.next()=>v?,_=tick.tick()=>{ensure_live(state,task).await?;continue;}}.context("协助后台在初始化时退出")?;
            if v["id"].as_u64()==Some(id) && v.get("method").is_none() {
                ensure!(v.get("error").is_none(),"协助后台不支持所需协议，请更新 Codex；未降低权限或更换执行端");
                ensure_live(state,task).await?;
                return Ok(v["result"].clone());
            }
            ensure!(early.len()<128,"协助初始化事件超过限制");early.push(v);
        }
    }).await.context("协助后台初始化超时")?
}
async fn update(state: &SharedState, id: &str, f: impl FnOnce(&mut Task)) {
    if let Some(task) = state.local_assistance.inner.lock().await.tasks.get_mut(id) {
        f(task);
    }
    state.local_assistance.changed.notify_waiters();
}
pub(super) async fn run(
    state: SharedState,
    id: String,
    parent: Parent,
    settings: settings::Settings,
    writable: bool,
    mut control: mpsc::Receiver<Control>,
) {
    let outcome = execute(&state, &id, &parent, &settings, writable, &mut control).await;
    if let Err(e) = outcome {
        update(&state, &id, |t| {
            t.pending.clear();
            if t.process_pid.is_none() {
                t.worker_stopped = true;
            }
            t.bump(
                if t.closing && t.worker_stopped {
                    Phase::Cancelled
                } else {
                    Phase::Failed
                },
                bounded(&e.to_string(), 4096),
            );
        })
        .await;
    }
    let (stopped, cleanup) = {
        let inner = state.local_assistance.inner.lock().await;
        let t = &inner.tasks[&id];
        (t.worker_stopped, t.delete_requested)
    };
    if stopped {
        let result = if cleanup {
            remove_owned(&state.config_path, &id)
        } else {
            // In-memory task feedback is enough. Do not persist a second copy
            // of protocol messages, prompts, approvals or model output.
            Ok(())
        };
        match result {
            Err(e) => {
                update(&state, &id, |t| {
                    t.detail = format!("{}；临时内容清理失败：{e}", t.detail)
                })
                .await
            }
            Ok(()) => {
                update(&state, &id, |t| {
                    t.api_token.clear();
                    if t.delete_requested {
                        t.erase();
                    }
                })
                .await
            }
        }
    }
}
async fn execute(
    state: &SharedState,
    id: &str,
    parent: &Parent,
    settings: &settings::Settings,
    writable: bool,
    control: &mut mpsc::Receiver<Control>,
) -> Result<()> {
    ensure_live(state, id).await?;
    let runtime = super::runtime::resolve_ready(&state.config_path)
        .context("协助尚未启动，请本人在设置中确认准备共享天才搭档 Server")?;
    let dir = run_dir(&state.config_path, id)?;
    std::fs::create_dir_all(&dir)?;
    crate::desktop::permissions::owner_only(&dir)?;
    crate::desktop::transaction::atomic_write(
        &dir.join("owner.json"),
        &serde_json::to_vec(&json!({"owner":"tiancai-local-assistance-v1","task":id}))?,
    )?;
    let home = dir.join("codex-home");
    std::fs::create_dir_all(&home)?;
    crate::desktop::permissions::owner_only(&home)?;
    let (model, token, goal, effort) = {
        let inner = state.local_assistance.inner.lock().await;
        let t = &inner.tasks[id];
        (
            t.model.clone(),
            t.api_token.clone(),
            t.goal.clone(),
            t.reasoning_effort.clone(),
        )
    };
    let port = state
        .config
        .lock()
        .await
        .local_listen_port()
        .context("Hub 本机服务端口无效")?;
    crate::desktop::transaction::atomic_write(
        &home.join("config.toml"),
        configuration(
            &model,
            effort.as_deref(),
            port,
            id,
            &token,
            &parent.cwd,
            settings.full_access,
            writable,
        )?
        .as_bytes(),
    )?;
    let binary = &runtime.program;
    ensure_live(state, id).await?;
    let mut command = Command::new(&binary);
    runtime.configure_command(&mut command)?;
    command
        .arg("app-server")
        .arg("--listen")
        .arg("stdio://")
        .current_dir(&home)
        .env("CODEX_HOME", &home)
        .env_remove("CODEX_API_BASE_URL")
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("CODEX_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("ALL_PROXY")
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("all_proxy")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    #[cfg(target_os = "macos")]
    command.process_group(0);
    ensure_live(state, id).await?;
    let mut child = command
        .spawn()
        .map_err(|error| anyhow::anyhow!(super::binary::spawn_error(&binary, &error)))?;
    let pid = child.id();
    update(state, id, |t| t.process_pid = pid).await;
    let tree = super::process::ProcessTree::attach(&child);
    // Record ownership before the first model turn; a crash leaves a cleanable,
    // process-checked directory rather than an anonymous Codex profile.
    let marker = write_owner(&dir, id, pid);
    let result = match (&tree, marker) {
        (Ok(_), Ok(())) => {
            conversation(
                state, id, parent, &model, &goal, writable, &mut child, control,
            )
            .await
        }
        (Err(e), _) => Err(anyhow::anyhow!(e.to_string())),
        (_, Err(e)) => Err(e),
    };
    // Stop before erasing files, including on failed handshakes and disconnects.
    let _ = child.start_kill();
    let reaped = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
    let tree_stopped = match tree {
        Ok(tree) => tree.stop_and_confirm().await.is_ok(),
        Err(_) => true,
    };
    let exited = matches!(reaped, Ok(Ok(_))) && tree_stopped;
    let marker = if exited {
        write_owner(&dir, id, None)
    } else {
        Ok(())
    };
    update(state, id, |t| {
        t.worker_stopped = exited;
        t.pending.clear();
        if exited {
            t.process_pid = None;
        } else {
            t.bump(
                Phase::Failed,
                "协助后台退出尚未确认，记录保留，请关闭天工后重试清理",
            );
        }
    })
    .await;
    marker?;
    if !exited {
        bail!("协助后台停止未确认，未清理记录");
    }
    result
}
async fn conversation(
    state: &SharedState,
    id: &str,
    parent: &Parent,
    model: &str,
    goal: &str,
    writable: bool,
    child: &mut Child,
    control: &mut mpsc::Receiver<Control>,
) -> Result<()> {
    let mut stdin = child.stdin.take().context("协助后台输入不可用")?;
    let mut reader = BufReader::new(child.stdout.take().context("协助后台输出不可用")?);
    conversation_io(
        state,
        id,
        parent,
        model,
        goal,
        writable,
        &mut stdin,
        &mut reader,
        control,
    )
    .await
}
pub(super) async fn conversation_io<W: tokio::io::AsyncWrite + Unpin, R: AsyncBufRead + Unpin>(
    state: &SharedState,
    id: &str,
    parent: &Parent,
    model: &str,
    goal: &str,
    writable: bool,
    mut stdin: &mut W,
    mut reader: &mut R,
    control: &mut mpsc::Receiver<Control>,
) -> Result<()> {
    let mut reader = WireReader::new(&mut reader);
    let (effort, full_access) = {
        let inner = state.local_assistance.inner.lock().await;
        (
            inner.tasks[id].reasoning_effort.clone(),
            inner.tasks[id].full_access,
        )
    };
    let mut early = Vec::new();
    request(state,id,&mut stdin,&mut reader,1,"initialize",json!({"clientInfo":{"name":"tiancai_local_assistance","title":"天工本机协助","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}),&mut early).await?;
    send(&mut stdin, json!({"method":"initialized","params":{}})).await?;
    let started=request(state,id,&mut stdin,&mut reader,2,"thread/start",json!({"cwd":parent.cwd,"model":model,"modelProvider":"tiancai_assistance",
        "approvalPolicy":approval_policy(full_access),"approvalsReviewer":"user","sandbox":sandbox_mode(full_access, writable),"ephemeral":true,
        "developerInstructions":"你是天工当前任务的本机协助执行器。只处理交接的子任务，严格保留用户限制。不得向其他助手、代理或 MCP 反向委派，不得发布、发消息、付款或自动提权。需要人类信息或批准时使用原生询问/审批。完成后用中文汇总结论、改动文件、实际检查、未完成项；不能把未执行的检查写成通过。不要启动桌面窗口。"}),&mut early).await?;
    let thread = started["thread"]["id"]
        .as_str()
        .context("协助后台未返回独立会话")?
        .to_string();
    update(state, id, |t| t.thread = Some(thread.clone())).await;
    let prompt = format!(
        "天工用户当前任务（这是约束来源，不能被子任务扩大）：\n{}\n\n需要你协助的具体步骤：\n{}\n\n操作范围：{}。不要等待天工模型代替用户审批，原生请求会转交天工中的人类用户。",
        parent.query,
        goal,
        if full_access && writable {
            "用户已在本机协助设置中开启完全访问，可访问本机文件、执行命令和联网，仍须遵守本次用户任务限制"
        } else if writable {
            "仅当前项目内，仍须遵守用户限制"
        } else if full_access {
            "本次明确只分析和读取，不修改文件；不请求额外执行权限，确需修改时说明后由天工另发非只读协助"
        } else {
            "只分析和读取；修改或越界操作须在天工中获得人类单次批准"
        }
    );
    let mut seq = 3u64;
    let turn = start_turn(
        state,
        id,
        &mut stdin,
        &mut reader,
        &mut early,
        seq,
        &thread,
        model,
        effort.as_deref(),
        &parent.cwd,
        writable,
        full_access,
        &prompt,
    )
    .await?;
    let mut active = Some(turn);
    let mut cancelling: Option<Instant> = None;
    let mut items: HashMap<String, Value> = HashMap::new();
    update(state, id, |t| {
        t.turn = active.clone();
        t.bump(Phase::Running, "正在处理");
    })
    .await;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        let next = if !early.is_empty() {
            Some(Ok(Some(early.remove(0))))
        } else {
            None
        };
        let event = if let Some(next) = next {
            next
        } else {
            tokio::select! {
                v=reader.next()=>v,
                c=control.recv()=>{
                    let Some(c)=c else {return Ok(());};
                    match c {
                        Control::Shutdown=>{update(state,id,|t|{t.delete_requested=false;t.detail="正在停止后台并清理临时内容".into();}).await;return Ok(());}
                        Control::Decision{nonce,response}=>{
                            let prompt={let mut inner=state.local_assistance.inner.lock().await;let task=inner.tasks.get_mut(id).unwrap();
                                if let Some(index)=task.pending.iter().position(|p|p.nonce==nonce) {Some(task.pending.remove(index))}else{None}};
                            if let Some(prompt)=prompt {
                                match protocol::decision(&prompt,&response) {
                                    Ok(response)=>{send(&mut stdin,json!({"id":prompt.request_id,"result":response})).await?;
                                        update(state,id,|t|{if t.pending.is_empty(){t.bump(Phase::Running,"已收到你的处理，继续协助");}}).await;}
                                    Err(e)=>{update(state,id,|t|{t.detail=e.to_string();t.pending.push(prompt);}).await;}
                                }
                            }
                        }
                        Control::Followup(text)=>{
                            let allowed={let inner=state.local_assistance.inner.lock().await;inner.tasks[id].phase==Phase::Starting && inner.tasks[id].turns<3};
                            if allowed && !text.trim().is_empty() && text.len()<=MAX_TEXT {
                                seq+=1;let t=start_turn(state,id,&mut stdin,&mut reader,&mut early,seq,&thread,model,effort.as_deref(),&parent.cwd,writable,full_access,&text).await?;
                                active=Some(t);items.clear();update(state,id,|task|{task.turns+=1;task.turn=active.clone();task.result.clear();task.bump(Phase::Running,"正在补充处理");}).await;
                            }
                        }
                        Control::Finish if active.is_none()=>{update(state,id,|t|t.detail="协助已结束".into()).await;return Ok(());}
                        Control::Delete if active.is_none()=>{update(state,id,|t|{t.delete_requested=true;t.detail="正在清理记录".into();}).await;return Ok(());}
                        Control::Cancel|Control::Delete|Control::Finish=>{
                            if let Some(turn)=&active {seq+=1;send(&mut stdin,json!({"id":seq,"method":"turn/interrupt","params":{"threadId":thread,"turnId":turn}})).await?;
                                cancelling=Some(Instant::now());update(state,id,|t|{t.pending.clear();t.bump(Phase::Cancelling,"正在停止协助");}).await;}
                            else {return Ok(());}
                        }
                    }
                    continue;
                }
                _=tick.tick()=>{
                    if cancelling.is_some_and(|at|at.elapsed()>Duration::from_secs(10)){update(state,id,|t|t.bump(Phase::Stopped,"协助后台已结束；未收到本轮中断确认，请核对已发生的修改")).await;return Ok(());}
                    let inner=state.local_assistance.inner.lock().await;let t=&inner.tasks[id];
                    if inner.cleaning {drop(inner);return Ok(());}
                    // Completed sessions stay available for follow-ups until
                    // explicit cleanup/finish/disconnect or the real Hub exit.
                    let expired=active.is_some() && t.updated.elapsed()>Duration::from_secs(1800);
                    let prompt_expired=t.pending.iter().any(|p|crate::types::now_ms().saturating_sub(p.at_ms)>600_000);
                    drop(inner);
                    if expired||prompt_expired {update(state,id,|t|{if !t.phase.terminal(){t.bump(Phase::Stopped,"协助等待超时，后台已停止");}}).await;return Ok(());}
                    if cancelling.is_none() && ensure_live(state,id).await.is_err(){
                        if let Some(turn)=&active{seq+=1;send(&mut stdin,json!({"id":seq,"method":"turn/interrupt","params":{"threadId":thread,"turnId":turn}})).await?;
                            cancelling=Some(Instant::now());update(state,id,|t|{t.pending.clear();t.bump(Phase::Cancelling,"天工任务已结束，正在停止协助");}).await;}
                        else{return Ok(());}
                    }
                    continue;
                }
            }
        };
        let v = event?.context("协助后台连接中断；不会自动重放任务")?;
        if let Some(outcome) = protocol::turn_outcome(&v, &thread, active.as_deref()) {
            active = None;
            update(state, id, |t| {
                t.pending.clear();
                let detail = if outcome == Phase::Failed {
                    v["params"]["turn"]["error"]["message"]
                        .as_str()
                        .map(|s| bounded(s, 4096))
                        .unwrap_or_else(|| "协助未完成，请查看返回结果".into())
                } else {
                    match outcome {
                        Phase::Completed => "协助处理完成",
                        Phase::Cancelled => "已停止协助",
                        _ => "协助已结束",
                    }
                    .into()
                };
                t.bump(outcome, detail);
            })
            .await;
            if cancelling.is_some() {
                return Ok(());
            }
            continue;
        }
        let p = &v["params"];
        let method = v["method"].as_str().unwrap_or("");
        if p.get("threadId")
            .and_then(Value::as_str)
            .is_some_and(|s| s != thread)
        {
            continue;
        }
        if v.get("id").is_some() && !method.is_empty() {
            ensure!(
                active.is_some()
                    && p["threadId"] == thread
                    && (p["turnId"].as_str() == active.as_deref()
                        || method == "mcpServer/elicitation/request" && p["turnId"].is_null()),
                "协助审批归属不匹配，后台已停止"
            );
            if !protocol::supported(method, p) {
                send(&mut stdin,json!({"id":v["id"],"error":{"code":-32601,"message":"此类交互尚不能在天工内安全处理，已拒绝；请调整子任务。"}})).await?;
                update(state, id, |t| {
                    t.progress("协助请求了暂不支持的交互，已拒绝执行")
                })
                .await;
                continue;
            }
            ensure!(
                serde_json::to_vec(p)?.len() <= MAX_TEXT,
                "协助询问内容超过显示范围，已停止后台任务"
            );
            ensure!(
                state.local_assistance.inner.lock().await.tasks[id]
                    .pending
                    .len()
                    < 8,
                "协助待确认请求过多，已停止后台任务"
            );
            let mut params = p.clone();
            if matches!(
                method,
                "item/commandExecution/requestApproval" | "item/fileChange/requestApproval"
            ) {
                if let Some(item) = p["itemId"].as_str().and_then(|id| items.get(id)) {
                    params["operationPreview"] = item.clone();
                }
            }
            ensure!(
                serde_json::to_vec(&params)?.len() <= MAX_TEXT,
                "协助操作预览过长，已停止；请缩小子任务范围"
            );
            let prompt = Prompt {
                nonce: settings::token(),
                request_id: v["id"].clone(),
                method: method.into(),
                params,
                at_ms: crate::types::now_ms(),
            };
            update(state, id, |t| {
                t.pending.push(prompt);
                t.bump(Phase::WaitingUser, "需要你在天工中处理");
            })
            .await;
            continue;
        }
        if method == "serverRequest/resolved" {
            update(state, id, |t| {
                t.pending
                    .retain(|p| p.request_id != v["params"]["requestId"]);
                if t.pending.is_empty() && t.phase == Phase::WaitingUser {
                    t.bump(Phase::Running, "继续处理");
                }
            })
            .await;
        }
        if p.get("turnId")
            .and_then(Value::as_str)
            .is_some_and(|t| Some(t) != active.as_deref())
        {
            continue;
        }
        if method == "item/completed" || method == "item/started" {
            if active.is_none()
                || p["threadId"] != thread
                || p["turnId"].as_str() != active.as_deref()
            {
                continue;
            }
            let item = &p["item"];
            let completed = method == "item/completed";
            if let Some(key) = item["id"].as_str() {
                if completed {
                    items.remove(key);
                } else if items.len() < 64
                    && serde_json::to_vec(item)?.len() <= MAX_TEXT
                    && matches!(
                        item["type"].as_str(),
                        Some("commandExecution" | "fileChange")
                    )
                {
                    items.insert(key.into(), item.clone());
                }
            }
            update(state,id,|t|match item["type"].as_str(){
                Some("agentMessage") if completed=>{if let Some(text)=item["text"].as_str(){if t.result.len()+text.len()+2<=MAX_TEXT {if !t.result.is_empty(){t.result.push_str("\n\n");}t.result.push_str(text);}t.progress(bounded(text,160));}},
                Some("commandExecution")=>{if let Some(cmd)=item["command"].as_str(){t.progress(if completed{"检查完成"}else{"正在执行检查"});
                    if completed && t.commands.len()<24 {t.commands.push(json!({"command":bounded(cmd,4000),"exitCode":item["exitCode"],"status":item["status"],"output":bounded(item["aggregatedOutput"].as_str().unwrap_or(""),4000)}));}}},
                Some("fileChange")=>{
                    if completed && item["status"]=="completed" {if let Some(changes)=item["changes"].as_array(){for c in changes{if let Some(path)=c["path"].as_str(){if !t.files.iter().any(|v|v==path) && t.files.len()<128{t.files.push(bounded(path,2048));}}}}}
                    t.progress(if !completed{"正在准备文件修改"}else if item["status"]=="completed"{"文件修改已完成"}else{"文件修改未执行成功，请核对返回结果"});},
                _=>{}
            }).await;
        }
        if method == "error" {
            update(state, id, |t| {
                t.progress(format!(
                    "协助反馈：{}",
                    bounded(
                        p["error"]["message"].as_str().unwrap_or("请求处理失败"),
                        1000
                    )
                ))
            })
            .await;
        }
    }
}
async fn start_turn<W: tokio::io::AsyncWrite + Unpin, R: AsyncBufRead + Unpin>(
    state: &SharedState,
    task: &str,
    stdin: &mut W,
    reader: &mut WireReader<R>,
    early: &mut Vec<Value>,
    id: u64,
    thread: &str,
    model: &str,
    effort: Option<&str>,
    cwd: &Path,
    writable: bool,
    full_access: bool,
    text: &str,
) -> Result<String> {
    let policy = if full_access && writable {
        json!({"type":"dangerFullAccess"})
    } else if writable {
        json!({"type":"workspaceWrite","writableRoots":[cwd],"networkAccess":false})
    } else {
        json!({"type":"readOnly"})
    };
    let mut params = json!({"threadId":thread,"model":model,"cwd":cwd,"approvalPolicy":approval_policy(full_access),"sandboxPolicy":policy,
        "input":[{"type":"text","text":text}]});
    if let Some(effort) = effort {
        params["effort"] = effort.into();
    }
    let v = request(state, task, stdin, reader, id, "turn/start", params, early).await?;
    v["turn"]["id"]
        .as_str()
        .map(str::to_string)
        .context("协助后台没有确认本轮任务")
}
fn bounded(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}
