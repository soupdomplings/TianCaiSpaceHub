//! Read installed metadata and prepare our small preload; never edit the install.
use super::*;
use crate::desktop::renderer_cdp::Cdp;
use futures_util::{SinkExt, StreamExt};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    process::{Child, ChildStderr, Command, Stdio},
};
use tokio_tungstenite::tungstenite::Message;

const ANCHOR: &str = "let ye=createSkillVlmService({authenticationManager:e.authenticationManager,productManager:e.productManager,appVersion:r.appVersion,fetch:s,logger:O});";
const BOOTSTRAP: &[u8] = include_bytes!("host-bridge.cjs");
const MODEL_CATALOG: &[u8] = include_bytes!("model-catalog.cjs");
const MAIN_ANCHOR: &str = "new DaemonAppServerProcessManager({env:u,requestHandler:";

pub(super) struct Prepared {
    executable: PathBuf,
    bootstrap: PathBuf,
    server: PathBuf,
    main_bootstrap: PathBuf,
}

pub(super) enum Outcome {
    NeedsRestart,
    Ready(Prepared),
}

pub(super) fn prepare(config_path: &Path) -> Result<Outcome> {
    ensure!(
        cfg!(windows),
        "WorkBuddy 内置模型分享当前仅适配 Windows；macOS 待单独验证"
    );
    let (running, discovered) = crate::web::workbuddy_launch::discover()?;
    if running {
        return Ok(Outcome::NeedsRestart);
    }
    let executable = if let Some(configured) = std::env::var_os("WORKBUDDY_DESKTOP_PATH") {
        crate::web::workbuddy_launch::resolve_executable(&PathBuf::from(configured))
    } else {
        discovered
    }
    .context("未找到 WorkBuddy，请先安装，或设置 WORKBUDDY_DESKTOP_PATH 指定桌面程序")?;
    let resources = executable
        .parent()
        .context("WorkBuddy 安装路径无效")?
        .join("resources");
    verify_archive(&resources.join("app.asar"))?;
    verify_fuses(&executable)?;
    let root = super::super::state_root(config_path).join("workbuddy-host");
    fs::create_dir_all(&root).context("无法准备 WorkBuddy 本机分享适配器")?;
    let bootstrap = root.join("host-bridge.cjs");
    let catalog = root.join("model-catalog.cjs");
    if fs::read(&catalog).ok().as_deref() != Some(MODEL_CATALOG) {
        super::super::transaction::atomic_write(&catalog, MODEL_CATALOG)?;
    }
    if fs::read(&bootstrap).ok().as_deref() != Some(BOOTSTRAP) {
        super::super::transaction::atomic_write(&bootstrap, BOOTSTRAP)?;
    }
    Ok(Outcome::Ready(Prepared {
        executable,
        bootstrap,
        server: resources.join("app.asar/main/server.js"),
        main_bootstrap: resources.join("app.asar/main/main-bootstrap.js"),
    }))
}

