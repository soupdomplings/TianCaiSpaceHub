//! Bounded, capability-checked refresh of messages in the official local renderer.
//! No model request, navigation, reload, draft edit, or installed file edit.
use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(target_os = "macos")]
use std::process::Stdio;

use anyhow::{Result, ensure};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
#[cfg(target_os = "macos")]
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use url::Url;

pub(super) const DISPLAY_PORT: u16 = 18769;
const TIMEOUT: Duration = Duration::from_secs(12);
const MAX_HTTP_BYTES: usize = 128 * 1024;
const MAX_SOCKET_BYTES: usize = 512 * 1024;
const GROUP: &str = "tiancaispacehub-gmclaw-display";
const ENABLE_HINT: &str =
    "天工桌面消息同步尚未启用，请正常退出天工后使用 Hub 的启动天工 Claw 按钮打开一次";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DisplayRefresh {
    Updated,
    Deferred(&'static str),
    Limited(&'static str),
    /// No cached chat panel: the next native opening reads the saved messages.
    Closed,
}

#[derive(Clone, Copy)]
enum SetupView {
    App,
    Panel,
}

#[derive(Clone, Copy, Debug)]
enum ViewIssue {
    AppScope,
    AppRefs,
    AppSchema,
    AppSets,
    PanelScope,
    PanelRefs,
    PanelSchema,
    InstanceIdentity,
    PanelIdentity,
    ApiUnavailable,
    DataEndpoint,
    Authorization,
    ScenarioShape,
    PageShape,
    SummaryShape,
    TaskIdentity,
}

impl ViewIssue {
    fn detail(self) -> &'static str {
        match self {
            Self::AppScope => "主视图引用作用域未通过核对，页面未修改",
            Self::AppRefs => "主视图任务列表引用不完整，页面未修改",
            Self::AppSchema => "主视图任务列表字段类型不兼容，页面未修改",
            Self::AppSets => "主视图打开或删除任务的状态类型不兼容，页面未修改",
            Self::PanelScope => "会话消息引用作用域未通过核对，页面未修改",
            Self::PanelRefs => "会话消息或执行状态引用不完整，页面未修改",
            Self::PanelSchema => "会话消息或执行状态字段类型不兼容，页面未修改",
            Self::InstanceIdentity => "桌面缓存的任务与会话对应关系未通过核对，页面未修改",
            Self::PanelIdentity => "当前会话窗口身份已变化，页面未修改",
            Self::ApiUnavailable => "桌面消息读取接口不可用，页面未修改",
            Self::DataEndpoint => "桌面消息读取地址未通过本机核对，页面未修改",
            Self::Authorization => "桌面消息读取授权尚未就绪，页面未修改",
            Self::ScenarioShape => "桌面项目或任务列表结构不兼容，页面未修改",
            Self::PageShape => "桌面完整消息或分页结构不兼容，页面未修改",
            Self::SummaryShape => "桌面消息摘要结构不兼容，页面未修改",
            Self::TaskIdentity => "已保存任务与会话身份不一致，页面未修改",
        }
    }
}

impl std::fmt::Display for ViewIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.detail())
    }
}

impl std::error::Error for ViewIssue {}

pub(super) fn view_issue_detail(error: &anyhow::Error) -> Option<&'static str> {
    error
        .downcast_ref::<ViewIssue>()
        .map(|issue| issue.detail())
}

fn view_issue(code: &str) -> Option<ViewIssue> {
    match code {
        "app_fields" => Some(ViewIssue::AppSchema),
        "app_sets" => Some(ViewIssue::AppSets),
        "panel_fields" => Some(ViewIssue::PanelSchema),
        "instance_identity" => Some(ViewIssue::InstanceIdentity),
        "panel_identity" => Some(ViewIssue::PanelIdentity),
        "api_fields" => Some(ViewIssue::ApiUnavailable),
        "data_endpoint" => Some(ViewIssue::DataEndpoint),
        "authorization" => Some(ViewIssue::Authorization),
        "scenario_shape" => Some(ViewIssue::ScenarioShape),
        "page_shape" => Some(ViewIssue::PageShape),
        "summary_shape" => Some(ViewIssue::SummaryShape),
        "task_identity" => Some(ViewIssue::TaskIdentity),
        _ => None,
    }
}

impl SetupView {
    fn schema_name(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Panel => "panel",
        }
    }

    fn scope_error(self) -> ViewIssue {
        match self {
            Self::App => ViewIssue::AppScope,
            Self::Panel => ViewIssue::PanelScope,
        }
    }

    fn field_error(self) -> ViewIssue {
        match self {
            Self::App => ViewIssue::AppRefs,
            Self::Panel => ViewIssue::PanelRefs,
        }
    }

    fn allows_scope(self, description: &str) -> bool {
        // Setup values may move between Closure and Block when the desktop's
        // compiler or function parameters change. The full schema identifies
        // the correct scope; unrelated module/global/script scopes stay out.
        ["Closure", "Block"].iter().any(|kind| {
            description == *kind
                || description
                    .strip_prefix(*kind)
                    .is_some_and(|suffix| suffix.starts_with(" (") && suffix.ends_with(')'))
        })
    }
}

pub(super) async fn available(executable: &Path, desktop_pids: &[u32]) -> Result<()> {
    tokio::time::timeout(TIMEOUT, available_inner(executable, desktop_pids))
        .await
        .map_err(|_| anyhow::anyhow!("天工桌面消息同步检查超时"))??;
    Ok(())
}

