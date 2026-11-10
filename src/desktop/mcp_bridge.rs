//! Streamable HTTP to STDIO for clients whose native MCP UI is HTTP only.
//! Processes start on an authenticated initialize, never while editing/scanning.
use super::{
    clients::ClientId,
    resources::{Library, ResourceKind},
};
use crate::app_state::SharedState;
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, broadcast, oneshot},
};

const MAX_MESSAGE: usize = 8 * 1024 * 1024;
const IDLE: Duration = Duration::from_secs(600);
struct Session {
    resource: String,
    config_path: std::path::PathBuf,
    fingerprint: String,
    child: Mutex<Child>,
    input: Mutex<ChildStdin>,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    events: broadcast::Sender<Value>,
    touched: Mutex<Instant>,
}
type Sessions = Mutex<HashMap<String, Arc<Session>>>;
static SESSIONS: OnceLock<Sessions> = OnceLock::new();
static SHUTTING_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn sessions() -> &'static Sessions {
    SESSIONS.get_or_init(Default::default)
}
pub(crate) async fn shutdown() {
    SHUTTING_DOWN.store(true, std::sync::atomic::Ordering::SeqCst);
    let active = std::mem::take(&mut *sessions().lock().await);
    for session in active.into_values() {
        session.pending.lock().await.clear();
        let mut child = session.child.lock().await;
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    }
}
pub fn router() -> Router<SharedState> {
    Router::new().route(
        "/api/desktop/mcp-bridge/{resource}",
        post(handle).get(events).delete(close),
    )
}
fn equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |v, (a, b)| v | (a ^ b)) == 0
}
fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32000,"message":message}})),
    )
        .into_response()
}
async fn authorize(
    state: &SharedState,
    resource: &str,
    headers: &HeaderMap,
) -> Result<super::mcp::McpServer> {
    ensure!(headers.get("origin").is_none(), "请通过客户端使用 MCP");
    ensure!(uuid::Uuid::parse_str(resource).is_ok(), "MCP 身份无效");
    let lib = Library::new(&state.config_path)?;
    let catalog = lib.load()?;
    let item = catalog
        .resources
        .iter()
        .find(|r| r.id == resource && r.kind == ResourceKind::Mcp)
        .context("MCP 已移除")?;
    let targets = lib.targets(&catalog)?;
    ensure!(
        catalog.uses.iter().any(|u| u.resource_id == resource
            && !u.pending
            && targets
                .iter()
                .any(|t| t.id == u.target_id && t.client == ClientId::Tiangong)),
        "此 MCP 未在天工使用"
    );
    let expected = format!("Bearer {}", lib.bridge_token(resource)?);
    ensure!(
        headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| equal(h, &expected)),
        "MCP 本机授权无效"
    );
    let server = lib.mcp_config(item)?;
    ensure!(server.transport() == "stdio", "此 MCP 不需要本地桥接");
    Ok(server)
}
async fn spawn(
    config_path: &std::path::Path,
    resource: &str,
    server: &super::mcp::McpServer,
) -> Result<(String, Arc<Session>)> {
    ensure!(
        !SHUTTING_DOWN.load(std::sync::atomic::Ordering::SeqCst),
        "Hub 正在退出"
    );
    let config = &server.config;
    let (program, prefix) = resolve_command(config["command"].as_str().context("MCP 命令为空")?)?;
    let mut command = Command::new(program);
    command.args(prefix);
    if let Some(args) = config["args"].as_array() {
        for arg in args {
            command.arg(arg.as_str().context("MCP 参数无效")?);
        }
    }
    command.env_clear();
    for key in [
        "PATH",
        "Path",
        "PATHEXT",
        "SYSTEMROOT",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "TEMP",
        "TMP",
        "TMPDIR",
        "USERPROFILE",
        "HOME",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    if let Some(names) = config["env_vars"].as_array() {
        for name in names {
            let key = name.as_str().context("MCP 环境变量名无效")?;
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
    }
    if let Some(env) = config["env"].as_object() {
        for (key, value) in env {
            command.env(key, value.as_str().context("MCP 环境变量值无效")?);
        }
    }
    if let Some(cwd) = config["cwd"].as_str().filter(|s| !s.is_empty()) {
        let path = std::path::Path::new(cwd);
        super::transaction::reject_links(path)?;
        ensure!(path.is_dir(), "MCP 工作目录不存在");
        command.current_dir(path);
    }
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    command
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| anyhow::anyhow!("无法启动 MCP，请在配置中检查命令及运行环境"))?;
    let input = child.stdin.take().context("无法准备 MCP 输入")?;
    let output = child.stdout.take().context("无法准备 MCP 输出")?;
    let (events, _) = broadcast::channel(128);
    let session = Arc::new(Session {
        resource: resource.into(),
        config_path: config_path.to_path_buf(),
        fingerprint: super::transaction::fingerprint(Some(&serde_json::to_vec(server)?)),
        child: Mutex::new(child),
        input: Mutex::new(input),
        pending: Default::default(),
        events,
        touched: Mutex::new(Instant::now()),
    });
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut pool = sessions().lock().await;
        ensure!(
            !SHUTTING_DOWN.load(std::sync::atomic::Ordering::SeqCst),
            "Hub 正在退出"
        );
        ensure!(pool.len() < 64, "MCP 会话过多，请关闭不再使用的客户端会话");
        pool.insert(id.clone(), session.clone());
    }
    let (read_id, read_session) = (id.clone(), session.clone());
    tokio::spawn(async move {
        let mut reader = BufReader::new(output);
        let mut line = Vec::new();
        loop {
            line.clear();
            // take() bounds a malicious/no-newline stdout message too.
            let count = tokio::io::AsyncReadExt::take(&mut reader, (MAX_MESSAGE + 1) as u64)
                .read_until(b'\n', &mut line)
                .await;
            if !matches!(count,Ok(n)if n>0&&n<=MAX_MESSAGE) {
                break;
            }
            let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                break;
            };
            if value["jsonrpc"] != "2.0" {
                break;
            }
            *read_session.touched.lock().await = Instant::now();
            if value.get("method").is_none() {
                if let Some(id) = value.get("id") {
                    if let Some(sender) = read_session.pending.lock().await.remove(&id.to_string())
                    {
                        let _ = sender.send(value);
                        continue;
                    }
                }
            }
            let _ = read_session.events.send(value);
        }
        read_session.pending.lock().await.clear();
        sessions().lock().await.remove(&read_id);
        let _ = read_session.child.lock().await.kill().await;
    });
    let (idle_id, idle_session) = (id.clone(), session.clone());
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            if !sessions().lock().await.contains_key(&idle_id) {
                break;
            }
            if idle_session.touched.lock().await.elapsed() > IDLE {
                sessions().lock().await.remove(&idle_id);
                let _ = idle_session.child.lock().await.kill().await;
                break;
            }
        }
    });
    Ok((id, session))
}
async fn find(
    state: &SharedState,
    resource: &str,
    headers: &HeaderMap,
    server: &super::mcp::McpServer,
) -> Result<(String, Arc<Session>)> {
    let id = headers
        .get("mcp-session-id")
        .and_then(|h| h.to_str().ok())
        .context("MCP 会话已结束，请重新连接")?;
    let session = sessions()
        .lock()
        .await
        .get(id)
        .cloned()
        .context("MCP 会话已结束，请重新连接")?;
    ensure!(
        session.resource == resource && session.config_path == state.config_path,
        "MCP 会话归属无效"
    );
    let fingerprint = super::transaction::fingerprint(Some(&serde_json::to_vec(server)?));
    if fingerprint != session.fingerprint {
        sessions().lock().await.remove(id);
        let _ = session.child.lock().await.kill().await;
        anyhow::bail!("MCP 配置已变化，请重新连接");
    }
    *session.touched.lock().await = Instant::now();
    Ok((id.into(), session))
}
async fn handle(
    State(state): State<SharedState>,
    Path(resource): Path<String>,
    headers: HeaderMap,
    Json(message): Json<Value>,
) -> Response {
    let server = match authorize(&state, &resource, &headers).await {
        Ok(v) => v,
        Err(e) => return error(StatusCode::UNAUTHORIZED, &e.to_string()),
    };
    if message["jsonrpc"] != "2.0"
        || !message.is_object()
        || serde_json::to_vec(&message).is_ok_and(|b| b.len() > MAX_MESSAGE)
    {
        return error(StatusCode::BAD_REQUEST, "MCP 消息无效");
    }
    let init = message["method"] == "initialize";
    if init && (message.get("id").is_none() || !message["params"].is_object()) {
        return error(StatusCode::BAD_REQUEST, "initialize 缺少请求 ID 或参数");
    }
    let result = if init && headers.get("mcp-session-id").is_none() {
        spawn(&state.config_path, &resource, &server).await
    } else {
        find(&state, &resource, &headers, &server).await
    };
    let (id, session) = match result {
        Ok(v) => v,
        Err(e) => {
            return error(
                if init {
                    StatusCode::BAD_GATEWAY
                } else {
                    StatusCode::NOT_FOUND
                },
                &e.to_string(),
            );
        }
    };
    let receiver = if message.get("method").is_some() {
        if let Some(request_id) = message.get("id") {
            let key = request_id.to_string();
            let mut pending = session.pending.lock().await;
            if pending.len() >= 128 || pending.contains_key(&key) {
                return error(StatusCode::CONFLICT, "MCP 请求 ID 重复或请求过多");
            }
            let (tx, rx) = oneshot::channel();
            pending.insert(key.clone(), tx);
            Some((key, rx))
        } else {
            None
        }
    } else {
        None
    };
    let mut guard = receiver.as_ref().map(|(key, _)| PendingRequest {
        session: session.clone(),
        key: key.clone(),
        request_id: message["id"].clone(),
        done: false,
    });
    let mut bytes = match serde_json::to_vec(&message) {
        Ok(v) => v,
        Err(_) => return error(StatusCode::BAD_REQUEST, "MCP 消息无效"),
    };
    bytes.push(b'\n');
    if session.input.lock().await.write_all(&bytes).await.is_err() {
        sessions().lock().await.remove(&id);
        session.pending.lock().await.clear();
        let _ = session.child.lock().await.kill().await;
        return error(StatusCode::BAD_GATEWAY, "MCP 已退出，请重新连接");
    }
    let _activity = (message["method"] == "tools/call" && message.get("id").is_some()).then(|| {
        state
            .desktop_mcp_activity
            .begin(&resource, &server.name, ClientId::Tiangong, false)
    });
    if let Some((_, rx)) = receiver {
        let value = match tokio::time::timeout(Duration::from_secs(600), rx).await {
            Ok(Ok(v)) => v,
            _ => {
                return error(
                    StatusCode::GATEWAY_TIMEOUT,
                    "MCP 未完成响应，请重新连接或检查服务",
                );
            }
        };
        if let Some(guard) = &mut guard {
            guard.done = true;
        }
        let mut response = Json(value).into_response();
        if let Ok(header) = id.parse() {
            response.headers_mut().insert("mcp-session-id", header);
        }
        response
    } else {
        StatusCode::ACCEPTED.into_response()
    }
}
async fn events(
    State(state): State<SharedState>,
    Path(resource): Path<String>,
    headers: HeaderMap,
) -> Response {
    let server = match authorize(&state, &resource, &headers).await {
        Ok(v) => v,
        Err(e) => return error(StatusCode::UNAUTHORIZED, &e.to_string()),
    };
    let (_, session) = match find(&state, &resource, &headers, &server).await {
        Ok(v) => v,
        Err(e) => return error(StatusCode::NOT_FOUND, &e.to_string()),
    };
    let stream = futures_util::stream::unfold(session.events.subscribe(), |mut rx| async move {
        match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            Ok(Ok(value)) => Some((
                Ok::<_, std::convert::Infallible>(format!("event: message\ndata: {value}\n\n")),
                rx,
            )),
            Err(_) => Some((Ok(": keepalive\n\n".to_string()), rx)),
            _ => None,
        }
    });
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-store")
        .body(Body::from_stream(stream))
        .unwrap()
}
async fn close(
    State(state): State<SharedState>,
    Path(resource): Path<String>,
    headers: HeaderMap,
) -> Response {
    let server = match authorize(&state, &resource, &headers).await {
        Ok(v) => v,
        Err(e) => return error(StatusCode::UNAUTHORIZED, &e.to_string()),
    };
    match find(&state, &resource, &headers, &server).await {
        Ok((id, session)) => {
            sessions().lock().await.remove(&id);
            let _ = session.child.lock().await.kill().await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => error(StatusCode::NOT_FOUND, &e.to_string()),
    }
}

pub async fn stop_resource(config_path: &std::path::Path, resource: &str) {
    let mut pool = sessions().lock().await;
    let ids = pool
        .iter()
        .filter(|(_, s)| s.config_path == config_path && s.resource == resource)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let removed = ids
        .into_iter()
        .filter_map(|id| pool.remove(&id))
        .collect::<Vec<_>>();
    drop(pool);
    for session in removed {
        session.pending.lock().await.clear();
        let _ = session.child.lock().await.kill().await;
    }
}

struct PendingRequest {
    session: Arc<Session>,
    key: String,
    request_id: Value,
    done: bool,
}
impl Drop for PendingRequest {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let session = self.session.clone();
        let key = self.key.clone();
        let id = self.request_id.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move{session.pending.lock().await.remove(&key);let line=format!("{}\n",json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"Client stopped waiting"}}));let _=session.input.lock().await.write_all(line.as_bytes()).await;});
        }
    }
}

