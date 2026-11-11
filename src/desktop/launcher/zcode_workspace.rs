//! User-triggered, memory-only startup adaptation for ZCode's external models.
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use base64::Engine;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};
use url::Url;

use crate::{
    config::AppConfig,
    desktop::{
        clients::ClientId,
        models,
        renderer_cdp::{Cdp, MAX_BYTES},
        resources::Library,
        targets::ClientTarget,
    },
};

static START_GATE: Mutex<()> = Mutex::const_new(());
static SESSION: Mutex<Option<WorkspaceSession>> = Mutex::const_new(None);
const READY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Serialize)]
struct WorkspaceOptions {
    models: Vec<ModelIdentity>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelIdentity {
    provider_id: String,
    model_id: String,
}
struct WorkspaceSession {
    task: JoinHandle<()>,
    options: watch::Sender<WorkspaceOptions>,
    socket: Url,
    executable: PathBuf,
}

fn workspace_options(path: &Path, config: &AppConfig) -> Result<Option<WorkspaceOptions>> {
    if !config.ai_gateway.enabled
        || !config.ai_gateway.providers.iter().any(|provider| {
            provider.enabled
                && provider
                    .desktop_binding
                    .as_ref()
                    .is_some_and(|binding| binding.client == ClientId::ZCode)
        })
    {
        return Ok(None);
    }
    let library = Library::new(path)?;
    let catalog = library.load()?;
    select_options(config, &library.targets(&catalog)?)
}

fn select_options(
    config: &AppConfig,
    targets: &[ClientTarget],
) -> Result<Option<WorkspaceOptions>> {
    if !config.ai_gateway.enabled {
        return Ok(None);
    }
    let mut selected = Vec::new();
    for provider in config
        .ai_gateway
        .providers
        .iter()
        .filter(|provider| provider.enabled)
    {
        let Some(binding) = provider
            .desktop_binding
            .as_ref()
            .filter(|binding| binding.client == ClientId::ZCode)
        else {
            continue;
        };
        let target = targets
            .iter()
            .find(|target| target.client == ClientId::ZCode && target.id == binding.target_id)
            .context("ZCode 模型配置位置已变化，请在接入设置重新选择")?;
        if models::using(binding, target)
            .map_err(|error| anyhow::anyhow!("无法读取已保存的 ZCode 模型配置：{error}"))?
        {
            ensure!(
                uuid::Uuid::parse_str(&binding.id).is_ok() && !binding.model.is_empty(),
                "ZCode 模型身份无效，请重新保存模型"
            );
            selected.push(ModelIdentity {
                provider_id: format!("tiancai-hub-{}", binding.id),
                model_id: binding.model.clone(),
            });
        }
    }
    Ok((!selected.is_empty()).then_some(WorkspaceOptions { models: selected }))
}

pub(super) async fn open(path: &Path, config: AppConfig) -> Result<Value> {
    let _guard = START_GATE.lock().await;
    let saved_path = path.to_owned();
    let options =
        tokio::task::spawn_blocking(move || workspace_options(&saved_path, &config)).await??;
    let Some(options) = options else {
        let path = path.to_owned();
        return tokio::task::spawn_blocking(move || super::open(&path, ClientId::ZCode)).await?;
    };
    let saved_path = path.to_owned();
    let executable =
        tokio::task::spawn_blocking(move || super::locate(&saved_path, ClientId::ZCode))
            .await??
            .context("未找到 ZCode 程序，请到接入设置选择程序位置")?;
    let mut previous = SESSION.lock().await;
    if let Some(session) = previous.as_ref()
        && !session.task.is_finished()
        && session.executable == executable
    {
        let mut cdp = Cdp::connect(&session.socket, "ZCode").await?;
        let operation = async {
            let probe = cdp.evaluate(PROBE).await?;
            ensure!(
                probe["workspaceOpen"] == true,
                "ZCode 当前未在工作区，请查看客户端提示；当前页面和草稿已保留"
            );
            if probe["installed"] == true {
                let expression = format!(
                    "window.__tiancaiZCodeWorkspace.updateModels({})",
                    serde_json::to_string(&options)?
                );
                ensure!(
                    cdp.evaluate(&expression).await? == true,
                    "ZCode 尚未加载当前保存的模型，请正常退出后重新增强启动"
                );
            }
            session.options.send_replace(options);
            let _ = cdp.call("Page.bringToFront", json!({})).await;
            Ok(opened(probe["installed"] == true))
        }
        .await;
        let _ = cdp.socket.close(None).await;
        return operation;
    }
    if let Some(session) = previous.take() {
        session.task.abort();
    }
    let program = executable.clone();
    ensure!(
        !tokio::task::spawn_blocking(move || running(&program)).await??,
        "ZCode 已在运行，当前任务和草稿已保留；首次增强启动请正常退出 ZCode，再从 Hub 启动"
    );
    let port = {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.local_addr()?.port()
    };
    let program = executable.clone();
    tokio::task::spawn_blocking(move || start(&program, port)).await??;
    let socket = tokio::time::timeout(Duration::from_secs(15), find_target(&executable, port))
        .await
        .context("ZCode 启动连接尚未就绪，请查看客户端后重试")??;
    let mut cdp = Cdp::connect(&socket, "ZCode").await?;
    let probe = tokio::time::timeout(Duration::from_secs(15), startup_probe(&mut cdp))
        .await
        .context("ZCode 启动页面尚未就绪，请查看客户端后重试")??;
    let entry = probe["module"]
        .as_str()
        .context("当前 ZCode 启动脚本暂不兼容，已保留普通启动")?;
    ensure!(
        same_renderer_module(&probe["page"], entry),
        "ZCode 启动脚本不属于当前页面"
    );
    let entry = entry.to_owned();
    // A restored workspace may already contain drafts or running tasks. Attach
    // for a later native refresh without reloading or changing its contents.
    let reload = probe["workspaceOpen"] != true;
    if reload {
        ensure!(
            safe_to_reload(&probe),
            "ZCode 当前页面存在输入或登录操作，请正常退出后重新增强启动；草稿已保留"
        );
    }
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (options_tx, options_rx) = watch::channel(options);
    let task = tokio::spawn(async move {
        let outcome = tokio::time::timeout(READY_TIMEOUT, async {
            cdp.call("Network.enable", json!({})).await?;
            cdp.call("Network.setCacheDisabled", json!({"cacheDisabled":true})).await?;
            cdp.call("Fetch.enable", json!({"patterns":[{"urlPattern":entry,"requestStage":"Response","resourceType":"Script"}]})).await?;
            if reload {
                ensure!(safe_to_reload(&cdp.evaluate(PROBE).await?), "ZCode 页面已变化，当前任务和草稿已保留；请正常退出后重新增强启动");
                cdp.call("Page.reload", json!({"ignoreCache":true})).await?;
                let options = options_rx.borrow().clone();
                wait_ready(&mut cdp, &options).await?;
            }
            Ok(())
        }).await.unwrap_or_else(|_| Err(anyhow::anyhow!("ZCode 增强工作区准备超时，请查看客户端提示；官方登录状态及配置保持原样")));
        if outcome.is_err() {
            cleanup(&mut cdp).await;
            let _ = ready_tx.send(outcome);
            return;
        }
        let _ = ready_tx.send(outcome);
        while let Ok(event) = cdp.event().await {
            if event["method"] == "Fetch.requestPaused" {
                let options = options_rx.borrow().clone();
                if adapt_response(&mut cdp, &event["params"], &options)
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
        cleanup(&mut cdp).await;
    });
    *previous = Some(WorkspaceSession {
        task,
        options: options_tx,
        socket,
        executable,
    });
    drop(previous);
    ready_rx
        .await
        .context("ZCode 增强启动连接已结束，请重试")??;
    Ok(opened(reload))
}

fn opened(enhanced: bool) -> Value {
    json!({"opened":true,"enhanced":enhanced,"message":if enhanced {
        "ZCode 已使用保存的外部模型进入工作区"
    } else {
        "ZCode 已进入工作区，当前任务和草稿保留；增强设置将在下次原生刷新时生效"
    }})
}

async fn cleanup(cdp: &mut Cdp) {
    let _ = cdp.call("Fetch.disable", json!({})).await;
    let _ = cdp
        .call("Network.setCacheDisabled", json!({"cacheDisabled":false}))
        .await;
    let _ = cdp.socket.close(None).await;
}

fn safe_to_reload(probe: &Value) -> bool {
    probe["workspaceOpen"] == false
        && probe["loginOpen"] == true
        && probe["hasInput"] == false
        && probe["oauthPending"] == false
}

async fn startup_probe(cdp: &mut Cdp) -> Result<Value> {
    loop {
        let probe = cdp.evaluate(PROBE).await?;
        if probe["module"].is_string()
            && (probe["workspaceOpen"] == true || probe["loginOpen"] == true)
        {
            return Ok(probe);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn wait_ready(cdp: &mut Cdp, options: &WorkspaceOptions) -> Result<()> {
    let mut adapted = false;
    let mut timer = tokio::time::interval(Duration::from_millis(200));
    loop {
        tokio::select! {
            event = cdp.event() => {
                let event = event?;
                if event["method"] == "Fetch.requestPaused" {
                    adapt_response(cdp, &event["params"], options).await?;
                    adapted = true;
                }
            }
            _ = timer.tick(), if adapted => {
                let probe = cdp.evaluate(PROBE).await?;
                if probe["installed"] == true && probe["available"] == true && probe["workspaceOpen"] == true && probe["loginOpen"] == false { return Ok(()); }
            }
        }
    }
}

async fn adapt_response(cdp: &mut Cdp, params: &Value, options: &WorkspaceOptions) -> Result<()> {
    let id = params["requestId"]
        .as_str()
        .context("ZCode 增强启动响应缺少标识")?;
    let operation = async {
        ensure!(
            params["responseStatusCode"] == 200,
            "ZCode 启动脚本读取失败"
        );
        let body = cdp
            .call("Fetch.getResponseBody", json!({"requestId":id}))
            .await?;
        let text = body["body"].as_str().context("ZCode 启动脚本内容缺失")?;
        let source = if body["base64Encoded"] == true {
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(text)?)?
        } else {
            text.to_owned()
        };
        ensure!(source.len() <= MAX_BYTES / 2, "ZCode 启动脚本超过支持大小");
        let expression = format!(
            "({})({},{})",
            include_str!("zcode-workspace-adapter.js"),
            serde_json::to_string(&source)?,
            serde_json::to_string(options)?
        );
        let adapted = cdp.evaluate(&expression).await?;
        ensure!(
            adapted["adapted"] == true,
            "当前 ZCode 版本的启动结构暂不兼容，已保留原启动"
        );
        let source = adapted["source"]
            .as_str()
            .context("ZCode 启动适配结果缺失")?;
        ensure!(source.len() <= MAX_BYTES / 2, "ZCode 启动脚本超过支持大小");
        let headers: Vec<_> = params["responseHeaders"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|header| {
                !matches!(
                    header["name"]
                        .as_str()
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .as_str(),
                    "content-length" | "content-encoding" | "etag"
                )
            })
            .cloned()
            .collect();
        cdp.call(
            "Fetch.fulfillRequest",
            json!({"requestId":id,"responseCode":200,"responseHeaders":headers,
            "body":base64::engine::general_purpose::STANDARD.encode(source.as_bytes())}),
        )
        .await?;
        Ok(())
    }
    .await;
    if operation.is_err() {
        let _ = cdp
            .call("Fetch.continueRequest", json!({"requestId":id}))
            .await;
    }
    operation
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
    web_socket_debugger_url: Option<String>,
}

async fn find_target(executable: &Path, port: u16) -> Result<Url> {
    let expected = executable
        .parent()
        .context("ZCode 安装目录无效")?
        .join("resources/app.asar/out/renderer/index.html");
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_millis(300))
        .timeout(Duration::from_secs(1))
        .build()?;
    loop {
        let result = async {
            let response = client
                .get(format!("http://127.0.0.1:{port}/json/list"))
                .send()
                .await?;
            ensure!(response.status().is_success(), "ZCode 启动通道尚未就绪");
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                ensure!(
                    bytes.len() + chunk.len() <= 128 * 1024,
                    "ZCode 启动窗口数据过大"
                );
                bytes.extend_from_slice(&chunk);
            }
            select_target(serde_json::from_slice(&bytes)?, &expected, port)
        }
        .await;
        if let Ok(socket) = result {
            return Ok(socket);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn select_target(targets: Vec<Target>, expected: &Path, port: u16) -> Result<Url> {
    let mut pages = targets
        .into_iter()
        .filter(|target| target.kind == "page" && same_page(&target.url, expected));
    let page = pages.next().context("ZCode 主窗口尚未出现")?;
    ensure!(
        pages.next().is_none(),
        "ZCode 主窗口不唯一，请仅保留一个工作窗口后重试"
    );
    let socket = Url::parse(page.web_socket_debugger_url.as_deref().unwrap_or(""))?;
    ensure!(
        socket.scheme() == "ws"
            && socket.host_str() == Some("127.0.0.1")
            && socket.port() == Some(port)
            && socket.username().is_empty()
            && socket.password().is_none()
            && socket.query().is_none()
            && socket.fragment().is_none()
            && !page.id.is_empty()
            && socket.path() == format!("/devtools/page/{}", page.id),
        "ZCode 启动通道不属于本机窗口"
    );
    Ok(socket)
}

fn same_page(actual: &str, expected: &Path) -> bool {
    let Ok(mut actual) = Url::parse(actual) else {
        return false;
    };
    if actual.scheme() != "file"
        || actual.host_str().is_some()
        || actual.fragment().is_some()
        || actual.query_pairs().any(|(key, _)| key == "windowKind")
    {
        return false;
    }
    // Native boot fields (locale, workspace and launch telemetry) are query
    // parameters. Compare the installation's complete file path separately.
    actual.set_query(None);
    let Ok(actual) = actual.to_file_path() else {
        return false;
    };
    #[cfg(windows)]
    {
        actual
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .eq(expected
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_lowercase()))
    }
    #[cfg(not(windows))]
    {
        actual == expected
    }
}

fn same_renderer_module(page: &Value, entry: &str) -> bool {
    let (Some(page), Ok(entry)) = (page.as_str(), Url::parse(entry)) else {
        return false;
    };
    let Ok(page) = Url::parse(page) else {
        return false;
    };
    let Some(directory) = page.path().strip_suffix("index.html") else {
        return false;
    };
    page.scheme() == "file"
        && entry.scheme() == "file"
        && page.host_str() == entry.host_str()
        && entry.query().is_none()
        && entry.fragment().is_none()
        && entry
            .path()
            .strip_prefix(&format!("{directory}assets/styles-"))
            .is_some_and(|name| !name.contains('/') && name.ends_with(".js"))
}

const PROBE: &str = r#"(() => {
 const state = window.__tiancaiZCodeWorkspace;
 // Native modulepreloads can exceed the resource timing buffer. The page's
 // declarations remain available even when timing entries are absent.
 const isModule = url => /\/assets\/styles-[^/]+\.js$/.test(url);
 const module = [...document.querySelectorAll('link[rel="modulepreload"][href],script[src]')]
  .map(e => e.href || e.src).find(isModule)
  ?? performance.getEntriesByType('resource').map(e => e.name).find(isModule);
 return {
  installed: state?.version === 1,
  available: state?.available === true,
  workspaceOpen: !!document.querySelector('[data-testid="sidebar-primary-navigation"],[data-testid="sidebar-task-list-region"],[data-testid="chat-model-select-trigger"],[data-testid="composer-provider-trigger"]'),
  loginOpen: !!document.querySelector('[data-testid="login-use-api-key-button"]'),
  oauthPending: !!document.querySelector('[data-testid="oauth-cancel"]'),
  hasInput: [...document.querySelectorAll('textarea,input:not([type="hidden"]),[contenteditable="true"]')].some(e => !!(e.value ?? e.textContent)?.trim()),
  module: module ?? null,
  page: location.href
 };
})()"#;

#[cfg(windows)]
fn running(executable: &Path) -> Result<bool> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
    };
    let expected = executable.canonicalize()?;
    let name = executable
        .file_name()
        .context("ZCode 程序位置无效")?
        .to_string_lossy();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        ensure!(
            snapshot != INVALID_HANDLE_VALUE,
            "ZCode 运行状态暂不可读取，请稍后重试"
        );
        let result = (|| {
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = Process32FirstW(snapshot, &mut entry) != 0;
            while found {
                let length = entry
                    .szExeFile
                    .iter()
                    .position(|ch| *ch == 0)
                    .unwrap_or(entry.szExeFile.len());
                if String::from_utf16_lossy(&entry.szExeFile[..length]).eq_ignore_ascii_case(&name)
                {
                    let process =
                        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                    if process.is_null() {
                        return Ok(true);
                    }
                    let mut image = vec![0; 32768];
                    let mut size = image.len() as u32;
                    let read =
                        QueryFullProcessImageNameW(process, 0, image.as_mut_ptr(), &mut size);
                    CloseHandle(process);
                    if read == 0 {
                        return Ok(true);
                    }
                    let path = PathBuf::from(String::from_utf16_lossy(&image[..size as usize]));
                    if path.canonicalize().is_ok_and(|path| path == expected) {
                        return Ok(true);
                    }
                }
                found = Process32NextW(snapshot, &mut entry) != 0;
            }
            Ok(false)
        })();
        CloseHandle(snapshot);
        result
    }
}