async fn available_inner(executable: &Path, desktop_pids: &[u32]) -> Result<()> {
    let socket = target(executable, desktop_pids).await?;
    let mut cdp = connect_cdp(&socket).await?;
    let operation = async {
        let (target, arguments) = renderer_arguments(&mut cdp, None, None, &[]).await?;
        let result = cdp
            .call(
                "Runtime.callFunctionOn",
                json!({
                    "functionDeclaration": include_str!("display-schema.js"),
                    "arguments": arguments, "objectGroup": GROUP, "silent": true,
                    "returnByValue": true, "generatePreview": false, "objectId": target,
                }),
            )
            .await?;
        ensure!(
            result.get("exceptionDetails").is_none(),
            "天工桌面视图能力检查未完成"
        );
        match result
            .get("result")
            .and_then(|value| value.get("value"))
            .and_then(Value::as_str)
        {
            Some("ready") => {}
            Some("app_fields") => anyhow::bail!(ViewIssue::AppSchema),
            Some("panel_fields") => anyhow::bail!(ViewIssue::PanelSchema),
            Some("instance_identity") => anyhow::bail!(ViewIssue::InstanceIdentity),
            _ => anyhow::bail!("天工桌面视图能力检查结果无法核验"),
        }
        Ok(())
    }
    .await;
    let _ = cdp
        .call("Runtime.releaseObjectGroup", json!({"objectGroup": GROUP}))
        .await;
    let _ = cdp.socket.close(None).await;
    operation
}

pub(super) async fn refresh(
    executable: &Path,
    desktop_pids: &[u32],
    task_id: &str,
    session_id: &str,
    hub_row_ids: &[i64],
) -> Result<DisplayRefresh> {
    ensure!(
        valid_id(task_id) && valid_id(session_id),
        "天工桌面消息同步身份无效"
    );
    ensure!(
        hub_row_ids.len() <= 128
            && hub_row_ids
                .iter()
                .all(|id| *id > 0 && *id <= 9_007_199_254_740_991),
        "天工桌面消息同步行身份无效"
    );
    tokio::time::timeout(
        TIMEOUT,
        refresh_inner(executable, desktop_pids, task_id, session_id, hub_row_ids),
    )
    .await
    .map_err(|_| anyhow::anyhow!("天工桌面消息同步超时，已保存内容不会重放"))?
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

async fn refresh_inner(
    executable: &Path,
    desktop_pids: &[u32],
    task_id: &str,
    session_id: &str,
    hub_row_ids: &[i64],
) -> Result<DisplayRefresh> {
    let socket = target(executable, desktop_pids).await?;
    let mut cdp = connect_cdp(&socket).await?;
    let result = refresh_renderer(&mut cdp, task_id, session_id, hub_row_ids).await;
    // Release remote handles even when refresh was declined. Never retain UI data.
    let _ = cdp
        .call("Runtime.releaseObjectGroup", json!({"objectGroup": GROUP}))
        .await;
    let _ = cdp.socket.close(None).await;
    result
}

async fn connect_cdp(socket: &Url) -> Result<Cdp> {
    let config = WebSocketConfig {
        max_message_size: Some(MAX_SOCKET_BYTES),
        max_frame_size: Some(MAX_SOCKET_BYTES),
        ..WebSocketConfig::default()
    };
    let (socket, _) = connect_async_with_config(socket.as_str(), Some(config), false)
        .await
        .map_err(|_| anyhow::anyhow!(ENABLE_HINT))?;
    Ok(Cdp { socket, next_id: 0 })
}

struct Cdp {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u32,
}

impl Cdp {
    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let request = json!({"id": id, "method": method, "params": params}).to_string();
        ensure!(
            request.len() <= MAX_SOCKET_BYTES,
            "天工桌面同步请求超过限制"
        );
        self.socket
            .send(Message::Text(request))
            .await
            .map_err(|_| anyhow::anyhow!("天工桌面同步连接中断"))?;
        for _ in 0..64 {
            let message = self
                .socket
                .next()
                .await
                .ok_or_else(|| anyhow::anyhow!("天工桌面同步连接已关闭"))?
                .map_err(|_| anyhow::anyhow!("天工桌面同步响应无法读取"))?;
            let Message::Text(text) = message else {
                continue;
            };
            ensure!(text.len() <= MAX_SOCKET_BYTES, "天工桌面同步响应超过限制");
            let value: Value = serde_json::from_str(&text)
                .map_err(|_| anyhow::anyhow!("天工桌面同步响应无法核验"))?;
            if value.get("id").and_then(Value::as_u64) != Some(id as u64) {
                continue;
            }
            ensure!(value.get("error").is_none(), "天工桌面同步协议不兼容");
            return value
                .get("result")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("天工桌面同步响应缺失"));
        }
        anyhow::bail!("天工桌面同步事件超过限制")
    }

    async fn properties(&mut self, object: &str) -> Result<Value> {
        self.call(
            "Runtime.getProperties",
            json!({
                "objectId": object, "ownProperties": true, "generatePreview": false,
            }),
        )
        .await
    }

    /// Only bounded Closure/Block scopes of this exact view are examined,
    /// never module/global/script scopes. CDP
    /// enumerates all setup properties, so primitive values (including the
    /// native renderer's authorization) briefly enter local memory. They are
    /// never logged, persisted, or returned; only required object handles and
    /// the nullable cleanup handle are retained as call arguments.
    async fn setup_refs(
        &mut self,
        function: &str,
        required: &[&str],
        view: SetupView,
    ) -> Result<Vec<Value>> {
        let properties = self.properties(function).await?;
        let scope_list = properties
            .get("internalProperties")
            .and_then(Value::as_array)
            .and_then(|values| {
                values
                    .iter()
                    .find(|v| v.get("name").and_then(Value::as_str) == Some("[[Scopes]]"))
            })
            .and_then(|value| value.get("value"))
            .and_then(object_id)
            .ok_or_else(|| anyhow::anyhow!(view.scope_error()))?;
        let scopes = self.properties(&scope_list).await?;
        let candidates: Vec<String> = scopes
            .get("result")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|property| {
                let value = property.get("value")?;
                let description = value.get("description")?.as_str()?;
                view.allows_scope(description)
                    .then(|| object_id(value))
                    .flatten()
            })
            .collect();
        if candidates.is_empty() || candidates.len() > 8 {
            anyhow::bail!(view.scope_error());
        }
        let mut matched = None;
        for scope in candidates {
            let values = self.properties(&scope).await?;
            let Some(values) = values.get("result").and_then(Value::as_array) else {
                continue;
            };
            let arguments: Option<Vec<Value>> = required
                .iter()
                .map(|name| {
                    values
                        .iter()
                        .find(|value| value.get("name").and_then(Value::as_str) == Some(*name))
                        .and_then(|property| property.get("value"))
                        .and_then(|value| {
                            if let Some(id) = object_id(value) {
                                Some(json!({"objectId": id}))
                            } else if *name == "activeStreamCleanup"
                                && value.get("subtype").and_then(Value::as_str) == Some("null")
                            {
                                Some(json!({"value": null}))
                            } else {
                                None
                            }
                        })
                })
                .collect();
            if let Some(arguments) = arguments {
                let mut schema_arguments = vec![json!({"value": view.schema_name()})];
                schema_arguments.extend(arguments.iter().cloned());
                let schema = self
                    .call(
                        "Runtime.callFunctionOn",
                        json!({
                            "functionDeclaration": include_str!("display-setup-schema.js"),
                            "arguments": schema_arguments, "objectGroup": GROUP,
                            "silent": true, "returnByValue": true,
                            "generatePreview": false, "objectId": function,
                        }),
                    )
                    .await?;
                let compatible = schema.get("exceptionDetails").is_none()
                    && schema
                        .get("result")
                        .and_then(|value| value.get("value"))
                        .and_then(Value::as_bool)
                        == Some(true);
                if compatible {
                    if matched.is_some() {
                        anyhow::bail!(view.scope_error());
                    }
                    matched = Some(arguments);
                }
            }
        }
        matched.ok_or_else(|| anyhow::anyhow!(view.field_error()))
    }
}