fn resolve_command(raw: &str) -> Result<(std::path::PathBuf, Vec<std::path::PathBuf>)> {
    #[cfg(windows)]
    {
        let name = std::path::Path::new(raw)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(raw)
            .to_ascii_lowercase();
        if matches!(name.as_str(), "npm" | "npx" | "npm.cmd" | "npx.cmd") {
            let dirs = if std::path::Path::new(raw).components().count() > 1 {
                vec![
                    std::path::PathBuf::from(raw)
                        .parent()
                        .context("命令目录无效")?
                        .to_path_buf(),
                ]
            } else {
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                    .collect::<Vec<_>>()
            };
            for dir in dirs {
                let node = dir.join("node.exe");
                let script = dir
                    .join("node_modules/npm/bin")
                    .join(if name.starts_with("npx") {
                        "npx-cli.js"
                    } else {
                        "npm-cli.js"
                    });
                if node.is_file() && script.is_file() {
                    return Ok((node, vec![script]));
                }
            }
            anyhow::bail!(
                "没有找到 Node.js 的 npm/npx 运行文件，请安装 Node.js，或填写 node.exe 并将脚本作为第一个参数"
            );
        }
        ensure!(
            !name.ends_with(".cmd") && !name.ends_with(".bat"),
            "请直接填写可执行文件，或使用 node.exe 与脚本参数"
        );
    }
    Ok((raw.into(), vec![]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[tokio::test]
    async fn tool_activity_is_authenticated_tracks_real_forwarding_and_ends_with_response() {
        use super::super::{
            resources::UseRecord,
            targets::{McpFormat, target},
        };
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("held-tool.ps1");
        let release = dir.path().join("release");
        std::fs::write(
            &script,
            r#"param([string]$Release)
while ($null -ne ($line=[Console]::ReadLine())) {
  $v=$line | ConvertFrom-Json
  if ($v.method -eq 'tools/call') {
    while (-not (Test-Path -LiteralPath $Release)) { Start-Sleep -Milliseconds 20 }
  }
  if ($null -ne $v.id) {
    $reply=@{jsonrpc='2.0';id=$v.id;result=@{}}
    [Console]::WriteLine(($reply | ConvertTo-Json -Compress))
  }
}
"#,
        )
        .unwrap();
        let command = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let config_path = dir.path().join("hub.toml");
        let lib = Library::new(&config_path).unwrap();
        let server = super::super::mcp::McpServer {
            name: "synthetic tool".into(),
            config: json!({"command":command,"args":["-NoProfile","-NonInteractive","-File",script,"-Release",release]}),
        };
        let item = lib
            .save_mcp(None, server, String::new(), &lib.load().unwrap().revision)
            .unwrap();
        let target = target(
            ClientId::Tiangong,
            "fixture",
            dir.path().into(),
            dir.path().join("skills"),
            dir.path().join("unused.db"),
            McpFormat::Tiangong,
            "tiangong",
        );
        let mut catalog = lib.load().unwrap();
        catalog.uses.push(UseRecord {
            resource_id: item.id.clone(),
            target_id: target.id.clone(),
            location: target.mcp.clone(),
            files: vec![],
            entry_fingerprint: None,
            undo: "fixture".into(),
            pending: false,
        });
        catalog.target_overrides.push(target);
        super::super::transaction::atomic_write(
            &lib.root.join("library.json"),
            &serde_json::to_vec(&catalog).unwrap(),
        )
        .unwrap();
        let config = crate::config::AppConfig {
            state_path: dir.path().join("state.json"),
            ..Default::default()
        };
        let state = crate::app_state::AppState::new(config_path.clone(), config, None, None);
        let denied = handle(
            State(state.clone()),
            Path(item.id.clone()),
            HeaderMap::new(),
            Json(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}})),
        )
        .await;
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert!(state.desktop_mcp_activity.snapshot().is_empty());
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", lib.bridge_token(&item.id).unwrap())
                .parse()
                .unwrap(),
        );
        let initialized = handle(
            State(state.clone()),
            Path(item.id.clone()),
            headers.clone(),
            Json(json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{}})),
        )
        .await;
        assert!(initialized.status().is_success());
        assert!(
            state.desktop_mcp_activity.snapshot().is_empty(),
            "initialization is not a tool call"
        );
        headers.insert(
            "mcp-session-id",
            initialized.headers()["mcp-session-id"].clone(),
        );
        let (call_state, resource) = (state.clone(), item.id.clone());
        let call = tokio::spawn(async move {
            handle(State(call_state), Path(resource), headers,
            Json(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"fixture","arguments":{}}}))).await
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            while state.desktop_mcp_activity.snapshot().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let live = state.desktop_mcp_activity.snapshot();
        assert_eq!(live.len(), 1);
        assert!(live[0].active);
        assert_eq!(live[0].resource, item.id);
        assert_eq!(live[0].client, ClientId::Tiangong);
        std::fs::write(&release, b"release").unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), call)
            .await
            .unwrap()
            .unwrap();
        assert!(response.status().is_success());
        assert!(
            state
                .desktop_mcp_activity
                .snapshot()
                .iter()
                .all(|r| !r.active)
        );
        assert!(
            state.desktop_activity.snapshot().is_empty(),
            "tool calls must not become model requests"
        );
        stop_resource(&config_path, &item.id).await;
    }
    #[test]
    fn local_capabilities_require_exact_bearer() {
        assert!(equal("Bearer test", "Bearer test"));
        assert!(!equal("Bearer test", "Bearer tesT"));
        assert!(!equal("Bearer test", "Bearer test-more"));
    }
    #[cfg(windows)]
    #[tokio::test]
    async fn synthetic_stdio_cancellation_and_resource_close_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("synthetic-stdio.ps1");
        std::fs::write(&script,r#"param([string]$Marker)
while ($null -ne ($line=[Console]::ReadLine())) {
  $v=$line | ConvertFrom-Json
  if ($v.method -eq 'notifications/cancelled') { [IO.File]::WriteAllText($Marker,[string]$v.params.requestId) }
  elseif ($v.method -eq 'initialize') { [Console]::WriteLine(('{"jsonrpc":"2.0","id":'+$v.id+',"result":{"protocolVersion":"2025-03-26","capabilities":{},"serverInfo":{"name":"synthetic","version":"1"}}}')) }
}
"#).unwrap();
        let program = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let marker = dir.path().join("cancelled.txt");
        let server = super::super::mcp::McpServer {
            name: "synthetic".into(),
            config: json!({"command":program,"args":["-NoProfile","-NonInteractive","-File",script.to_str().unwrap(),"-Marker",marker.to_str().unwrap()]}),
        };
        let config = dir.path().join("hub.toml");
        let (id, a) = spawn(&config, "synthetic-a", &server).await.unwrap();
        let (tx, rx) = oneshot::channel();
        a.pending.lock().await.insert("1".into(), tx);
        a.input
            .lock()
            .await
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
            .await
            .unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reply["result"]["serverInfo"]["name"], "synthetic");
        let (tx, _) = oneshot::channel();
        a.pending.lock().await.insert("2".into(), tx);
        drop(PendingRequest {
            session: a.clone(),
            key: "2".into(),
            request_id: json!(2),
            done: false,
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            while !marker.is_file() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "2");
        assert!(a.pending.lock().await.is_empty());
        let (other, b) = spawn(&config, "synthetic-b", &server).await.unwrap();
        stop_resource(&config, "synthetic-a").await;
        assert!(!sessions().lock().await.contains_key(&id));
        assert!(sessions().lock().await.contains_key(&other));
        assert!(a.child.lock().await.try_wait().unwrap().is_some());
        assert!(b.child.lock().await.try_wait().unwrap().is_none());
        stop_resource(&config, "synthetic-b").await;
        assert!(b.child.lock().await.try_wait().unwrap().is_some());
    }
}
