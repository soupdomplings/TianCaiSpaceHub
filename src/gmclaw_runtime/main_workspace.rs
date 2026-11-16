//! Transient native account scope for the 1.1 Work DataServer. Only an explicit
//! enhanced launch enables the main inspector; a verified connection closes it
//! after LiveEdit. Early connection failures require a normal desktop exit.
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use crate::{desktop::renderer_cdp::Cdp, gmclaw_im::GmClawBridgeConfig};

const ADAPTER: &str = include_str!("main-workspace-adapter.js");
const CONTEXT: &str = include_str!("main-workspace-context.js");
const ACCOUNT: &str = "conversationAccount: () => db ? readClawSession(db)?.userId || \"\" : \"\",";
const TIMEOUT: Duration = Duration::from_secs(12);

pub(super) struct Prepared {
    pub port: u16,
    entry: PathBuf,
}

/// Inspect just the installed entry. Older layouts keep their renderer-only
/// startup; no version pin, copied application or per-file hashes are needed.
pub(super) fn prepare(executable: &Path) -> Result<Option<Prepared>> {
    let Some(entry) = native_entry(executable)? else {
        return Ok(None);
    };
    let reservation = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let port = reservation.local_addr()?.port();
    Ok(Some(Prepared { port, entry }))
}

fn native_entry(executable: &Path) -> Result<Option<PathBuf>> {
    #[cfg(windows)]
    let resources = executable
        .parent()
        .context("天工安装位置无效")?
        .join("resources");
    #[cfg(target_os = "macos")]
    let resources = executable
        .parent()
        .and_then(Path::parent)
        .context("天工安装位置无效")?
        .join("Resources");
    #[cfg(not(any(windows, target_os = "macos")))]
    let resources = executable
        .parent()
        .context("天工安装位置无效")?
        .join("resources");
    let archive = resources.join("app.asar");
    let source = read_main(&archive)?;
    if !source.contains("conversationAccount:") {
        return Ok(None);
    }
    ensure!(
        source.matches(ACCOUNT).count() == 1,
        "当前天工的本地会话结构尚未适配，请先使用普通启动"
    );
    Ok(Some(archive.join("out/main/index.js")))
}

fn read_main(archive: &Path) -> Result<String> {
    let mut file = std::fs::File::open(archive).context("无法读取天工安装资源")?;
    let mut header = [0_u8; 16];
    file.read_exact(&mut header)?;
    let number = |start| u32::from_le_bytes(header[start..start + 4].try_into().unwrap()) as u64;
    let size = number(12);
    ensure!(
        number(0) == 4 && (1..=16 * 1024 * 1024).contains(&size) && number(4) >= size + 8,
        "天工安装资源目录不兼容"
    );
    let mut bytes = vec![0_u8; size as usize];
    file.read_exact(&mut bytes)?;
    let tree: Value = serde_json::from_slice(&bytes)?;
    let entry = &tree["files"]["out"]["files"]["main"]["files"]["index.js"];
    let size = entry["size"]
        .as_u64()
        .filter(|n| *n <= 8 * 1024 * 1024)
        .context("天工主程序资源不兼容")?;
    let offset = entry["offset"]
        .as_str()
        .and_then(|n| n.parse::<u64>().ok())
        .context("天工主程序资源位置无效")?;
    let start = (8 + number(4))
        .checked_add(offset)
        .context("天工主程序资源位置无效")?;
    ensure!(
        entry["unpacked"] != true
            && entry.get("link").is_none()
            && start
                .checked_add(size)
                .is_some_and(|end| end <= file.metadata().map(|m| m.len()).unwrap_or(0)),
        "天工主程序资源不完整"
    );
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0_u8; size as usize];
    file.read_exact(&mut bytes)?;
    String::from_utf8(bytes).context("天工主程序脚本格式不兼容")
}

fn local_http() -> Result<Client> {
    Ok(Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()?)
}

pub(super) async fn check_existing(
    config: &GmClawBridgeConfig,
    executable: Option<&Path>,
) -> Result<()> {
    let executable = match executable.and_then(super::resolve_executable) {
        Some(executable) => executable,
        None => super::discover()?.executable.context("未找到天工安装")?,
    };
    if native_entry(&executable)?.is_some() {
        ensure!(
            gateway_ready(
                &local_http()?,
                &super::connection_authorization(config).await?
            )
            .await?,
            "天工已通过普通方式启动，请正常退出天工后再从 Hub 增强启动；当前会话和草稿已保留"
        );
    }
    Ok(())
}