fn object_id(value: &Value) -> Option<String> {
    value
        .get("objectId")
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 512)
        .map(str::to_owned)
}

async fn renderer_arguments(
    cdp: &mut Cdp,
    task_id: Option<&str>,
    session_id: Option<&str>,
    hub_row_ids: &[i64],
) -> Result<(String, Vec<Value>)> {
    let expression = format!(
        "({})({}, {})",
        include_str!("display-target.js"),
        serde_json::to_string(&task_id)?,
        serde_json::to_string(&session_id)?,
    );
    let evaluated = cdp
        .call(
            "Runtime.evaluate",
            json!({
                "expression": expression, "objectGroup": GROUP, "silent": true,
                "generatePreview": false, "returnByValue": false,
            }),
        )
        .await?;
    ensure!(
        evaluated.get("exceptionDetails").is_none(),
        "天工桌面视图身份无法核验"
    );
    let target = evaluated
        .get("result")
        .and_then(object_id)
        .ok_or_else(|| anyhow::anyhow!("天工桌面视图尚未准备"))?;
    let properties = cdp.properties(&target).await?;
    let properties = properties
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("天工桌面视图结构无法核验"))?;
    let handle = |name: &str| {
        properties
            .iter()
            .find(|value| value.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|value| value.get("value"))
            .and_then(object_id)
    };
    let app = handle("appRender").ok_or_else(|| anyhow::anyhow!("天工桌面视图尚未准备"))?;
    let app_refs = cdp
        .setup_refs(
            &app,
            &[
                "chatInstances",
                "scenarios",
                "openingTasks",
                "deletingTaskIds",
            ],
            SetupView::App,
        )
        .await?;
    let panel = handle("panelRender");
    let props = handle("panelProps");
    ensure!(panel.is_some() == props.is_some(), "天工桌面会话视图不完整");
    let panel_refs = if let Some(panel) = panel {
        Some(
            cdp.setup_refs(
                &panel,
                &[
                    "messages",
                    "isStreaming",
                    "pendingConfirm",
                    "showWelcome",
                    "hasOlderMessages",
                    "forceScrollToBottom",
                    "loadingOlderMessages",
                    "dbModelsLoaded",
                    "localTaskId",
                    "sessionId",
                    "activeStreamCleanup",
                ],
                SetupView::Panel,
            )
            .await?,
        )
    } else {
        None
    };
    let mut arguments: Vec<Value> = (0..11)
        .map(|index| {
            panel_refs
                .as_ref()
                .map_or(json!({"value": null}), |refs| refs[index].clone())
        })
        .collect();
    arguments.extend(app_refs);
    arguments.push(props.map_or(json!({"value": null}), |id| json!({"objectId": id})));
    arguments.push(json!({"value": task_id}));
    arguments.push(json!({"value": session_id}));
    arguments.push(json!({"value": hub_row_ids}));
    Ok((target, arguments))
}