// Exact installation entries are read without executing vendor JavaScript.
fn archive_entry(file: &mut fs::File, tree: &Value, data: u64, key: &str) -> Result<Vec<u8>> {
    let mut entry = tree;
    for component in key.split('/') {
        entry = &entry["files"][component];
    }
    ensure!(
        entry["unpacked"] != true && entry.get("link").is_none(),
        "WorkBuddy 安装资源布局已变化，暂未启用分享"
    );
    let size = entry["size"]
        .as_u64()
        .filter(|size| *size <= 8 * 1024 * 1024)
        .context("WorkBuddy 安装资源不兼容")?;
    let offset = entry["offset"]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .context("WorkBuddy 安装资源位置无效")?;
    let start = data
        .checked_add(offset)
        .context("WorkBuddy 安装资源位置无效")?;
    ensure!(
        start
            .checked_add(size)
            .is_some_and(|end| end <= file.metadata().map(|m| m.len()).unwrap_or(0)),
        "WorkBuddy 安装资源不完整"
    );
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; size as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn verify_archive(path: &Path) -> Result<()> {
    let mut file = fs::File::open(path).context("无法读取 WorkBuddy 安装资源")?;
    let mut header = [0; 16];
    file.read_exact(&mut header)?;
    let value = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap()) as u64;
    let size = value(12);
    ensure!(
        value(0) == 4 && (1..=16 * 1024 * 1024).contains(&size) && value(4) >= size + 8,
        "WorkBuddy ASAR 目录格式不兼容"
    );
    let data = 8 + value(4);
    let mut bytes = vec![0; size as usize];
    file.read_exact(&mut bytes)?;
    let tree: Value = serde_json::from_slice(&bytes).context("WorkBuddy ASAR 目录无效")?;
    let package: Value =
        serde_json::from_slice(&archive_entry(&mut file, &tree, data, "package.json")?)?;
    ensure!(
        package["version"] == "5.9.3",
        "当前 WorkBuddy 版本尚未适配内置模型分享；本轮支持 Windows 5.9.3，请保留普通启动"
    );
    ensure!(
        matches!(
            package["main"].as_str(),
            Some("main/index.js" | "./main/index.js")
        ),
        "WorkBuddy 主程序入口已变化，暂未启用分享"
    );
    let server = archive_entry(&mut file, &tree, data, "main/server.js")?;
    let source = std::str::from_utf8(&server).context("WorkBuddy 宿主脚本格式不兼容")?;
    ensure!(
        source.matches(ANCHOR).count() == 1 && source.contains("coordinator:"),
        "WorkBuddy 宿主接线位置已变化，暂未启用分享"
    );
    let bootstrap = archive_entry(&mut file, &tree, data, "main/main-bootstrap.js")?;
    let bootstrap = std::str::from_utf8(&bootstrap).context("WorkBuddy 主程序脚本格式不兼容")?;
    ensure!(
        bootstrap.matches(MAIN_ANCHOR).count() == 1
            && bootstrap.contains("this.options.processExecArgv"),
        "WorkBuddy 子进程启动入口已变化，暂未启用分享"
    );
    Ok(())
}

fn verify_fuses(path: &Path) -> Result<()> {
    const SENTINEL: &[u8] = b"dL7pKGdnNz796PbbjQWNKmHXBZaB9tsX";
    let mut file = fs::File::open(path)?;
    let mut tail = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..count]);
        if let Some(index) = tail
            .windows(SENTINEL.len())
            .position(|value| value == SENTINEL)
        {
            let wire = &tail[index + SENTINEL.len()..];
            if wire.len() >= 10 {
                ensure!(
                    wire[0] == 1 && wire[1] >= 8 && wire[2] == b'1' && wire[5] == b'1',
                    "当前 WorkBuddy 不允许本机分享适配启动；保留原程序，暂未启用分享"
                );
                return Ok(());
            }
        }
        let keep = tail.len().saturating_sub(64);
        tail.drain(..keep);
    }
    anyhow::bail!("当前 WorkBuddy 启动格式不兼容，暂未启用内置模型分享")
}

