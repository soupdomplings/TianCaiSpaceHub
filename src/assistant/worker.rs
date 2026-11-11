use super::*;
use anyhow::bail;
use std::{collections::HashMap, path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

type Events = mpsc::Sender<(String, Result<Value>)>;
enum Pending {
    Initialize,
    Thread,
    Turn,
    Interrupt,
}
impl Pending {
    fn method(&self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::Thread => "thread/start",
            Self::Turn => "turn/start",
            Self::Interrupt => "turn/interrupt",
        }
    }
}
fn server_error_detail(stage: &str, error: &Value) -> String {
    let info = error.get("codexErrorInfo").or_else(|| {
        error
            .get("data")
            .and_then(|data| data.get("codexErrorInfo"))
    });
    let http_status = info.and_then(|info| {
        [
            "httpConnectionFailed",
            "responseStreamConnectionFailed",
            "responseStreamDisconnected",
        ]
        .iter()
        .find_map(|name| info[*name]["httpStatusCode"].as_u64())
    });
    let message = error["message"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let reason = if message.contains("readonly access is no longer supported") {
        "Server 不再接受旧的只读权限参数，请更新 Hub"
    } else if let Some(status) = http_status {
        match status {
            400 | 422 => "管理员模型服务拒绝请求参数",
            401 => "管理员模型凭据无效或已过期，请重新获取配置",
            403 => "管理员模型服务拒绝访问，请核对服务权限",
            404 => "管理员模型接口不存在，请核对服务地址和模型 ID",
            408 | 504 => "管理员模型服务响应超时",
            429 => "管理员模型服务达到调用限制，请稍后再试",
            500..=599 => "管理员模型服务暂时不可用",
            _ => "管理员模型服务请求失败",
        }
    } else {
        match info.and_then(Value::as_str) {
            Some("contextWindowExceeded") => "模型上下文已满，请清空对话后重试",
            Some("usageLimitExceeded") => "管理员模型服务达到使用限制",
            Some("unauthorized") => "模型服务鉴权失败，请重新获取管理员配置",
            Some("badRequest") => "模型服务拒绝请求参数",
            Some("sandboxError") => "Server 无法建立当前只读权限环境",
            _ if matches!(error["code"].as_i64(), Some(-32600 | -32602)) => {
                "Server 不接受当前请求参数，请更新 Hub 与共享运行包"
            }
            _ => "天才搭档 Server 执行失败，请重试",
        }
    };
    // Protocol messages may contain upstream bodies, URLs or credentials. Show
    // the structured failure and stage without copying that private text.
    let mut context = vec![stage.to_owned()];
    if let Some(code) = error["code"].as_i64() {
        context.push(format!("RPC {code}"));
    }
    if let Some(status) = http_status {
        context.push(format!("HTTP {status}"));
    }
    format!("{reason}（{}）", context.join("，"))
}
pub(super) struct Worker {
    pub epoch: String,
    child: Option<Child>,
    tree: Option<crate::local_assistance::process::ProcessTree>,
    input: Box<dyn AsyncWrite + Unpin + Send>,
    reader: Option<tokio::task::JoinHandle<()>>,
    tools: Vec<tokio::task::JoinHandle<()>>,
    directory: PathBuf,
    config: model::Configuration,
    squad: Arc<Squad>,
    account: ExpectedAccount,
    thread: Option<String>,
    active: Option<String>,
    stopping: Option<Instant>,
    turn_error: Option<String>,
    pending: HashMap<u64, (Pending, Instant)>,
    sequence: u64,
}
fn owner(dir: &Path, id: &str, pid: Option<u32>, program: Option<&Path>) -> Result<()> {
    let mut marker = json!({"owner":"tiancai-assistant-v1","id":id,"pid":pid,"hubPid":std::process::id(),"program":program});
    #[cfg(target_os = "macos")]
    if let Some(pid) = pid {
        if let Ok(identity) = process_identity(pid) {
            marker["processIdentity"] = identity.into();
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = &mut marker;
    crate::desktop::transaction::atomic_write(
        &dir.join("owner.json"),
        &serde_json::to_vec(&marker)?,
    )
}
#[cfg(target_os = "macos")]
fn process_identity(pid: u32) -> Result<String> {
    let output = std::process::Command::new("/bin/ps")
        .args([
            "-ww",
            "-p",
            &pid.to_string(),
            "-o",
            "lstart=",
            "-o",
            "command=",
        ])
        .output()?;
    ensure!(output.status.success(), "无法核对天才搭档后台启动身份");
    let text = String::from_utf8(output.stdout)?.trim().to_owned();
    ensure!(!text.is_empty(), "天才搭档后台身份为空");
    Ok(text)
}
#[cfg(target_os = "macos")]
async fn reclaim_orphan(config_path: &Path, id: &str) -> Result<()> {
    let dir = owned_dir(config_path, id)?;
    let marker: Value = serde_json::from_slice(&std::fs::read(dir.join("owner.json"))?)?;
    ensure!(
        marker["owner"] == "tiancai-assistant-v1" && marker["id"] == id,
        "临时目录归属不符"
    );
    let Some(pid) = marker["pid"].as_u64() else {
        return Ok(());
    };
    let pid: u32 = pid.try_into()?;
    if !crate::local_assistance::process::alive(pid)? {
        return Ok(());
    }
    let hub_pid: u32 = marker["hubPid"]
        .as_u64()
        .context("临时会话缺少 Hub 身份")?
        .try_into()?;
    ensure!(
        !crate::local_assistance::process::alive(hub_pid)?,
        "原 Hub 仍在运行，未回收临时会话"
    );
    let program = PathBuf::from(
        marker["program"]
            .as_str()
            .context("临时会话缺少运行端位置")?,
    );
    ensure!(
        program.is_absolute()
            && program.starts_with(crate::local_assistance::runtime::root(config_path)),
        "残留进程不属于共享运行端"
    );
    let expected = marker["processIdentity"]
        .as_str()
        .context("残留进程缺少启动身份，未自动结束")?;
    let current = process_identity(pid)?;
    ensure!(
        current == expected && current.contains(program.to_string_lossy().as_ref()),
        "残留进程启动身份不符，未自动结束"
    );
    let group: i32 = pid.try_into()?;
    ensure!(
        unsafe { libc::getpgid(group) } == group,
        "残留后台进程组不符"
    );
    ensure!(
        unsafe { libc::kill(-group, libc::SIGKILL) } == 0,
        "无法结束残留天才搭档进程组"
    );
    for _ in 0..20 {
        if unsafe { libc::kill(-group, 0) } < 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!("残留进程组退出尚未确认，临时会话保留")
}
fn owned_dir(config_path: &Path, id: &str) -> Result<PathBuf> {
    ensure!(
        uuid::Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id),
        "天才搭档临时会话标识无效"
    );
    let dir = settings::root(config_path).join("runs").join(id);
    crate::desktop::transaction::reject_links(&dir)?;
    Ok(dir)
}
fn remove_owned(config_path: &Path, id: &str) -> Result<()> {
    let dir = owned_dir(config_path, id)?;
    if !dir.exists() {
        return Ok(());
    }
    let marker: Value = serde_json::from_slice(&std::fs::read(dir.join("owner.json"))?)?;
    ensure!(
        marker["owner"] == "tiancai-assistant-v1" && marker["id"] == id,
        "临时会话归属不符，未清理"
    );
    if let Some(pid) = marker["pid"].as_u64() {
        ensure!(
            !crate::local_assistance::process::alive(pid.try_into()?)?,
            "天才搭档后台尚未退出，临时目录保留"
        );
    }
    // This UUID directory is generated by this feature and never a user workdir.
    std::fs::remove_dir_all(&dir).context("天才搭档临时目录暂无法清理，请稍后重试")
}
pub(super) async fn recover(config_path: &Path) -> Result<()> {
    let runs = settings::root(config_path).join("runs");
    crate::desktop::transaction::reject_links(&runs)?;
    if !runs.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(runs)? {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if !uuid::Uuid::parse_str(&id).is_ok_and(|v| v.to_string() == id) {
            continue;
        }
        #[cfg(target_os = "macos")]
        if let Err(error) = reclaim_orphan(config_path, &id).await {
            tracing::warn!("天才搭档残留后台未回收：{error}");
            continue;
        }
        if let Err(error) = remove_owned(config_path, &id) {
            tracing::warn!("天才搭档残留会话未清理：{error}");
        }
    }
    Ok(())
}
fn configuration(
    config: &model::Configuration,
    dir: &Path,
    port: u16,
    id: &str,
    token: &str,
) -> Result<String> {
    let model = &config.model;
    let mut value = crate::local_assistance::codex_configuration(
        &model.id,
        model.default_reasoning_effort.as_deref(),
        "tiancai_buddy",
        &dir.join("work"),
        "never",
        "read-only",
    )?;
    value.insert(
        "model_catalog_json".into(),
        dir.join("models.json")
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    value.insert(
        "model_context_window".into(),
        (model.context_window as i64).into(),
    );
    value.insert(
        "model_auto_compact_token_limit".into(),
        ((model.context_window as f64 * 0.8) as i64).into(),
    );
    value.insert("project_doc_max_bytes".into(), 0.into());
    value.insert("features".into(),toml::Value::try_from(json!({"shell_tool":false,"view_image":false,
        "multi_agent":false,"apps":false,"plugins":false,"remote_plugin":false,"remote_control":false,
        "shell_snapshot":false,"skill_mcp_dependency_install":false}))?);
    value.insert(
        "agents".into(),
        toml::Value::try_from(json!({"enabled":false}))?,
    );
    value.insert(
        "skills".into(),
        toml::Value::try_from(json!({"bundled":{"enabled":false},"include_instructions":false}))?,
    );
    value.insert(
        "memories".into(),
        toml::Value::try_from(json!({"generate_memories":false,"use_memories":false}))?,
    );
    value.insert(
        "shell_environment_policy".into(),
        toml::Value::try_from(json!({"inherit":"none"}))?,
    );
    value.insert(
        "model_providers".into(),
        toml::Value::try_from(json!({"tiancai_buddy":{
            "name":"天才搭档管理员模型","wire_api":"responses","requires_openai_auth":false,
            "base_url":format!("http://127.0.0.1:{port}/api/assistant/model/{id}"),
            "experimental_bearer_token":token,"supports_websockets":false,
            "request_max_retries":0,"stream_max_retries":0,
            "capabilities":{"remote_compaction":"unsupported","external_web_access":false}
        }}))?,
    );
    Ok(toml::to_string(&value)?)
}
impl Worker {
    pub(super) async fn start(
        state: &SharedState,
        ready: crate::local_assistance::runtime::ReadyRuntime,
        squad: Arc<Squad>,
        account: ExpectedAccount,
        config: model::Configuration,
        events: Events,
    ) -> Result<Self> {
        let epoch = uuid::Uuid::new_v4().to_string();
        let directory = owned_dir(&state.config_path, &epoch)?;
        std::fs::create_dir_all(directory.join("home"))?;
        std::fs::create_dir_all(directory.join("work"))?;
        owner(&directory, &epoch, None, None)?;
        let token = uuid::Uuid::new_v4().to_string();
        let port = state
            .config
            .lock()
            .await
            .local_listen_port()
            .context("Hub 本机服务端口无效")?;
        crate::desktop::transaction::atomic_write(
            &directory.join("home/config.toml"),
            configuration(&config, &directory, port, &epoch, &token)?.as_bytes(),
        )?;
        crate::desktop::transaction::atomic_write(
            &directory.join("models.json"),
            &serde_json::to_vec(&config.catalog()?)?,
        )?;
        let mut command = Command::new(&ready.program);
        command
            .arg("app-server")
            .current_dir(directory.join("work"))
            .env("CODEX_HOME", directory.join("home"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy().to_ascii_uppercase();
            if name.starts_with("OPENAI_")
                || name.starts_with("AZURE_OPENAI_")
                || name.starts_with("CODEX_")
                || matches!(
                    name.as_str(),
                    "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY" | "NO_PROXY"
                )
            {
                if name != "CODEX_HOME" {
                    command.env_remove(key);
                }
            }
        }
        ready.configure_command(&mut command)?;
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        #[cfg(target_os = "macos")]
        command.process_group(0);
        let mut child = command
            .spawn()
            .context("天才搭档 Server 无法启动，请重新准备共享运行包")?;
        let tree = crate::local_assistance::process::ProcessTree::attach(&child)?;
        owner(&directory, &epoch, child.id(), Some(&ready.program))?;
        let input = child.stdin.take().context("天才搭档后台输入不可用")?;
        let stdout = child.stdout.take().context("天才搭档后台输出不可用")?;
        let reader_epoch = epoch.clone();
        let reader = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut bytes = Vec::new();
            let result: Result<()> = async {
                loop {
                    let data = reader.fill_buf().await?;
                    if data.is_empty() {
                        bail!("天才搭档后台连接已关闭");
                    }
                    let count = data
                        .iter()
                        .position(|b| *b == b'\n')
                        .map_or(data.len(), |n| n + 1);
                    ensure!(
                        bytes.len() + count <= 8 * 1024 * 1024,
                        "天才搭档协议消息超过限制"
                    );
                    bytes.extend_from_slice(&data[..count]);
                    reader.consume(count);
                    if bytes.last() == Some(&b'\n') {
                        let message = serde_json::from_slice(&bytes)
                            .context("天才搭档 Server 协议不兼容，请更新共享运行包")?;
                        bytes.clear();
                        if events
                            .send((reader_epoch.clone(), Ok(message)))
                            .await
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                }
            }
            .await;
            if let Err(error) = result {
                let _ = events.send((reader_epoch, Err(error))).await;
            }
        });
        let bridge = proxy::Bridge::new(
            epoch.clone(),
            token,
            squad.clone(),
            account.clone(),
            config.model.id.clone(),
        );
        let mut worker = Self {
            epoch,
            child: Some(child),
            tree: Some(tree),
            input: Box::new(input),
            reader: Some(reader),
            tools: Vec::new(),
            directory,
            config,
            squad,
            account,
            thread: None,
            active: None,
            stopping: None,
            turn_error: None,
            pending: HashMap::new(),
            sequence: 0,
        };
        worker
            .request(
                "initialize",
                json!({"clientInfo":{"name":"tiancai_buddy","title":"天才搭档",
            "version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}),
                Pending::Initialize,
            )
            .await?;
        {
            let mut inner = state.assistant.inner.lock().await;
            inner.bridge = Some(bridge);
            inner.process_active = true;
        }
        Ok(worker)
    }
    async fn wire(&mut self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        tokio::time::timeout(Duration::from_secs(5), self.input.write_all(&bytes))
            .await
            .context("天才搭档后台发送超时")??;
        Ok(())
    }
    async fn request(&mut self, method: &str, params: Value, kind: Pending) -> Result<()> {
        self.sequence += 1;
        let id = self.sequence;
        self.wire(json!({"id":id,"method":method,"params":params}))
            .await?;
        self.pending.insert(id, (kind, Instant::now()));
        Ok(())
    }
    pub(super) fn ready(&self) -> bool {
        self.thread.is_some()
            && self.active.is_none()
            && !self
                .pending
                .values()
                .any(|(kind, _)| matches!(kind, Pending::Turn))
    }
    pub(super) fn has_active_turn(&self) -> bool {
        self.active.is_some()
            || self
                .pending
                .values()
                .any(|(kind, _)| matches!(kind, Pending::Turn))
    }
    pub(super) fn check_timeout(&self) -> Result<()> {
        ensure!(
            !self
                .pending
                .values()
                .any(|(_, at)| at.elapsed() > Duration::from_secs(25)),
            "天才搭档后台初始化或请求响应超时，请重试"
        );
        ensure!(
            !self
                .stopping
                .is_some_and(|at| at.elapsed() > Duration::from_secs(10)),
            "未收到停止确认，已关闭本次天才搭档后台"
        );
        Ok(())
    }
    pub(super) async fn turn(&mut self, text: &str) -> Result<()> {
        ensure!(self.ready(), "天才搭档会话正在准备");
        self.turn_error = None;
        let params = crate::remote_control_backend::turn_start_params(
            self.thread.as_deref().context("天才搭档会话尚未就绪")?,
            text,
            &[],
        );
        self.request("turn/start", params, Pending::Turn).await
    }
    pub(super) async fn interrupt(&mut self) -> Result<()> {
        for tool in self.tools.drain(..) {
            tool.abort();
        }
        if let Some(turn) = self.active.clone() {
            self.stopping = Some(Instant::now());
            self.request(
                "turn/interrupt",
                json!({"threadId":self.thread,"turnId":turn}),
                Pending::Interrupt,
            )
            .await?;
        } else if self.has_active_turn() {
            self.stopping = Some(Instant::now());
        }
        Ok(())
    }
    pub(super) async fn tool_result(
        &mut self,
        turn: &str,
        id: Value,
        result: Result<Value>,
    ) -> Result<()> {
        ensure!(
            self.squad.assistant_identity()?.as_ref() == Some(&self.account),
            "小分队账户已切换，工具结果已丢弃"
        );
        if self.active.as_deref() != Some(turn) || self.stopping.is_some() {
            return Ok(());
        }
        let (success, text) = match result {
            Ok(value) => (true, serde_json::to_string(&value)?),
            Err(error) => (false, error.to_string()),
        };
        self.wire(json!({"id":id,"result":{"success":success,"contentItems":[{"type":"inputText","text":text}]}})).await
    }
    pub(super) async fn event(&mut self, state: &SharedState, event: Result<Value>) -> Result<()> {
        let message = event?;
        ensure!(
            self.squad.assistant_identity()?.as_ref() == Some(&self.account),
            "小分队账户已切换，本次会话已停止"
        );
        let method = message["method"].as_str().unwrap_or("");
        let params = &message["params"];
        if !method.is_empty()
            && params["turnId"]
                .as_str()
                .is_some_and(|turn| Some(turn) != self.active.as_deref())
        {
            // A delayed delta from the stopped turn cannot appear in the next answer.
            if message.get("id").is_none() {
                return Ok(());
            }
        }
        if method.is_empty() {
            if let Some((kind, _)) = message["id"]
                .as_u64()
                .and_then(|id| self.pending.remove(&id))
            {
                if let Some(error) = message.get("error") {
                    bail!(server_error_detail(kind.method(), error));
                }
                match kind {
                    Pending::Initialize => {
                        self.wire(json!({"method":"initialized","params":{}}))
                            .await?;
                        let mut params = crate::remote_control_backend::ThreadStartOptions {
                            cwd: Some(self.directory.join("work").to_string_lossy().into_owned()),
                            model: Some(self.config.model.id.clone()),
                            model_provider: Some("tiancai_buddy".into()),
                            reasoning_effort: self.config.model.default_reasoning_effort.clone(),
                            approval_policy: Some("never".into()),
                            ..Default::default()
                        }
                        .to_params();
                        params["sandbox"] = "read-only".into();
                        params["ephemeral"] = true.into();
                        params["dynamicTools"] = tools::definitions().into();
                        params["developerInstructions"] = "你是天才搭档，帮助用户管理天才空间 Hub 的渠道、模型与客户端配置。只使用 Hub 动态工具；不得执行 shell、读取任意文件、联网、创建其他代理或通过原生工具改配置。先读取现状，仅修改用户要求的内容。删除必须等待 Hub 真人批量确认。密钥只用 Hub 凭据表单取得引用，禁止让用户在聊天中粘贴明文。修改客户端参数前先读取能力。不启动客户端、安装器或真实模型测试。用中文简洁说明实际结果；不把未做的检查说成通过。".into();
                        self.request("thread/start", params, Pending::Thread)
                            .await?;
                    }
                    Pending::Thread => {
                        self.thread = Some(
                            message["result"]["thread"]["id"]
                                .as_str()
                                .context("天才搭档 Server 未返回会话标识")?
                                .into(),
                        );
                        state.assistant.inner.lock().await.detail = "天才搭档已就绪".into();
                    }
                    Pending::Turn => {
                        self.active = Some(
                            message["result"]["turn"]["id"]
                                .as_str()
                                .context("天才搭档 Server 未返回本轮标识")?
                                .into(),
                        );
                        if self.stopping.is_some() {
                            self.interrupt().await?;
                        }
                    }
                    Pending::Interrupt => (),
                }
            }
            return Ok(());
        }
        if params["threadId"]
            .as_str()
            .is_some_and(|id| Some(id) != self.thread.as_deref())
        {
            return Ok(());
        }
        if let Some(id) = message.get("id") {
            if method != "item/tool/call"
                || self.stopping.is_some()
                || self.active.is_none()
                || params["threadId"].as_str() != self.thread.as_deref()
                || params["turnId"].as_str() != self.active.as_deref()
            {
                self.wire(json!({"id":id,"error":{"code":-32601,"message":"天才搭档只允许当前会话的 Hub 管理工具"}})).await?;
                return Ok(());
            }
            let name = params["tool"]
                .as_str()
                .context("Hub 工具缺少名称")?
                .to_owned();
            ensure!(
                tools::definitions().iter().any(|d| d["name"] == name),
                "天才搭档 Server 请求了未知工具"
            );
            let args = params["arguments"].clone();
            let args = if let Some(text) = args.as_str() {
                serde_json::from_str(text)?
            } else {
                args
            };
            let state = state.clone();
            let tx = state.assistant.tx.clone();
            let epoch = self.epoch.clone();
            let turn = self.active.clone().unwrap();
            let id = id.clone();
            self.tools.retain(|task| !task.is_finished());
            let account = self.account.clone();
            self.tools.push(tokio::spawn(async move {
                let result = TOOL_ACCOUNT
                    .scope((epoch.clone(), account), async {
                        ensure_current_account(&state).await?;
                        tools::call(&state, &name, args).await
                    })
                    .await;
                let _ = tx.send(Control::Tool {
                    epoch,
                    turn,
                    id,
                    result,
                });
            }));
            return Ok(());
        }
        match method {
            "turn/started" => {
                if let Some(id) = params["turn"]["id"].as_str() {
                    self.active = Some(id.into());
                }
            }
            "item/agentMessage/delta" => {
                if self.stopping.is_none() {
                    if let Some(delta) = params["delta"].as_str() {
                        let mut inner = state.assistant.inner.lock().await;
                        if let Some(message) = inner
                            .messages
                            .last_mut()
                            .filter(|m| m.role == "assistant" && m.status == "running")
                        {
                            if message.text.len() + delta.len() <= 2 * 1024 * 1024 {
                                message.text.push_str(delta);
                            }
                        }
                    }
                }
            }
            "error" => {
                let detail = server_error_detail("模型请求", &params["error"]);
                state.assistant.inner.lock().await.detail =
                    if params["willRetry"].as_bool() == Some(true) {
                        format!("{detail}；Server 正在继续处理")
                    } else {
                        detail.clone()
                    };
                self.turn_error = Some(detail);
            }
            "turn/completed" => {
                let turn = &params["turn"];
                if turn["id"].as_str() != self.active.as_deref() {
                    return Ok(());
                }
                let status = match turn["status"].as_str() {
                    Some("completed") => "completed",
                    Some("interrupted") => "stopped",
                    _ => "error",
                };
                let detail = if status == "completed" {
                    "回复完成".to_owned()
                } else if status == "stopped" {
                    "已停止".to_owned()
                } else if !turn["error"].is_null() {
                    server_error_detail("本轮执行", &turn["error"])
                } else {
                    self.turn_error
                        .take()
                        .unwrap_or_else(|| "本轮未完成，请重试".into())
                };
                self.active = None;
                self.stopping = None;
                self.turn_error = None;
                for tool in self.tools.drain(..) {
                    tool.abort();
                }
                finish(state, status, detail).await;
            }
            _ => (),
        }
        Ok(())
    }
    pub(super) async fn close(&mut self) -> Result<()> {
        for tool in self.tools.drain(..) {
            tool.abort();
        }
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
        if let Some(tree) = &self.tree {
            tree.stop_and_confirm().await?;
        }
        if let Some(child) = &mut self.child {
            tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .context("天才搭档后台退出超时")??;
        }
        owner(&self.directory, &self.epoch, None, None)?;
        let config_root = self
            .directory
            .parent()
            .and_then(Path::parent)
            .context("临时目录无效")?;
        // Resolve the exact already-validated feature directory, without scanning other caches.
        ensure!(
            self.directory.parent() == Some(config_root.join("runs").as_path()),
            "临时目录超出清理范围"
        );
        crate::desktop::transaction::reject_links(&self.directory)?;
        std::fs::remove_dir_all(&self.directory).context("天才搭档临时目录暂无法清理")?;
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
        for task in self.tools.drain(..) {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::DuplexStream;

    fn sample_config() -> model::Configuration {
        model::Configuration::parse(json!({"enabled":true,"name":"天才搭档",
            "model":{"id":"fixture-model","label":"合成模型","contextWindow":128000,
                "reasoningEfforts":["low","medium"],"defaultReasoningEffort":"medium","supportsReasoning":true},
            "api":{"baseUrl":"https://models.example/v1","apiKey":"fixture-admin-key"}})).unwrap()
    }
    async fn frame(reader: &mut BufReader<DuplexStream>) -> Value {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }
    #[test]
    fn isolated_configuration_uses_admin_context_and_disables_native_tools() {
        let config = sample_config();
        let dir = Path::new(if cfg!(windows) {
            "C:/fixture/assistant"
        } else {
            "/fixture/assistant"
        });
        let value: toml::Value = toml::from_str(
            &configuration(&config, dir, 8999, "fixture", "local-bridge-token").unwrap(),
        )
        .unwrap();
        assert_eq!(value["model_context_window"].as_integer(), Some(128000));
        assert_eq!(
            value["model_auto_compact_token_limit"].as_integer(),
            Some(102400)
        );
        assert_eq!(value["features"]["shell_tool"].as_bool(), Some(false));
        assert_eq!(value["features"]["view_image"].as_bool(), Some(false));
        assert_eq!(value["web_search"].as_str(), Some("disabled"));
        assert_eq!(value["approval_policy"].as_str(), Some("never"));
        assert_eq!(value["sandbox_mode"].as_str(), Some("read-only"));
        assert_eq!(value["history"]["persistence"].as_str(), Some("none"));
        assert_eq!(value["model_provider"].as_str(), Some("tiancai_buddy"));
        assert_eq!(value["model_providers"].as_table().unwrap().len(), 1);
        assert_eq!(value["model_reasoning_effort"].as_str(), Some("medium"));
        assert_eq!(
            value["model_providers"]["tiancai_buddy"]["capabilities"]["remote_compaction"].as_str(),
            Some("unsupported")
        );
        assert_eq!(
            config.catalog().unwrap()["models"][0]["shell_type"],
            "disabled"
        );
        assert_eq!(
            config.catalog().unwrap()["models"][0]["apply_patch_tool_type"],
            Value::Null
        );
        assert!(
            !toml::to_string(&value)
                .unwrap()
                .contains("synthetic-assistant-token")
        );
        assert!(
            !toml::to_string(&value)
                .unwrap()
                .contains("fixture-admin-key")
        );
    }
    #[test]
    fn server_failure_details_preserve_stage_and_codes_without_private_text() {
        let retired = server_error_detail(
            "turn/start",
            &json!({"code":-32600,
            "message":"invalid request:readOnly access is no longer supported use permissionProfile for restricted reads"}),
        );
        assert!(retired.contains("只读权限参数"));
        assert!(retired.contains("turn/start"));
        assert!(retired.contains("RPC -32600"));
        let failed = server_error_detail(
            "本轮执行",
            &json!({
            "message":"https://models.example/?token=fixture-private-token Authorization: fixture-private-key",
            "additionalDetails":"private response body",
            "codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":401}}}),
        );
        assert!(failed.contains("HTTP 401") && failed.contains("凭据无效"));
        for private in [
            "fixture-private-token",
            "fixture-private-key",
            "models.example",
            "private response body",
        ] {
            assert!(!failed.contains(private));
        }
        assert!(
            server_error_detail(
                "本轮执行",
                &json!({"codexErrorInfo":"contextWindowExceeded"})
            )
            .contains("上下文已满")
        );
    }
    #[tokio::test]
    async fn protocol_fixture_reuses_one_thread_and_dispatches_only_scoped_dynamic_tools() {
        let temp = tempfile::tempdir().unwrap();
        let config = crate::config::AppConfig {
            state_path: temp.path().join("state.json"),
            ..Default::default()
        };
        let state = crate::app_state::AppState::new(
            temp.path().join("config.toml"),
            config.clone(),
            None,
            None,
        );
        let squad = Arc::new(Squad::new(&state.config_path, &config).unwrap());
        let account = squad.assistant_test_identity("generation-one").unwrap();
        let epoch = uuid::Uuid::new_v4().to_string();
        state.assistant.inner.lock().await.bridge = Some(proxy::Bridge::new(
            epoch.clone(),
            "fixture-local-token".into(),
            squad.clone(),
            account.clone(),
            "fixture-model".into(),
        ));
        let (writer, reader) = tokio::io::duplex(128 * 1024);
        let mut reader = BufReader::new(reader);
        let mut worker = Worker {
            epoch,
            child: None,
            tree: None,
            input: Box::new(writer),
            reader: None,
            tools: Vec::new(),
            directory: temp.path().join("run"),
            config: sample_config(),
            squad: squad.clone(),
            account,
            thread: None,
            active: None,
            stopping: None,
            turn_error: None,
            pending: HashMap::new(),
            sequence: 0,
        };
        worker
            .request(
                "initialize",
                json!({"capabilities":{"experimentalApi":true}}),
                Pending::Initialize,
            )
            .await
            .unwrap();
        assert_eq!(frame(&mut reader).await["method"], "initialize");
        worker
            .event(&state, Ok(json!({"id":1,"result":{}})))
            .await
            .unwrap();
        assert_eq!(frame(&mut reader).await["method"], "initialized");
        let start = frame(&mut reader).await;
        assert_eq!(start["method"], "thread/start");
        assert_eq!(start["params"]["ephemeral"], true);
        assert_eq!(start["params"]["sandbox"], "read-only");
        assert_eq!(start["params"]["approvalPolicy"], "never");
        assert_eq!(start["params"]["modelProvider"], "tiancai_buddy");
        assert_eq!(start["params"]["model"], "fixture-model");
        assert_eq!(
            start["params"]["config"]["model_reasoning_effort"],
            "medium"
        );
        assert_eq!(
            start["params"]["dynamicTools"].as_array().unwrap().len(),
            tools::definitions().len()
        );
        worker
            .event(
                &state,
                Ok(json!({"id":2,"result":{"thread":{"id":"one-thread"}}})),
            )
            .await
            .unwrap();
        for turn in ["turn-one", "turn-two"] {
            worker.turn("请说明操作入口").await.unwrap();
            let request = frame(&mut reader).await;
            assert_eq!(request["method"], "turn/start");
            assert_eq!(request["params"]["threadId"], "one-thread");
            assert_eq!(
                request["params"],
                crate::remote_control_backend::turn_start_params(
                    "one-thread",
                    "请说明操作入口",
                    &[]
                )
            );
            for field in ["sandboxPolicy", "approvalPolicy", "model", "effort"] {
                assert!(request["params"].get(field).is_none());
            }
            worker
                .event(
                    &state,
                    Ok(json!({"id":request["id"],"result":{"turn":{"id":turn}}})),
                )
                .await
                .unwrap();
            if turn == "turn-one" {
                worker
                    .event(
                        &state,
                        Ok(json!({"id":"tool-1","method":"item/tool/call","params":{
                    "threadId":"one-thread","turnId":turn,"tool":"hub_help","arguments":{}}})),
                    )
                    .await
                    .unwrap();
                let mut control = state.assistant.rx.lock().await.take().unwrap();
                let result = tokio::time::timeout(Duration::from_secs(2), control.recv())
                    .await
                    .unwrap()
                    .unwrap();
                if let Control::Tool {
                    turn, id, result, ..
                } = result
                {
                    worker.tool_result(&turn, id, result).await.unwrap();
                } else {
                    panic!("expected tool response");
                }
                *state.assistant.rx.lock().await = Some(control);
                let result = frame(&mut reader).await;
                assert_eq!(result["id"], "tool-1");
                assert_eq!(result["result"]["success"], true);
                worker
                    .event(
                        &state,
                        Ok(
                            json!({"id":"native-1","method":"item/commandExecution/requestApproval",
                    "params":{"threadId":"one-thread","turnId":turn}}),
                        ),
                    )
                    .await
                    .unwrap();
                assert_eq!(frame(&mut reader).await["error"]["code"], -32601);
            } else {
                worker.interrupt().await.unwrap();
                let stop = frame(&mut reader).await;
                assert_eq!(stop["method"], "turn/interrupt");
                assert_eq!(stop["params"]["turnId"], turn);
            }
            worker
                .event(
                    &state,
                    Ok(
                        json!({"method":"turn/completed","params":{"threadId":"one-thread",
                "turn":{"id":turn,"status":if turn=="turn-one"{"completed"}else{"interrupted"}}}}),
                    ),
                )
                .await
                .unwrap();
            assert!(worker.ready());
        }
        worker.turn("测试失败提示").await.unwrap();
        let request = frame(&mut reader).await;
        worker
            .event(
                &state,
                Ok(json!({"id":request["id"],"result":{"turn":{"id":"failed-turn"}}})),
            )
            .await
            .unwrap();
        {
            let mut inner = state.assistant.inner.lock().await;
            inner.running = true;
            inner.messages = vec![
                Message {
                    id: "empty".into(),
                    role: "assistant",
                    text: String::new(),
                    status: "running",
                },
                Message {
                    id: "partial".into(),
                    role: "assistant",
                    text: "已有回复".into(),
                    status: "running",
                },
            ];
        }
        worker.event(&state, Ok(json!({"method":"error","params":{"threadId":"one-thread","turnId":"failed-turn",
            "willRetry":true,"error":{"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":503}},
                "message":"fixture-private-key"}}}))).await.unwrap();
        assert!(state.assistant.inner.lock().await.running);
        assert!(worker.has_active_turn());
        assert_eq!(worker.sequence, request["id"].as_u64().unwrap());
        worker.event(&state, Ok(json!({"method":"turn/completed","params":{"threadId":"one-thread",
            "turn":{"id":"failed-turn","status":"failed","error":{
                "message":"fixture-private-key","codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":401}}}}}}))).await.unwrap();
        {
            let inner = state.assistant.inner.lock().await;
            assert!(!inner.running);
            assert!(inner.detail.contains("HTTP 401"));
            assert_eq!(inner.messages[0].text, inner.detail);
            assert_eq!(inner.messages[0].status, "error");
            assert_eq!(inner.messages[1].text, "已有回复");
            assert!(!inner.detail.contains("fixture-private-key"));
        }
        assert!(worker.ready());
        assert_eq!(worker.thread.as_deref(), Some("one-thread"));
        squad.assistant_test_identity("generation-two").unwrap();
        assert!(worker.event(&state,Ok(json!({"method":"item/tool/call","id":"old-account",
            "params":{"threadId":"one-thread","turnId":"turn-two","tool":"hub_help","arguments":{}}}))).await.is_err());
    }
}