async fn refresh_renderer(
    cdp: &mut Cdp,
    task_id: &str,
    session_id: &str,
    hub_row_ids: &[i64],
) -> Result<DisplayRefresh> {
    let (target, arguments) =
        renderer_arguments(cdp, Some(task_id), Some(session_id), hub_row_ids).await?;
    let result = cdp
        .call(
            "Runtime.callFunctionOn",
            json!({
                "functionDeclaration": include_str!("display-sync.js"),
                "arguments": arguments, "objectGroup": GROUP, "silent": true,
                "awaitPromise": true, "returnByValue": true, "generatePreview": false,
                "objectId": target,
            }),
        )
        .await?;
    ensure!(
        result.get("exceptionDetails").is_none(),
        "天工桌面消息同步未完成，已保存内容保留"
    );
    match result
        .get("result")
        .and_then(|v| v.get("value"))
        .and_then(Value::as_str)
    {
        Some("updated") => Ok(DisplayRefresh::Updated),
        Some("busy") => Ok(DisplayRefresh::Deferred(
            "桌面正在执行、审批或加载历史，空闲后自动更新",
        )),
        Some("changed") => Ok(DisplayRefresh::Deferred(
            "读取期间桌面会话已变化，稍后自动更新",
        )),
        Some("unpersisted") => Ok(DisplayRefresh::Deferred(
            "桌面存在尚未保存的消息，已保留窗口内容",
        )),
        Some("ambiguous") => Ok(DisplayRefresh::Deferred(
            "消息来源尚不能唯一核对，已保留窗口内容",
        )),
        Some("fetch_failed") => Ok(DisplayRefresh::Deferred(
            "桌面消息读取未完成，稍后自动重试显示",
        )),
        Some("limited") => Ok(DisplayRefresh::Limited(
            "历史达到显示读取上限，内容已保存；重新打开任务可读取",
        )),
        Some("closed") => Ok(DisplayRefresh::Closed),
        code => {
            if let Some(issue) = code.and_then(view_issue) {
                return Err(issue.into());
            }
            anyhow::bail!("天工桌面消息同步结构不兼容，已保存内容保留")
        }
    }
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

async fn target(executable: &Path, desktop_pids: &[u32]) -> Result<Url> {
    ensure!(
        !desktop_pids.is_empty() && desktop_pids.len() <= 64,
        "天工桌面运行身份无法核验"
    );
    let path = executable.to_owned();
    let expected = tokio::task::spawn_blocking(move || installed_renderer(&path))
        .await
        .map_err(|_| anyhow::anyhow!("天工桌面资源检查未完成"))??;
    let path = executable
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("天工桌面安装无法核验"))?;
    verify_listener(&path, desktop_pids).await?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .read_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|_| anyhow::anyhow!("天工桌面同步连接无法准备"))?;
    let response = client
        .get(format!("http://127.0.0.1:{DISPLAY_PORT}/json/list"))
        .send()
        .await
        .map_err(|_| anyhow::anyhow!(ENABLE_HINT))?;
    ensure!(response.status().is_success(), "天工桌面同步目标无法核验");
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= MAX_HTTP_BYTES as u64),
        "天工桌面同步目标超过限制"
    );
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| anyhow::anyhow!("天工桌面同步目标无法读取"))?;
        ensure!(
            bytes.len() + chunk.len() <= MAX_HTTP_BYTES,
            "天工桌面同步目标超过限制"
        );
        bytes.extend_from_slice(&chunk);
    }
    let values: Vec<Target> = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("天工桌面同步目标格式无法核验"))?;
    ensure!(values.len() <= 32, "天工桌面同步目标超过限制");
    let pages: Vec<_> = values
        .into_iter()
        .filter(|target| target.kind == "page")
        .collect();
    ensure!(!pages.is_empty(), "天工桌面页面尚未出现");
    let mut matches = pages
        .into_iter()
        .filter(|target| same_file_url(&target.url, &expected));
    let target = matches
        .next()
        .ok_or_else(|| anyhow::anyhow!("天工桌面页面地址与安装路径不匹配"))?;
    ensure!(matches.next().is_none(), "天工桌面页面无法唯一核验");
    ensure!(valid_id(&target.id), "天工桌面页面标识无法核验");
    let socket = Url::parse(target.web_socket_debugger_url.as_deref().unwrap_or(""))
        .map_err(|_| anyhow::anyhow!("天工桌面同步通道无效"))?;
    ensure!(
        socket.scheme() == "ws"
            && socket.host_str() == Some("127.0.0.1")
            && socket.port() == Some(DISPLAY_PORT)
            && socket.username().is_empty()
            && socket.password().is_none()
            && socket.query().is_none()
            && socket.fragment().is_none()
            && socket.path() == format!("/devtools/page/{}", target.id),
        "天工桌面同步通道不属于本机窗口"
    );
    // Reduce the interval in which a stopped listener could be replaced.
    verify_listener(&path, desktop_pids).await?;
    Ok(socket)
}

fn same_file_url(actual: &str, expected: &Path) -> bool {
    let Ok(url) = Url::parse(actual) else {
        return false;
    };
    if url.scheme() != "file" || url.query().is_some() || url.fragment().is_some() {
        return false;
    }
    let Ok(actual) = url.to_file_path() else {
        return false;
    };
    #[cfg(windows)]
    {
        // PathBuf preserves '/' inside a joined string, while to_file_path()
        // returns Windows '\\' separators. Compare the complete component list
        // so equivalent spellings match without accepting another ASAR page.
        let mut actual = actual.components();
        let mut expected = expected.components();
        loop {
            match (actual.next(), expected.next()) {
                (None, None) => return true,
                (Some(actual), Some(expected))
                    if actual
                        .as_os_str()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&expected.as_os_str().to_string_lossy()) => {}
                _ => return false,
            }
        }
    }
    #[cfg(not(windows))]
    {
        actual == expected
    }
}

#[cfg(all(test, windows))]
mod path_tests {
    use super::same_file_url;
    use std::path::Path;

    #[test]
    fn windows_page_identity_accepts_equivalent_separators_and_case() {
        let archive = Path::new(r"D:\Program Files\tiangong-desktop\resources\app.asar");
        let mixed = archive.join("out/renderer/index.html");
        let native = archive.join("out").join("renderer").join("index.html");
        let page = "file:///d:/Program%20Files/tiangong-desktop/resources/app.asar/out/renderer/index.html";
        assert!(same_file_url(page, &mixed));
        assert!(same_file_url(page, &native));
    }

    #[test]
    fn windows_page_identity_rejects_different_pages_and_url_extras() {
        let expected = Path::new(
            r"D:\Program Files\tiangong-desktop\resources\app.asar\out\renderer\index.html",
        );
        for page in [
            "file:///C:/Program%20Files/tiangong-desktop/resources/app.asar/out/renderer/index.html",
            "file:///D:/Other/tiangong-desktop/resources/app.asar/out/renderer/index.html",
            "file:///D:/Program%20Files/tiangong-desktop/resources/app.asar/out/renderer/other.html",
            "file:///D:/Program%20Files/tiangong-desktop/resources/app.asar/out/renderer/index.html?other=1",
            "file:///D:/Program%20Files/tiangong-desktop/resources/app.asar/out/renderer/index.html#other",
            "file://remote/D:/Program%20Files/tiangong-desktop/resources/app.asar/out/renderer/index.html",
            "https://example.invalid/out/renderer/index.html",
        ] {
            assert!(!same_file_url(page, expected));
        }
    }
}

#[cfg(test)]
mod scope_tests {
    use super::{SetupView, ViewIssue, view_issue_detail};

    #[test]
    fn setup_scopes_follow_capabilities_instead_of_compiler_layout() {
        for view in [SetupView::App, SetupView::Panel] {
            for scope in ["Closure", "Closure (setup)", "Block", "Block (setup)"] {
                assert!(view.allows_scope(scope));
            }
        }
        for scope in ["Module", "Global", "Script", "BlockBody", "Closure (setup"] {
            assert!(!SetupView::App.allows_scope(scope));
            assert!(!SetupView::Panel.allows_scope(scope));
        }
    }