impl Prepared {
    pub(super) async fn launch(self, callback: &str, lease: &str, bridge_id: &str) -> Result<()> {
        // Packaged Electron strips --require from NODE_OPTIONS and overwrites
        // that environment variable. Use its supported, temporary main-process
        // inspector before the original entry executes, then resume it normally.
        let reservation = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let port = reservation.local_addr()?.port();
        drop(reservation);
        let mut command = Command::new(&self.executable);
        command
            .current_dir(self.executable.parent().context("WorkBuddy 安装目录无效")?)
            .env_remove("ELECTRON_RUN_AS_NODE")
            .arg(format!("--inspect-brk=127.0.0.1:{port}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .context("无法启动 WorkBuddy 分享，请检查程序权限")?;
        // Only the native inspector URI emitted by this exact Child is used.
        // Drain and discard everything else; never retain client log content.
        let address = startup_stderr(
            child
                .stderr
                .take()
                .context("无法读取本次 WorkBuddy 启动通道")?,
            port,
        );
        let options = json!({"callback":callback,"lease":lease,"bridgeId":bridge_id,"captureAuthorization":true,
            "role":"main","bootstrap":self.bootstrap,"server":self.server,
            "mainBootstrap":self.main_bootstrap});
        let mut release_attempted = false;
        let mut native_waiting = false;
        let result = attach_startup(
            address,
            &mut child,
            &self.bootstrap,
            options,
            &mut native_waiting,
            &mut release_attempted,
        )
        .await;
        let result = match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let restored =
                    restore_unreleased(&mut child, native_waiting, release_attempted, || {
                        let mut normal = Command::new(&self.executable);
                        normal
                            .current_dir(
                                self.executable.parent().context("WorkBuddy 安装目录无效")?,
                            )
                            .env_remove("ELECTRON_RUN_AS_NODE")
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null());
                        #[cfg(windows)]
                        {
                            use std::os::windows::process::CommandExt;
                            normal.creation_flags(0x08000000);
                        }
                        let mut ordinary = normal.spawn().context(
                            "分享接线未完成，普通启动也未成功，请从原程序入口打开 WorkBuddy",
                        )?;
                        std::thread::spawn(move || {
                            let _ = ordinary.wait();
                        });
                        Ok(())
                    });
                match restored {
                    Ok(true) => Err(anyhow::anyhow!(
                        "WorkBuddy 分享未启用：{error}。已恢复普通启动，可直接使用客户端"
                    )),
                    Ok(false) => Err(anyhow::anyhow!(
                        "WorkBuddy 分享未启用：{error}。请正常退出 WorkBuddy 后重试或从原入口普通打开"
                    )),
                    Err(recovery) => {
                        Err(anyhow::anyhow!("WorkBuddy 分享未启用：{error}。{recovery}"))
                    }
                }
            }
        };
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        result
    }
}

fn restore_unreleased(
    child: &mut Child,
    native_waiting: bool,
    release_attempted: bool,
    normal: impl FnOnce() -> Result<()>,
) -> Result<bool> {
    // A failed inspector bind can continue the ordinary user entry without
    // publishing a URI. Only a confirmed native waiting startup can be replaced;
    // a lost release reply still forbids replacement of this exact Child.
    if !native_waiting || release_attempted || child.try_wait()?.is_some() {
        return Ok(false);
    }
    child
        .kill()
        .context("本次暂停的 WorkBuddy 启动未能结束，请正常退出后普通打开客户端")?;
    child.wait()?;
    normal()?;
    Ok(true)
}

fn startup_url(line: &str, port: u16) -> Option<url::Url> {
    let url = url::Url::parse(
        line.trim_end_matches('\r')
            .strip_prefix("Debugger listening on ")?,
    )
    .ok()?;
    (url.scheme() == "ws"
        && url.host_str() == Some("127.0.0.1")
        && url.port() == Some(port)
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && uuid::Uuid::parse_str(url.path().strip_prefix('/')?).is_ok())
    .then_some(url)
}

fn read_startup_address(mut stderr: impl Read, port: u16, sender: oneshot::Sender<url::Url>) {
    let mut sender = Some(sender);
    let mut line = Vec::new();
    let mut overflow = false;
    let mut bytes = [0_u8; 512];
    loop {
        let Ok(count) = stderr.read(&mut bytes) else {
            break;
        };
        if count == 0 {
            break;
        }
        for byte in &bytes[..count] {
            if *byte == b'\n' {
                if !overflow
                    && let Ok(text) = std::str::from_utf8(&line)
                    && let Some(url) = startup_url(text, port)
                    && let Some(sender) = sender.take()
                {
                    let _ = sender.send(url);
                }
                line.clear();
                overflow = false;
            } else if line.len() < 256 {
                line.push(*byte);
            } else {
                overflow = true;
            }
        }
    }
}

fn startup_stderr(stderr: ChildStderr, port: u16) -> oneshot::Receiver<url::Url> {
    let (sender, receiver) = oneshot::channel();
    std::thread::spawn(move || read_startup_address(stderr, port, sender));
    receiver
}