#[cfg(windows)]
fn start(executable: &Path, port: u16) -> Result<()> {
    use std::{ffi::OsString, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION,
            STARTUPINFOW,
        },
    };
    let application: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut command: Vec<u16> = format!(
        "\"{}\" --remote-debugging-address=127.0.0.1 --remote-debugging-port={port}",
        executable.display()
    )
    .encode_utf16()
    .chain(Some(0))
    .collect();
    let cwd: Vec<u16> = executable
        .parent()
        .context("ZCode 安装目录无效")?
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut variables: Vec<_> = std::env::vars_os()
        .filter(|(key, _)| {
            !key.to_string_lossy()
                .eq_ignore_ascii_case("ELECTRON_RUN_AS_NODE")
        })
        .collect();
    variables.sort_by_key(|(key, _)| key.to_string_lossy().to_lowercase());
    let mut environment = Vec::new();
    for (key, value) in variables {
        let mut entry = OsString::from(key);
        entry.push("=");
        entry.push(value);
        environment.extend(entry.encode_wide());
        environment.push(0);
    }
    environment.push(0);
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            cwd.as_ptr(),
            &startup,
            &mut process,
        )
    };
    ensure!(
        created != 0,
        "无法打开 ZCode：{}",
        std::io::Error::last_os_error()
    );
    unsafe {
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    Ok(())
}

#[cfg(not(windows))]
fn running(_executable: &Path) -> Result<bool> {
    anyhow::bail!("ZCode 增强启动目前仅支持 Windows，其他平台请使用普通启动")
}
#[cfg(not(windows))]
fn start(_executable: &Path, _port: u16) -> Result<()> {
    anyhow::bail!("ZCode 增强启动目前仅支持 Windows")
}

#[cfg(test)]
mod tests;