async fn gateway_ready(
    http: &Client,
    authorization: &reqwest::header::HeaderValue,
) -> Result<bool> {
    let response = http
        .get("http://127.0.0.1:18768/data/claw/session")
        .header(reqwest::header::AUTHORIZATION, authorization.clone())
        .send()
        .await?;
    if !response.status().is_success()
        || super::status_json(response).await?["hubConversationOwner"] != "ai-gateway"
    {
        return Ok(false);
    }
    // This also proves the live callback (rather than just a display marker)
    // accepts the account. Native history remains in the original DataServer.
    Ok(http
        .get("http://127.0.0.1:18768/data/scenarios")
        .header(reqwest::header::AUTHORIZATION, authorization.clone())
        .header("X-Tiangong-Account", "ai-gateway")
        .send()
        .await?
        .status()
        .is_success())
}

impl Prepared {
    pub async fn activate(self, pid: u32, config: &GmClawBridgeConfig) -> Result<()> {
        self.attach(pid, config).await
    }

    async fn attach(&self, pid: u32, config: &GmClawBridgeConfig) -> Result<()> {
        let http = local_http()?;
        // This exact child was launched with the saved token. A cached token
        // from an earlier ordinary instance must not authorize its preparation.
        let authorization = super::authorization(&config.auth_token)?;
        let socket = tokio::time::timeout(TIMEOUT, async {
            loop {
                let response = http
                    .get(format!("http://127.0.0.1:{}/json/list", self.port))
                    .send()
                    .await;
                if let Ok(response) = response
                    && response.status().is_success()
                    && let Ok(value) = super::status_json(response).await
                    && let Some(items) = value.as_array()
                    && items.len() == 1
                    && let Some(raw) = items[0]["webSocketDebuggerUrl"].as_str()
                    && let Ok(url) = Url::parse(raw)
                    && url.scheme() == "ws"
                    && url.host_str() == Some("127.0.0.1")
                    && url.port() == Some(self.port)
                    && url.username().is_empty()
                    && url.password().is_none()
                {
                    break url;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("天工本次启动通道未就绪，请正常退出后重试增强启动"))?;
        let mut inspector = Inspector {
            cdp: Cdp::connect(&socket, "天工").await?,
            sequence: 0,
            entry: self.entry.clone(),
            script: None,
        };
        let mut rollback = None;
        let mut owned = false;
        let mut result = tokio::time::timeout(TIMEOUT, async {
            let identity = inspector.evaluate("process.pid").await?;
            ensure!(
                identity.as_u64() == Some(pid as u64),
                "天工本次启动进程已变化，未接入其他程序"
            );
            owned = true;
            inspector.call("Debugger.enable", json!({})).await?;
            while inspector.script.is_none() {
                inspector.receive().await?;
            }
            let id = inspector.script.clone().unwrap();
            let original = inspector
                .call("Debugger.getScriptSource", json!({"scriptId":id}))
                .await?;
            let source = original["scriptSource"]
                .as_str()
                .context("天工主程序脚本暂不可读")?;
            let expression = format!("({ADAPTER})({})", serde_json::to_string(source)?);
            let adapted = inspector.evaluate(&expression).await?;
            ensure!(
                adapted["adapted"] == true,
                "当前天工的本地运行结构尚未适配，已保留原程序；请使用普通启动"
            );
            inspector.evaluate(CONTEXT).await?;
            rollback = Some((id.clone(), source.to_owned()));
            let changed = inspector
                .call(
                    "Debugger.setScriptSource",
                    json!({
                "scriptId":id,"scriptSource":adapted["source"]}),
                )
                .await?;
            ensure!(
                changed["status"] == "Ok",
                "天工本地运行适配未生效，已保留原程序"
            );
            let ready = loop {
                match gateway_ready(&http, &authorization).await {
                    Ok(true) => break true,
                    Ok(false) => break false,
                    Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
                }
            };
            ensure!(ready, "天工本地会话归属未就绪，请正常退出后重试增强启动");
            Ok(())
        })
        .await
        .unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "天工本地会话接入超时，请正常退出后重试增强启动"
            ))
        });
        if owned && result.is_err() {
            let restored = match rollback {
                Some((id, source)) => inspector
                    .call(
                        "Debugger.setScriptSource",
                        json!({
                    "scriptId":id,"scriptSource":source}),
                    )
                    .await
                    .is_ok_and(|v| v["status"] == "Ok"),
                None => true,
            };
            if restored {
                let _ = inspector
                    .evaluate("delete globalThis.__tiancaiHubWorkspaceMain")
                    .await;
            } else {
                // Live callbacks may still reference this helper. Preserve it
                // until the native process exits rather than breaking sends.
                result = Err(anyhow::anyhow!(
                    "天工增强适配未完成，原生回调恢复未获确认；请正常退出天工后重试"
                ));
            }
        }
        // No pause is used. After verifying this launch's PID, success and
        // failure both request inspector shutdown. Earlier connection failures
        // cannot issue that request and require normal desktop exit.
        if owned {
            let argument = serde_json::to_string(&format!("--inspect=127.0.0.1:{}", self.port))?;
            let _ = inspector.evaluate(&format!("(()=>{{for(let i=process.execArgv.length-1;i>=0;i--)if(process.execArgv[i]==={argument})process.execArgv.splice(i,1);setTimeout(()=>process.getBuiltinModule('inspector').close(),25).unref();return true;}})()")).await;
        }
        let _ = inspector.cdp.socket.close(None).await;
        result
    }
}