// Main-process debugger events are private to this startup adapter. Renderer
// Fetch.requestPaused filtering remains unchanged for the other clients.
struct StartupInspector {
    cdp: Cdp,
    sequence: u32,
    entry: PathBuf,
    entry_script: Option<String>,
    paused: Option<Value>,
}
impl StartupInspector {
    fn observe(&mut self, value: &Value) {
        if value["method"] == "Debugger.scriptParsed"
            && let Some(source) = value["params"]["url"].as_str()
        {
            let source = url::Url::parse(source)
                .ok()
                .and_then(|u| u.to_file_path().ok())
                .unwrap_or_else(|| PathBuf::from(source));
            if source
                .to_string_lossy()
                .replace('\\', "/")
                .eq_ignore_ascii_case(&self.entry.to_string_lossy().replace('\\', "/"))
            {
                self.entry_script = value["params"]["scriptId"].as_str().map(str::to_owned);
            }
        }
        if value["method"] == "Debugger.paused" {
            self.paused = Some(value["params"].clone());
        }
    }
    async fn receive(&mut self) -> Result<Value> {
        loop {
            match self.cdp.socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let value =
                        serde_json::from_str(&text).context("WorkBuddy 启动通道格式不兼容")?;
                    self.observe(&value);
                    return Ok(value);
                }
                Some(Ok(Message::Ping(bytes))) => {
                    self.cdp.socket.send(Message::Pong(bytes)).await?
                }
                Some(Ok(Message::Close(_))) | None => anyhow::bail!("WorkBuddy 启动通道已关闭"),
                Some(Err(_)) => anyhow::bail!("WorkBuddy 启动通道已中断"),
                _ => {}
            }
        }
    }
    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.sequence += 1;
        let id = self.sequence;
        tokio::time::timeout(Duration::from_secs(4), async {
            self.cdp
                .socket
                .send(Message::Text(
                    json!({"id":id,"method":method,"params":params}).to_string(),
                ))
                .await?;
            loop {
                let value = self.receive().await?;
                if value["id"].as_u64() == Some(id as u64) {
                    ensure!(value.get("error").is_none(), "WorkBuddy 启动调试协议不兼容");
                    return Ok(value["result"].clone());
                }
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("WorkBuddy 启动接线通信超时"))?
    }
    async fn evaluate(&mut self, expression: &str) -> Result<Value> {
        let result = self
            .call(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true,"silent":true}),
            )
            .await?;
        ensure!(
            result.get("exceptionDetails").is_none(),
            "WorkBuddy 启动适配器未完成加载"
        );
        result["result"]
            .get("value")
            .cloned()
            .context("WorkBuddy 启动接线响应缺失")
    }
    async fn enter_main(&mut self, release_attempted: &mut bool) -> Result<()> {
        self.call("Runtime.enable", json!({})).await?;
        self.call("Debugger.enable", json!({})).await?;
        let entry = url::Url::from_file_path(&self.entry)
            .map_err(|_| anyhow::anyhow!("WorkBuddy 主程序路径无效"))?;
        self.call(
            "Debugger.setBreakpointByUrl",
            json!({"url":entry.as_str(),"lineNumber":0}),
        )
        .await?;
        // Before runIfWaiting, Node's process.pid/env are not initialized yet.
        // Source ownership is already established by this Child's native URI.
        *release_attempted = true;
        self.call("Runtime.runIfWaitingForDebugger", json!({}))
            .await?;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                while self.paused.is_none() {
                    self.receive().await?;
                }
                let paused = self.paused.take().unwrap();
                if self
                    .entry_script
                    .as_deref()
                    .is_some_and(|id| paused["callFrames"][0]["location"]["scriptId"] == id)
                {
                    return Ok(());
                }
                *release_attempted = true;
                self.call("Debugger.resume", json!({})).await?;
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("WorkBuddy 未在本次主程序入口完成接线暂停"))?
    }
    async fn close_socket(&mut self) {
        let _ = self.cdp.socket.close(None).await;
    }
}