    #[test]
    fn diagnostic_exposes_only_a_typed_fixed_reason() {
        let issue = anyhow::Error::new(ViewIssue::PanelScope).context("outer context");
        assert_eq!(
            view_issue_detail(&issue),
            Some("会话消息引用作用域未通过核对，页面未修改")
        );
        assert!(view_issue_detail(&anyhow::anyhow!("arbitrary remote error")).is_none());
    }
}

fn installed_renderer(executable: &Path) -> Result<PathBuf> {
    ensure!(super::valid_executable(executable), "天工桌面安装无法核验");
    let executable = executable
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("天工桌面安装无法核验"))?;
    #[cfg(windows)]
    let resources = executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("天工桌面安装路径无效"))?
        .join("resources");
    #[cfg(target_os = "macos")]
    let resources = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow::anyhow!("天工桌面安装路径无效"))?
        .join("Resources");
    #[cfg(not(any(windows, target_os = "macos")))]
    let resources = {
        let _ = executable;
        anyhow::bail!("此平台不支持天工桌面消息同步")
    };
    let archive = resources.join("app.asar");
    let mut file =
        std::fs::File::open(&archive).map_err(|_| anyhow::anyhow!("天工桌面资源无法读取"))?;
    let file_size = file
        .metadata()
        .map_err(|_| anyhow::anyhow!("天工桌面资源无法核验"))?
        .len();
    let mut prefix = [0; 16];
    file.read_exact(&mut prefix)
        .map_err(|_| anyhow::anyhow!("天工桌面资源格式无法核验"))?;
    let number = |offset| {
        u32::from_le_bytes(prefix[offset..offset + 4].try_into().expect("ASAR prefix")) as usize
    };
    let header_size = number(4);
    let json_size = number(12);
    ensure!(
        number(0) == 4
            && json_size <= 8 * 1024 * 1024
            && json_size + 8 <= header_size
            && 8 + header_size as u64 <= file_size,
        "天工桌面资源头无法核验"
    );
    let mut header = vec![0; json_size];
    file.read_exact(&mut header)
        .map_err(|_| anyhow::anyhow!("天工桌面资源头无法读取"))?;
    let header: Value =
        serde_json::from_slice(&header).map_err(|_| anyhow::anyhow!("天工桌面资源索引无法核验"))?;
    let read_entry =
        |file: &mut std::fs::File, names: &[&str], maximum: usize| -> Result<Vec<u8>> {
            let mut value = &header;
            for name in names {
                value = value
                    .get("files")
                    .and_then(|v| v.get(*name))
                    .ok_or_else(|| anyhow::anyhow!("天工桌面视图入口资源缺失"))?;
            }
            ensure!(
                value.get("unpacked").is_none() && value.get("link").is_none(),
                "天工桌面视图资源无法核验"
            );
            let size = value
                .get("size")
                .and_then(Value::as_u64)
                .filter(|n| *n > 0 && *n <= maximum as u64)
                .ok_or_else(|| anyhow::anyhow!("天工桌面视图资源大小无效"))?;
            let offset = value
                .get("offset")
                .and_then(Value::as_str)
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| anyhow::anyhow!("天工桌面视图资源位置无效"))?;
            let start = (8 + header_size as u64)
                .checked_add(offset)
                .ok_or_else(|| anyhow::anyhow!("天工桌面视图资源位置无效"))?;
            ensure!(
                start.checked_add(size).is_some_and(|end| end <= file_size),
                "天工桌面视图资源范围无效"
            );
            file.seek(SeekFrom::Start(start))
                .map_err(|_| anyhow::anyhow!("天工桌面视图资源无法读取"))?;
            let mut bytes = vec![0; size as usize];
            file.read_exact(&mut bytes)
                .map_err(|_| anyhow::anyhow!("天工桌面视图资源无法读取"))?;
            Ok(bytes)
        };
    let package = read_entry(&mut file, &["package.json"], 64 * 1024)?;
    let package: Value = serde_json::from_slice(&package)
        .map_err(|_| anyhow::anyhow!("天工桌面应用身份无法核验"))?;
    ensure!(is_tiangong_package(&package), "桌面应用身份不属于天工 Claw");
    let html = read_entry(&mut file, &["out", "renderer", "index.html"], 256 * 1024)?;
    let html =
        std::str::from_utf8(&html).map_err(|_| anyhow::anyhow!("天工桌面视图入口格式无法核验"))?;
    // Locate the script through the installed HTML. Build hashes and package
    // versions are irrelevant; actual Vue/API capabilities are checked later.
    let entries = renderer_script_entries(html)?;
    let mut total_bytes = 0_u64;
    for entry in entries {
        let mut value = &header;
        for name in ["out", "renderer"]
            .into_iter()
            .chain(entry.iter().map(String::as_str))
        {
            value = value
                .get("files")
                .and_then(|files| files.get(name))
                .ok_or_else(|| anyhow::anyhow!("天工桌面视图入口脚本缺失"))?;
        }
        ensure!(
            value.get("unpacked").is_none() && value.get("link").is_none(),
            "天工桌面视图入口脚本无法核验"
        );
        let size = value
            .get("size")
            .and_then(Value::as_u64)
            .filter(|size| *size > 0 && *size <= 32 * 1024 * 1024)
            .ok_or_else(|| anyhow::anyhow!("天工桌面视图入口脚本大小无效"))?;
        total_bytes += size;
        ensure!(
            total_bytes <= 32 * 1024 * 1024,
            "天工桌面视图入口脚本超过限制"
        );
        let start = value
            .get("offset")
            .and_then(Value::as_str)
            .and_then(|offset| offset.parse::<u64>().ok())
            .and_then(|offset| (8 + header_size as u64).checked_add(offset))
            .ok_or_else(|| anyhow::anyhow!("天工桌面视图入口脚本位置无效"))?;
        ensure!(
            start.checked_add(size).is_some_and(|end| end <= file_size),
            "天工桌面视图入口脚本范围无效"
        );
    }
    // Chromium exposes ordinary Windows file paths, without canonicalize's
    // extended-length prefix. No virtual ASAR path is canonicalized.
    #[cfg(windows)]
    let archive = {
        let path = archive.to_string_lossy();
        PathBuf::from(path.strip_prefix("\\\\?\\").unwrap_or(&path))
    };
    Ok(archive.join("out").join("renderer").join("index.html"))
}