struct Inspector {
    cdp: Cdp,
    sequence: u32,
    entry: PathBuf,
    script: Option<String>,
}
impl Inspector {
    async fn receive(&mut self) -> Result<Value> {
        loop {
            match self.cdp.socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let value: Value = serde_json::from_str(&text)?;
                    if value["method"] == "Debugger.scriptParsed"
                        && let Some(raw) = value["params"]["url"].as_str()
                        && same_entry(raw, &self.entry)
                    {
                        self.script = value["params"]["scriptId"].as_str().map(str::to_owned);
                    }
                    return Ok(value);
                }
                Some(Ok(Message::Ping(bytes))) => {
                    self.cdp.socket.send(Message::Pong(bytes)).await?
                }
                Some(Ok(Message::Close(_))) | None => anyhow::bail!("天工主进程接入通道已关闭"),
                Some(Err(_)) => anyhow::bail!("天工主进程接入通信中断"),
                _ => {}
            }
        }
    }
    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        tokio::time::timeout(Duration::from_secs(4), self.call_inner(method, params))
            .await
            .map_err(|_| anyhow::anyhow!("天工主进程接入通信超时"))?
    }
    async fn call_inner(&mut self, method: &str, params: Value) -> Result<Value> {
        self.sequence += 1;
        let id = self.sequence;
        self.cdp
            .socket
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params}).to_string(),
            ))
            .await?;
        loop {
            let value = self.receive().await?;
            if value["id"].as_u64() == Some(id as u64) {
                ensure!(value.get("error").is_none(), "天工主进程接入协议不兼容");
                return Ok(value["result"].clone());
            }
        }
    }
    async fn evaluate(&mut self, expression: &str) -> Result<Value> {
        let value = self
            .call(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true,"silent":true}),
            )
            .await?;
        ensure!(
            value.get("exceptionDetails").is_none(),
            "天工主进程本地适配未完成"
        );
        value["result"]
            .get("value")
            .cloned()
            .context("天工主进程接入结果缺失")
    }
}

fn same_entry(raw: &str, expected: &Path) -> bool {
    let actual = Url::parse(raw)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .unwrap_or_else(|| PathBuf::from(raw));
    let actual = actual.to_string_lossy().replace('\\', "/");
    let expected = expected.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        actual.eq_ignore_ascii_case(&expected)
    } else {
        actual == expected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn main_entry_is_exact_and_uses_url_decoding() {
        let root = tempfile::tempdir().unwrap();
        let entry = root.path().join("天工 Work/app.asar/out/main/index.js");
        assert!(same_entry(
            Url::from_file_path(&entry).unwrap().as_str(),
            &entry
        ));
        assert!(!same_entry(
            Url::from_file_path(entry.with_file_name("other.js"))
                .unwrap()
                .as_str(),
            &entry
        ));
    }
}