async fn attach_startup(
    address: oneshot::Receiver<url::Url>,
    child: &mut Child,
    bootstrap: &Path,
    options: Value,
    native_waiting: &mut bool,
    release_attempted: &mut bool,
) -> Result<()> {
    let socket = tokio::time::timeout(Duration::from_secs(10), address)
        .await
        .map_err(|_| anyhow::anyhow!("WorkBuddy 未发布本次启动接线通道"))?
        .context("WorkBuddy 在发布本次启动通道前已退出")?;
    *native_waiting = true;
    ensure!(
        child.try_wait()?.is_none(),
        "WorkBuddy 本次启动已退出或转交，请先正常退出客户端"
    );
    let cdp = tokio::time::timeout(Duration::from_secs(3), Cdp::connect(&socket, "WorkBuddy"))
        .await
        .map_err(|_| anyhow::anyhow!("WorkBuddy 本机启动通道连接未完成"))??;
    let mut inspector = StartupInspector {
        cdp,
        sequence: 0,
        entry: bootstrap_entry(&options)?,
        entry_script: None,
        paused: None,
    };
    if let Err(error) = inspector.enter_main(release_attempted).await {
        if *release_attempted {
            // Our URI is owned, but entry discovery failed. Remove debugger
            // pauses and close the temporary endpoint where runtime is ready.
            let _ = inspector.call("Debugger.disable", json!({})).await;
            let _ = inspector
                .evaluate(&close_expression(socket.port().unwrap()))
                .await;
        }
        inspector.close_socket().await;
        return Err(error);
    }
    let pid = match inspector.evaluate("process.pid").await {
        Ok(pid) => pid,
        Err(error) => {
            let _ =
                resume_and_close(&mut inspector, socket.port().unwrap(), release_attempted).await;
            return Err(error);
        }
    };
    if pid.as_u64() != Some(child.id() as u64) {
        inspector.close_socket().await;
        anyhow::bail!("WorkBuddy 启动通道不属于本次启动，未接入其他程序");
    }
    install_and_resume(
        &mut inspector,
        bootstrap,
        options,
        socket.port().unwrap(),
        release_attempted,
    )
    .await
}

fn bootstrap_entry(options: &Value) -> Result<PathBuf> {
    let main = options["mainBootstrap"]
        .as_str()
        .context("WorkBuddy 主程序入口缺失")?;
    Ok(PathBuf::from(main).with_file_name("index.js"))
}

async fn install_and_resume(
    inspector: &mut StartupInspector,
    bootstrap: &Path,
    options: Value,
    port: u16,
    release_attempted: &mut bool,
) -> Result<()> {
    let file = json!(bootstrap.to_string_lossy());
    let expression = format!(
        "(()=>{{const loader=process.getBuiltinModule('module').createRequire({file})({file});return loader.install({options});}})()"
    );
    let installed = inspector.evaluate(&expression).await.and_then(|result| {
        ensure!(
            result["installed"] == true && result["role"] == "main",
            "WorkBuddy 主程序接线未确认，未启用分享"
        );
        Ok(())
    });
    // Run this on both success and failure, before propagating any error. The
    // scheduled close runs after our websocket disconnects; the client keeps
    // its original startup and is never left paused by a failed preload.
    let resumed = resume_and_close(inspector, port, release_attempted).await;
    installed?;
    resumed?;
    Ok(())
}

fn close_expression(port: u16) -> String {
    let argument = json!(format!("--inspect-brk=127.0.0.1:{port}"));
    format!(
        "(()=>{{if(!Number.isInteger(process.pid))return false;for(let i=process.execArgv.length-1;i>=0;i--)if(process.execArgv[i]==={argument})process.execArgv.splice(i,1);setImmediate(()=>process.getBuiltinModule('inspector').close());return true;}})()"
    )
}