fn is_tiangong_package(package: &Value) -> bool {
    fn recognized(value: &str) -> bool {
        if value.len() > 128 {
            return false;
        }
        let normalized: String = value
            .chars()
            .filter(|character| !matches!(character, ' ' | '-' | '_' | '.'))
            .flat_map(char::to_lowercase)
            .collect();
        matches!(
            normalized.as_str(),
            "tiangongdesktop"
                | "tiangongclaw"
                | "tiangong"
                | "gmclaw"
                | "gmclawdesktop"
                | "天工claw"
                | "天工"
                | "天工桌面"
        )
    }
    package
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(recognized)
        && match package.get("productName") {
            None => true,
            Some(Value::String(name)) => recognized(name),
            _ => false,
        }
}

/// Parse only bounded external script tags in the renderer entry document.
/// No JavaScript is executed or read here. Entries must stay below that same
/// ASAR renderer directory; Runtime inspection provides the capability check.
fn renderer_script_entries(html: &str) -> Result<Vec<Vec<String>>> {
    ensure!(
        html.len() <= 256 * 1024 && !html.contains('\0'),
        "天工桌面视图入口格式无法核验"
    );
    let mut entries = Vec::new();
    let mut remaining = html;
    let mut script_count = 0;
    while let Some(open) = remaining.find('<') {
        remaining = &remaining[open + 1..];
        if let Some(comment) = remaining.strip_prefix("!--") {
            let end = comment
                .find("-->")
                .ok_or_else(|| anyhow::anyhow!("天工桌面视图入口注释格式无法核验"))?;
            remaining = &comment[end + 3..];
            continue;
        }
        if remaining.len() < 6
            || !remaining.as_bytes()[..6].eq_ignore_ascii_case(b"script")
            || remaining
                .as_bytes()
                .get(6)
                .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'>')
        {
            continue;
        }
        script_count += 1;
        ensure!(script_count <= 32, "天工桌面视图入口脚本超过限制");
        let tag = &remaining[6..];
        let mut quote = None;
        let end = tag
            .bytes()
            .position(|byte| match quote {
                Some(current) if current == byte => {
                    quote = None;
                    false
                }
                Some(_) => false,
                None if matches!(byte, b'\'' | b'"') => {
                    quote = Some(byte);
                    false
                }
                None => byte == b'>',
            })
            .ok_or_else(|| anyhow::anyhow!("天工桌面视图入口脚本格式无法核验"))?;
        let src = script_source(&tag[..end])?;
        remaining = &tag[end + 1..];
        if let Some(src) = src {
            ensure!(src.len() <= 1024, "天工桌面视图入口脚本地址超过限制");
            let src = src.strip_prefix("./").unwrap_or(src);
            ensure!(
                !src.is_empty()
                    && !src.contains(['\\', ':', '?', '#', '%', '&'])
                    && src.chars().all(|character| !character.is_control()),
                "天工桌面视图入口脚本必须属于安装目录"
            );
            let names: Vec<String> = src.split('/').map(str::to_owned).collect();
            ensure!(
                names.len() <= 8
                    && names.iter().all(|name| !name.is_empty()
                        && name != "."
                        && name != ".."
                        && name.len() <= 255)
                    && names
                        .last()
                        .is_some_and(|name| name.ends_with(".js") || name.ends_with(".mjs")),
                "天工桌面视图入口脚本地址无法核验"
            );
            if !entries.contains(&names) {
                entries.push(names);
            }
        }
        // Script contents are raw text, including '<' characters. Do not
        // mistake a string inside an inline script for another script tag.
        let lower = remaining.to_ascii_lowercase();
        let close = lower
            .find("</script")
            .ok_or_else(|| anyhow::anyhow!("天工桌面视图入口脚本结束标记缺失"))?;
        let end = remaining[close..]
            .find('>')
            .ok_or_else(|| anyhow::anyhow!("天工桌面视图入口脚本结束标记无效"))?;
        remaining = &remaining[close + end + 1..];
    }
    ensure!(!entries.is_empty(), "天工桌面视图入口脚本缺失");
    Ok(entries)
}

fn script_source(mut attributes: &str) -> Result<Option<&str>> {
    let mut source = None;
    while !attributes.trim().is_empty() {
        attributes = attributes.trim_start();
        let end = attributes
            .find(|character: char| character.is_ascii_whitespace() || character == '=')
            .unwrap_or(attributes.len());
        ensure!(end > 0, "天工桌面视图入口脚本属性无效");
        let name = &attributes[..end];
        attributes = attributes[end..].trim_start();
        let Some(value) = attributes.strip_prefix('=') else {
            ensure!(
                !name.eq_ignore_ascii_case("src"),
                "天工桌面视图入口脚本地址缺失"
            );
            continue;
        };
        attributes = value.trim_start();
        let value;
        if attributes.starts_with(['\'', '"']) {
            let quote = attributes.as_bytes()[0] as char;
            let end = attributes[1..]
                .find(quote)
                .ok_or_else(|| anyhow::anyhow!("天工桌面视图入口脚本属性无法核验"))?
                + 1;
            value = &attributes[1..end];
            attributes = &attributes[end + 1..];
        } else {
            let end = attributes
                .find(char::is_whitespace)
                .unwrap_or(attributes.len());
            value = &attributes[..end];
            attributes = &attributes[end..];
        }
        if name.eq_ignore_ascii_case("src") {
            ensure!(source.is_none(), "天工桌面视图入口脚本地址重复");
            source = Some(value);
        }
    }
    Ok(source)
}

#[cfg(test)]
mod renderer_entry_tests {
    use super::{is_tiangong_package, renderer_script_entries};
    use serde_json::json;