async fn resume_and_close(
    inspector: &mut StartupInspector,
    port: u16,
    release_attempted: &mut bool,
) -> Result<()> {
    let closing = inspector.evaluate(&close_expression(port)).await;
    // Set this before sending, including a lost response or send failure. Once
    // resume may have reached the client, it must never be terminated/replaced.
    *release_attempted = true;
    let resumed = inspector.call("Debugger.resume", json!({})).await;
    inspector.close_socket().await;
    closing?;
    resumed?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn asar_anchor_and_version_are_checked_without_install_writes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app.asar");
        let write = |version: &str, source: &str, bootstrap: &str| {
            let package =
                serde_json::to_vec(&json!({"version":version,"main":"main/index.js"})).unwrap();
            let tree = serde_json::to_vec(&json!({"files":{"package.json":{"size":package.len(),"offset":"0"},"main":{"files":{
                "server.js":{"size":source.len(),"offset":package.len().to_string()},
                "main-bootstrap.js":{"size":bootstrap.len(),"offset":(package.len()+source.len()).to_string()}
            }}}})).unwrap();
            let padded = (tree.len() + 3) & !3;
            let mut bytes = Vec::new();
            for word in [
                4_u32,
                (padded + 8) as u32,
                (padded + 4) as u32,
                tree.len() as u32,
            ] {
                bytes.extend(word.to_le_bytes());
            }
            bytes.extend(tree);
            bytes.resize(16 + padded, 0);
            bytes.extend(package);
            bytes.extend(source.as_bytes());
            bytes.extend(bootstrap.as_bytes());
            fs::write(&file, bytes).unwrap();
        };
        let bootstrap = format!("{MAIN_ANCHOR} this.options.processExecArgv");
        write("5.9.3", &format!("{ANCHOR} coordinator:"), &bootstrap);
        verify_archive(&file).unwrap();
        write("5.9.4", &format!("{ANCHOR} coordinator:"), &bootstrap);
        assert!(verify_archive(&file).is_err());
        write(
            "5.9.3",
            &format!("{ANCHOR}{ANCHOR} coordinator:"),
            &bootstrap,
        );
        assert!(verify_archive(&file).is_err());
        write(
            "5.9.3",
            &format!("{ANCHOR} coordinator:"),
            &format!("{bootstrap}{MAIN_ANCHOR}"),
        );
        assert!(verify_archive(&file).is_err());
        write("5.9.3", &format!("{ANCHOR} coordinator:"), MAIN_ANCHOR);
        assert!(verify_archive(&file).is_err());
    }

    #[tokio::test]
    async fn startup_address_comes_only_from_owned_native_stderr() {
        let id = "00000000-0000-4000-8000-000000000001";
        let address = format!("Debugger listening on ws://127.0.0.1:49001/{id}");
        assert!(startup_url(&address, 49002).is_none());
        assert!(startup_url(&address.replace("127.0.0.1", "localhost"), 49001).is_none());
        assert!(startup_url(&format!("some client text: {address}"), 49001).is_none());
        assert!(startup_url(&format!("{address}?ignored=true"), 49001).is_none());
        let stream = format!(
            "{}\n{}\n{address}\r\nclient log content discarded\n",
            "x".repeat(900),
            address.replace("49001", "49002")
        );
        let (sender, receiver) = oneshot::channel();
        read_startup_address(std::io::Cursor::new(stream.as_bytes()), 49001, sender);
        assert_eq!(
            receiver.await.unwrap().as_str(),
            format!("ws://127.0.0.1:49001/{id}")
        );
    }

    async fn request(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        method: &str,
    ) -> Value {
        let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
            panic!("expected protocol request");
        };
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["method"], method);
        value
    }
    async fn reply(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        call: &Value,
        result: Value,
    ) {
        socket
            .send(Message::Text(
                json!({"id":call["id"],"result":result}).to_string(),
            ))
            .await
            .unwrap();
    }

    async fn inspector_fixture(
        entry: &Path,
    ) -> (
        StartupInspector,
        tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let url = url::Url::parse(&format!("ws://{address}")).unwrap();
        let accept = async {
            let (stream, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(stream).await.unwrap()
        };
        let (cdp, server) = tokio::join!(Cdp::connect(&url, "WorkBuddy"), accept);
        (
            StartupInspector {
                cdp: cdp.unwrap(),
                sequence: 0,
                entry: entry.to_owned(),
                entry_script: None,
                paused: None,
            },
            server,
        )
    }

    async fn startup_commands(
        socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        entry: &str,
    ) {
        for method in [
            "Runtime.enable",
            "Debugger.enable",
            "Debugger.setBreakpointByUrl",
        ] {
            let call = request(socket, method).await;
            if method == "Debugger.setBreakpointByUrl" {
                assert_eq!(call["params"]["url"], entry);
            }
            reply(socket, &call, json!({})).await;
        }
    }

    #[tokio::test]
    async fn startup_entry_pause_survives_command_replies_and_failed_preload_still_resumes() {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("index.js");
        let entry_url = url::Url::from_file_path(&entry).unwrap().to_string();
        let (mut inspector, mut socket) = inspector_fixture(&entry).await;
        let server = tokio::spawn(async move {
            startup_commands(&mut socket, &entry_url).await;
            let run = request(&mut socket, "Runtime.runIfWaitingForDebugger").await;
            // Internal startup pauses can arrive before the command reply.
            socket.send(Message::Text(json!({"method":"Debugger.paused","params":{"callFrames":[{"location":{"scriptId":"internal"}}]}}).to_string())).await.unwrap();
            reply(&mut socket, &run, json!({})).await;
            let resume = request(&mut socket, "Debugger.resume").await;
            socket.send(Message::Text(json!({"method":"Debugger.scriptParsed","params":{"scriptId":"entry","url":entry_url}}).to_string())).await.unwrap();
            socket.send(Message::Text(json!({"method":"Debugger.paused","params":{"callFrames":[{"location":{"scriptId":"entry"}}]}}).to_string())).await.unwrap();
            reply(&mut socket, &resume, json!({})).await;
            let install = request(&mut socket, "Runtime.evaluate").await;
            assert!(
                install["params"]["expression"]
                    .as_str()
                    .unwrap()
                    .contains("createRequire")
            );
            reply(
                &mut socket,
                &install,
                json!({"exceptionDetails":{"text":"fixture load failed"}}),
            )
            .await;
            let close = request(&mut socket, "Runtime.evaluate").await;
            let expression = close["params"]["expression"].as_str().unwrap();
            assert!(expression.contains("setImmediate"));
            assert!(expression.contains("getBuiltinModule('inspector').close()"));
            assert!(expression.contains("--inspect-brk=127.0.0.1:49001"));
            reply(&mut socket, &close, json!({"result":{"value":true}})).await;
            let resume = request(&mut socket, "Debugger.resume").await;
            reply(&mut socket, &resume, json!({})).await;
            assert!(matches!(socket.next().await, Some(Ok(Message::Close(_)))));
        });
        let mut released = false;
        inspector.enter_main(&mut released).await.unwrap();
        assert!(released);
        let error = install_and_resume(
            &mut inspector,
            &directory.path().join("host-bridge.cjs"),
            json!({}),
            49001,
            &mut released,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("未完成加载"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn lost_release_reply_marks_the_original_child_as_never_replaceable() {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("index.js");
        let entry_url = url::Url::from_file_path(&entry).unwrap().to_string();
        let (mut inspector, mut socket) = inspector_fixture(&entry).await;
        let server = tokio::spawn(async move {
            startup_commands(&mut socket, &entry_url).await;
            request(&mut socket, "Runtime.runIfWaitingForDebugger").await;
            // The command may have reached the child even if its reply is lost.
            socket.close(None).await.unwrap();
        });
        let mut released = false;
        assert!(inspector.enter_main(&mut released).await.is_err());
        assert!(released);
        #[cfg(windows)]
        {
            let mut child = dormant_child();
            let mut restarted = false;
            let preserved = restore_unreleased(&mut child, true, released, || {
                restarted = true;
                Ok(())
            })
            .unwrap();
            let running = child.try_wait().unwrap().is_none();
            child.kill().unwrap();
            child.wait().unwrap();
            assert!(!preserved && !restarted && running);
        }
        server.await.unwrap();
    }

    #[cfg(windows)]
    fn dormant_child() -> Child {
        use std::os::windows::process::CommandExt;
        // An owned synthetic shell blocked on its input, with no user files,
        // tasks or descendant processes. No installed client is launched.
        Command::new("cmd.exe")
            .args(["/D", "/Q", "/K"])
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn unreleased_child_fallback_requires_the_owned_native_startup_address() {
        let mut child = dormant_child();
        let mut restarts = 0;
        // Timeout/EOF before the native URI cannot prove an entry is paused.
        assert!(
            !restore_unreleased(&mut child, false, false, || {
                restarts += 1;
                Ok(())
            })
            .unwrap()
        );
        assert!(child.try_wait().unwrap().is_none());
        assert!(
            restore_unreleased(&mut child, true, false, || {
                restarts += 1;
                Ok(())
            })
            .unwrap()
        );
        assert!(child.try_wait().unwrap().is_some());
        assert!(
            !restore_unreleased(&mut child, true, false, || {
                restarts += 1;
                Ok(())
            })
            .unwrap()
        );
        assert_eq!(restarts, 1);
    }
}