    #[test]
    fn package_identity_does_not_depend_on_a_version_number() {
        for version in ["1.1.1", "1.2.0", "2.0.0-beta.1"] {
            assert!(is_tiangong_package(
                &json!({"name":"tiangong-desktop", "productName":"GMClaw", "version":version})
            ));
        }
        assert!(is_tiangong_package(&json!({"name":"tiangong-desktop"})));
        assert!(is_tiangong_package(
            &json!({"name":"gmclaw-desktop", "productName":"天工 Claw", "version":"3.0.0"})
        ));
        for package in [
            json!({"name":"other-app", "productName":"GMClaw"}),
            json!({"name":"tiangong-desktop", "productName":"another-app"}),
            json!({"name":"fake-ti angong-desktop"}),
        ] {
            assert!(!is_tiangong_package(&package));
        }
    }

    #[test]
    fn renderer_uses_the_installed_script_with_any_build_suffix() {
        for asset in ["index-oms9jgdP.js", "index-new-build.mjs", "app-v2.js"] {
            let html = format!(
                "<!-- <script src='fake.js'></script> --><script type=\"module\" crossorigin src='./assets/{asset}'></script>"
            );
            assert_eq!(
                renderer_script_entries(&html).unwrap(),
                vec![vec!["assets".to_owned(), asset.to_owned()]]
            );
        }
    }

    #[test]
    fn renderer_rejects_scripts_outside_its_installed_entry() {
        for script in [
            "../other.js",
            "/assets/index.js",
            "https://example.invalid/index.js",
            "//example.invalid/index.js",
            "assets/../index.js",
            "assets/%2e%2e/index.js",
            "assets/index.js?other=1",
            "assets/index.js#other",
            "assets/index.css",
        ] {
            assert!(renderer_script_entries(&format!("<script src='{script}'></script>")).is_err());
        }
        assert!(renderer_script_entries("<script src='one.js' src='two.js'></script>").is_err());
        assert!(
            renderer_script_entries("<script>const fake = '<script src=\"one.js\">';</script>")
                .is_err()
        );
    }
}

#[cfg(target_os = "macos")]
async fn helper_output(command: tokio::process::Command) -> Result<Vec<u8>> {
    tokio::time::timeout(Duration::from_secs(2), helper_output_inner(command))
        .await
        .map_err(|_| anyhow::anyhow!("天工桌面同步进程核验超时"))?
}

#[cfg(target_os = "macos")]
async fn helper_output_inner(mut command: tokio::process::Command) -> Result<Vec<u8>> {
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| anyhow::anyhow!("无法核验天工桌面同步进程"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("无法读取天工桌面同步进程"))?;
    let mut bytes = Vec::new();
    stdout
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| anyhow::anyhow!("天工桌面同步进程核验未完成"))?;
    ensure!(bytes.len() <= 16 * 1024, "天工桌面同步进程信息超过限制");
    ensure!(
        child
            .wait()
            .await
            .map_err(|_| anyhow::anyhow!("天工桌面同步进程核验未完成"))?
            .success(),
        "天工桌面同步进程无法核验"
    );
    Ok(bytes)
}

#[cfg(windows)]
async fn verify_listener(executable: &Path, desktop_pids: &[u32]) -> Result<()> {
    let executable = executable.to_owned();
    let desktop_pids = desktop_pids.to_owned();
    tokio::task::spawn_blocking(move || windows_listener(&executable, &desktop_pids))
        .await
        .map_err(|_| anyhow::anyhow!("天工桌面同步进程核验未完成"))?
}

#[cfg(windows)]
fn windows_listener(executable: &Path, desktop_pids: &[u32]) -> Result<()> {
    use std::{ffi::c_void, mem::size_of, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, GetLastError, HANDLE, NO_ERROR},
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
            TCP_TABLE_OWNER_PID_LISTENER,
        },
        Networking::WinSock::{AF_INET, AF_INET6},
        Security::{
            EqualSid, GetTokenInformation, IsValidSid, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
    };
    struct OwnedHandle(HANDLE);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    // Each buffer is aligned and bounded. No command line or environment read.
    fn table(family: u16) -> Result<(Vec<u64>, usize)> {
        let mut needed = 0u32;
        let initial = unsafe {
            GetExtendedTcpTable(
                std::ptr::null_mut(),
                &mut needed,
                0,
                family as u32,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        ensure!(
            initial == ERROR_INSUFFICIENT_BUFFER || initial == NO_ERROR,
            "天工桌面同步端口无法核验"
        );
        for _ in 0..3 {
            ensure!(
                (4..=1024 * 1024).contains(&needed),
                "天工桌面同步端口信息超过限制"
            );
            let mut data = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
            let capacity = data.len() * size_of::<u64>();
            let mut length = capacity as u32;
            let status = unsafe {
                GetExtendedTcpTable(
                    data.as_mut_ptr().cast(),
                    &mut length,
                    0,
                    family as u32,
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            };
            if status == ERROR_INSUFFICIENT_BUFFER {
                needed = length;
                continue;
            }
            ensure!(
                status == NO_ERROR && length >= 4 && length as usize <= capacity,
                "天工桌面同步端口信息无法核验"
            );
            return Ok((data, length as usize));
        }
        anyhow::bail!("天工桌面同步端口状态正在变化")
    }
    fn rows<T: Copy>(data: &[u64], length: usize) -> Result<Vec<T>> {
        let pointer = data.as_ptr().cast::<u8>();
        let count = unsafe { std::ptr::read_unaligned(pointer.cast::<u32>()) } as usize;
        ensure!(
            count <= 16384
                && count
                    .checked_mul(size_of::<T>())
                    .and_then(|n| n.checked_add(4))
                    .is_some_and(|end| end <= length),
            "天工桌面同步端口表无法核验"
        );
        Ok((0..count)
            .map(|index| unsafe {
                std::ptr::read_unaligned(pointer.add(4 + index * size_of::<T>()).cast::<T>())
            })
            .collect())
    }
    fn token(process: HANDLE) -> Result<OwnedHandle> {
        let mut handle = std::ptr::null_mut();
        ensure!(
            unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut handle) } != 0
                && !handle.is_null(),
            "天工桌面同步进程用户无法核验"
        );
        Ok(OwnedHandle(handle))
    }
    fn user(token: &OwnedHandle) -> Result<Vec<usize>> {
        let mut needed = 0u32;
        ensure!(
            unsafe {
                GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut needed)
            } == 0
                && unsafe { GetLastError() } == ERROR_INSUFFICIENT_BUFFER
                && needed as usize >= size_of::<TOKEN_USER>()
                && needed <= 64 * 1024,
            "天工桌面同步进程用户信息无法核验"
        );
        let mut data = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        let capacity = data.len() * size_of::<usize>();
        ensure!(
            unsafe {
                GetTokenInformation(
                    token.0,
                    TokenUser,
                    data.as_mut_ptr().cast(),
                    capacity as u32,
                    &mut needed,
                )
            } != 0
                && needed as usize >= size_of::<TOKEN_USER>()
                && needed as usize <= capacity,
            "天工桌面同步进程用户信息无法读取"
        );
        Ok(data)
    }
    fn sid(data: &[usize]) -> Result<PSID> {
        let pointer = data.as_ptr().cast::<u8>();
        let length = data.len() * size_of::<usize>();
        let user = unsafe { std::ptr::read(pointer.cast::<TOKEN_USER>()) };
        let sid = user.User.Sid.cast::<u8>();
        let base = pointer as usize;
        let start = sid as usize;
        ensure!(
            !sid.is_null()
                && start >= base
                && start.checked_add(8).is_some_and(|end| end <= base + length),
            "天工桌面同步进程用户身份无效"
        );
        let subauthorities = unsafe { *sid.add(1) } as usize;
        ensure!(
            subauthorities <= 15
                && start
                    .checked_add(8 + subauthorities * 4)
                    .is_some_and(|end| end <= base + length)
                && unsafe { IsValidSid(sid.cast::<c_void>()) } != 0,
            "天工桌面同步进程用户身份无法核验"
        );
        Ok(sid.cast::<c_void>())
    }
    let (data, length) = table(AF_INET)?;
    let mut owners = Vec::new();
    for row in rows::<MIB_TCPROW_OWNER_PID>(&data, length)? {
        if u16::from_be(row.dwLocalPort as u16) != DISPLAY_PORT {
            continue;
        }
        ensure!(
            row.dwLocalAddr.to_ne_bytes() == [127, 0, 0, 1] && row.dwOwningPid > 0,
            "天工桌面同步端口并非专用本机监听"
        );
        owners.push(row.dwOwningPid);
    }
    let (data, length) = table(AF_INET6)?;
    ensure!(
        rows::<MIB_TCP6ROW_OWNER_PID>(&data, length)?
            .iter()
            .all(|row| u16::from_be(row.dwLocalPort as u16) != DISPLAY_PORT),
        "天工桌面同步端口存在其他监听"
    );
    ensure!(owners.len() == 1, "{}", ENABLE_HINT);
    let owner = owners[0];
    ensure!(
        desktop_pids.contains(&owner),
        "天工桌面同步端口不属于当前天工实例"
    );
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, owner) };
    ensure!(!process.is_null(), "天工桌面同步进程无法查询");
    let process = OwnedHandle(process);
    let mut path = vec![0u16; 32768];
    let mut count = path.len() as u32;
    ensure!(
        unsafe { QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut count) } != 0
            && count > 0
            && count as usize <= path.len(),
        "天工桌面同步进程安装路径无法读取"
    );
    path.truncate(count as usize);
    let actual = PathBuf::from(std::ffi::OsString::from_wide(&path))
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("天工桌面同步进程安装路径无法核验"))?;
    ensure!(
        actual
            .to_string_lossy()
            .eq_ignore_ascii_case(&executable.to_string_lossy()),
        "天工桌面同步进程安装路径不匹配"
    );
    let actual_user = user(&token(process.0)?)?;
    let current_user = user(&token(unsafe { GetCurrentProcess() })?)?;
    ensure!(
        unsafe { EqualSid(sid(&actual_user)?, sid(&current_user)?) } != 0,
        "天工桌面同步进程不属于当前用户"
    );
    Ok(())
}

#[cfg(target_os = "macos")]
async fn verify_listener(executable: &Path, desktop_pids: &[u32]) -> Result<()> {
    let mut command = tokio::process::Command::new("/usr/sbin/lsof");
    command.args(["-nP", "-iTCP:18769", "-sTCP:LISTEN", "-Fpun"]);
    let bytes = helper_output(command).await?;
    let output =
        std::str::from_utf8(&bytes).map_err(|_| anyhow::anyhow!("天工桌面同步进程格式无法核验"))?;
    let mut pid = None;
    let mut uid = None;
    let mut address = None;
    for line in output.lines() {
        if let Some(value) = line.strip_prefix('p') {
            ensure!(pid.is_none(), "天工桌面同步进程无法唯一核验");
            pid = value.parse::<u32>().ok();
        }
        if let Some(value) = line.strip_prefix('u') {
            uid = value.parse::<u32>().ok();
        }
        if let Some(value) = line.strip_prefix('n') {
            ensure!(address.is_none(), "天工桌面同步端口无法唯一核验");
            address = Some(value);
        }
    }
    let pid = pid
        .filter(|value| desktop_pids.contains(value))
        .ok_or_else(|| anyhow::anyhow!(ENABLE_HINT))?;
    let mut command = tokio::process::Command::new("/usr/bin/id");
    command.arg("-u");
    let current = helper_output(command).await?;
    ensure!(
        uid == std::str::from_utf8(&current)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            && uid.is_some()
            && address == Some("127.0.0.1:18769"),
        "天工桌面同步端口不属于当前用户本机实例"
    );
    let mut command = tokio::process::Command::new("/bin/ps");
    command.args(["-p", &pid.to_string(), "-o", "comm="]);
    let path = helper_output(command).await?;
    let actual = std::str::from_utf8(&path)
        .ok()
        .and_then(|s| Path::new(s.trim()).canonicalize().ok());
    ensure!(
        actual.as_deref() == Some(executable),
        "天工桌面同步进程安装路径无法核验"
    );
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
async fn verify_listener(_: &Path, _: &[u32]) -> Result<()> {
    anyhow::bail!("此平台不支持天工桌面消息同步")
}
