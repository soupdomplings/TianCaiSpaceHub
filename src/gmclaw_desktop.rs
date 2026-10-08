//! Official GMClaw 1.1.1 desktop task storage API.
//!
//! This client only uses the authenticated local DataServer. It does not access
//! the desktop database, execute models, refresh the renderer, or retry writes.
use std::{collections::HashSet, path::Path, time::Duration};

use anyhow::{Result, bail, ensure};
use futures_util::StreamExt;
use reqwest::{Client, Method, StatusCode, header::HeaderValue};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use url::Url;
use uuid::Uuid;

use crate::gmclaw_executor::MAX_SESSION_STEPS;

const DATA_ENDPOINT: &str = "http://127.0.0.1:18768";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
// Native MCP discovery can perform three sequential requests of up to 30s.
// Keep this budget separate from the short desktop task/storage requests.
const MCP_REQUEST_TIMEOUT: Duration = Duration::from_secs(100);
const MAX_MCP_BYTES: usize = 4 * 1024 * 1024;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_LIST_BYTES: usize = 8 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 256 * 1024;
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_SESSION_EVENTS_BYTES: usize = 4 * 1024 * 1024;
const MAX_WRITE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TASKS: usize = 20_000;
pub(crate) const MAX_PROJECTS: usize = 20_000;

#[derive(Clone)]
pub(crate) struct DesktopClient {
    http: Client,
    mcp_http: Client,
    base_url: Url,
    authorization: HeaderValue,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DesktopTask {
    pub task_id: String,
    pub session_id: String,
    pub scenario_id: String,
    pub scenario_name: String,
    pub work_dir: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

/// Directory metadata also exists for desktop projects that have no tasks.
/// Ignore the task summaries in /data/scenarios when loading this catalog.
#[derive(Clone, Deserialize)]
pub(crate) struct DesktopProject {
    pub scenario_id: String,
    pub work_dir: String,
}

#[derive(Deserialize)]
struct ProjectList {
    scenarios: Vec<DesktopProject>,
}

#[derive(Clone, Deserialize, PartialEq)]
pub(crate) struct DesktopSessionMetadata {
    pub session_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub model_name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub updated_at: String,
    pub message_count: u64,
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
    #[serde(
        default = "empty_extra_data",
        deserialize_with = "deserialize_extra_data"
    )]
    pub extra_data: Value,
}

impl Default for DesktopSessionMetadata {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            title: String::new(),
            user_id: "local".to_owned(),
            agent_id: "desktop-orchestration".to_owned(),
            project_id: "default".to_owned(),
            model_name: String::new(),
            status: "active".to_owned(),
            updated_at: String::new(),
            message_count: 0,
            max_steps: default_max_steps(),
            extra_data: json!({}),
        }
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct DesktopMessage {
    pub id: i64,
    pub task_id: String,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub thinking_steps: Option<Vec<Value>>,
    #[serde(default)]
    pub files: Vec<Value>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Clone, Deserialize)]
pub(crate) struct DesktopMessagePage {
    pub messages: Vec<DesktopMessage>,
    #[serde(default)]
    pub has_more: bool,
}

#[derive(Clone, Deserialize, PartialEq)]
pub(crate) struct DesktopSessionEvent {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Deserialize)]
struct SessionEvents {
    messages: Vec<DesktopSessionEvent>,
}

#[derive(Deserialize)]
struct ScenarioList {
    scenarios: Vec<Scenario>,
}

#[derive(Deserialize)]
struct Scenario {
    scenario_id: String,
    name: String,
    #[serde(default)]
    work_dir: String,
    #[serde(default)]
    tasks: Vec<TaskRow>,
}

#[derive(Deserialize)]
struct TaskRow {
    task_id: String,
    session_id: String,
    scenario_id: String,
    title: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    completed_at: Option<String>,
}

#[derive(Deserialize)]
struct MessageCreated {
    id: i64,
    status: String,
}

#[derive(Deserialize)]
struct SessionCreated {
    session_id: String,
    status: String,
}

#[derive(Deserialize)]
struct WriteStatus {
    status: String,
}

impl DesktopClient {
    pub(crate) fn with_authorization(
        endpoint: &str,
        mut authorization: HeaderValue,
    ) -> Result<Self> {
        let harness = crate::gmclaw_executor::validate_endpoint(endpoint)?;
        ensure!(
            harness.scheme() == "http"
                && harness.host_str() == Some("127.0.0.1")
                && harness.port_or_known_default() == Some(7861),
            "天工桌面会话只支持官方本机默认服务地址"
        );
        ensure!(
            authorization
                .to_str()
                .ok()
                .is_some_and(|value| value.starts_with("Bearer ") && value.len() > 7),
            "天工桌面连接授权无效"
        );
        authorization.set_sensitive(true);
        let http = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(1))
            .read_timeout(Duration::from_secs(3))
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| anyhow::anyhow!("无法准备天工桌面本地连接"))?;
        let mcp_http = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(1))
            .read_timeout(MCP_REQUEST_TIMEOUT)
            .timeout(MCP_REQUEST_TIMEOUT)
            .build()
            .map_err(|_| anyhow::anyhow!("无法准备天工 MCP 本地连接"))?;
        Ok(Self {
            http,
            mcp_http,
            base_url: Url::parse(DATA_ENDPOINT)
                .map_err(|_| anyhow::anyhow!("天工桌面本地地址无效"))?,
            authorization,
        })
    }

    /// Read only one connection. The returned value can contain local secrets;
    /// callers must never serialize it into public status or diagnostics.
    pub(crate) async fn mcp_connection(&self, name: &str) -> Result<Option<Value>> {
        validate_id(name)?;
        self.request_json(
            Method::GET,
            &["data", "mcp", "connections", name],
            None,
            None,
            MAX_MCP_BYTES,
            true,
        )
        .await
    }

    pub(crate) async fn mcp_create_connection(&self, connection: Value) -> Result<()> {
        validate_mcp_changes(&connection, true)?;
        self.request_json(
            Method::POST,
            &["data", "mcp", "connections"],
            None,
            Some(connection),
            MAX_METADATA_BYTES,
            false,
        )
        .await?;
        Ok(())
    }

    /// PUT preserves omitted fields; POST would reset them to native defaults.
    pub(crate) async fn mcp_update_connection(&self, name: &str, changes: Value) -> Result<()> {
        validate_id(name)?;
        validate_mcp_changes(&changes, false)?;
        self.request_json(
            Method::PUT,
            &["data", "mcp", "connections", name],
            None,
            Some(changes),
            MAX_METADATA_BYTES,
            false,
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn mcp_delete_connection(&self, name: &str) -> Result<()> {
        validate_id(name)?;
        self.request_json(
            Method::DELETE,
            &["data", "mcp", "connections", name],
            None,
            None,
            MAX_METADATA_BYTES,
            true,
        )
        .await?;
        Ok(())
    }

    /// Native ok only indicates its weak probe result, never strict MCP success.
    pub(crate) async fn mcp_test_connection(&self, request: Value) -> Result<Value> {
        self.mcp_probe(&["data", "mcp", "test"], request).await
    }

    /// Native discovery can report ok with an empty list after a protocol error.
    /// The caller must use its own strict bridge handshake for connection status.
    pub(crate) async fn mcp_discover_tools(&self, request: Value) -> Result<Value> {
        self.mcp_probe(&["data", "mcp", "tools", "discover"], request)
            .await
    }

    async fn mcp_probe(&self, path: &[&str], request: Value) -> Result<Value> {
        let object = request
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("MCP 探测参数无效"))?;
        ensure!(
            object.keys().all(|key| matches!(
                key.as_str(),
                "server_url" | "token" | "headers" | "timeout_ms" | "server_name"
            )),
            "MCP 探测包含不支持的字段"
        );
        self.request_json_with_budget(
            &self.mcp_http,
            MCP_REQUEST_TIMEOUT,
            Method::POST,
            path,
            None,
            Some(request),
            MAX_MCP_BYTES,
            false,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("天工 MCP 响应缺失"))
    }

    /// The desktop exposes tasks through scenarios, not GET /data/tasks/:id.
    pub(crate) async fn list_tasks(&self) -> Result<Vec<DesktopTask>> {
        let scenarios = self.scenarios().await?;
        flatten_tasks(scenarios)
    }

    /// Official scenario ordering includes manual projects with empty tasks.
    /// This GET does not load session messages or create any project/task.
    pub(crate) async fn list_projects(&self) -> Result<Vec<DesktopProject>> {
        let list: ProjectList = self
            .required_json(Method::GET, &["data", "scenarios"], None, MAX_LIST_BYTES)
            .await?;
        validate_projects(list.scenarios)
    }

    pub(crate) async fn task(&self, task_id: &str) -> Result<Option<DesktopTask>> {
        validate_id(task_id)?;
        Ok(self
            .list_tasks()
            .await?
            .into_iter()
            .find(|task| task.task_id == task_id))
    }

    pub(crate) async fn get_session_metadata(
        &self,
        session_id: &str,
    ) -> Result<Option<DesktopSessionMetadata>> {
        validate_id(session_id)?;
        let value = self
            .request_json(
                Method::GET,
                &["data", "sessions", session_id],
                None,
                None,
                MAX_METADATA_BYTES,
                true,
            )
            .await?;
        let metadata: Option<DesktopSessionMetadata> = value.map(decode_json).transpose()?;
        if let Some(metadata) = &metadata {
            ensure!(
                metadata.session_id == session_id,
                "天工桌面返回的会话身份不匹配"
            );
        }
        Ok(metadata)
    }

    /// Harness message_index restarts each turn. The row ID is the reliable
    /// storage order, and an `over` frame does not resolve a preceding confirm.
    pub(crate) async fn get_session_events(
        &self,
        session_id: &str,
    ) -> Result<Vec<DesktopSessionEvent>> {
        validate_id(session_id)?;
        let mut events: SessionEvents = self
            .required_json(
                Method::GET,
                &["data", "sessions", session_id, "messages"],
                None,
                MAX_SESSION_EVENTS_BYTES,
            )
            .await?;
        ensure!(
            events.messages.len() <= MAX_TASKS
                && events
                    .messages
                    .iter()
                    .all(|event| event.id > 0 && event.session_id == session_id),
            "天工桌面事件身份或数量无效，无法确认会话状态"
        );
        events.messages.sort_by_key(|event| event.id);
        ensure!(
            !events
                .messages
                .windows(2)
                .any(|pair| pair[0].id == pair[1].id),
            "天工桌面事件身份重复，无法确认会话状态"
        );
        Ok(events.messages)
    }

    /// IDs are chosen by the caller and retained across an ambiguous failure.
    /// There is no automatic replay or rollback of either POST.
    pub(crate) async fn create_task(
        &self,
        work_dir: &str,
        title: &str,
        session_id: &str,
        task_id: &str,
    ) -> Result<DesktopTask> {
        validate_id(session_id)?;
        validate_id(task_id)?;
        validate_text(title, 512, "天工桌面会话标题无效")?;
        ensure!(
            !work_dir.is_empty()
                && work_dir.len() <= 4096
                && !work_dir.chars().any(char::is_control)
                && Path::new(work_dir).is_absolute(),
            "天工桌面会话目录必须是有效的本机绝对目录"
        );
        let operation = async {
            let scenarios = self.scenarios().await?;
            ensure!(
                !scenarios
                    .iter()
                    .flat_map(|s| &s.tasks)
                    .any(|task| { task.task_id == task_id || task.session_id == session_id }),
                "天工桌面会话身份已存在；请核对之前的创建结果"
            );
            let scenario = match scenarios.into_iter().find(|scenario| {
                Path::new(&scenario.work_dir).is_absolute()
                    && project_path_key(&scenario.work_dir) == project_path_key(work_dir)
            }) {
                Some(scenario) => scenario,
                None => {
                    let scenario_id = format!("tiancaispacehub-{}", Uuid::new_v4());
                    let folder_name = Path::new(work_dir)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("IM");
                    let name = format!("TianCaiSpaceHub · {}", folder_name)
                        .chars()
                        .take(128)
                        .collect::<String>();
                    let body = json!({
                        "scenario_id": scenario_id,
                        "name": name,
                        "work_dir": work_dir,
                    });
                    let scenario: Scenario = self
                        .required_json(
                            Method::POST,
                            &["data", "scenarios"],
                            Some(body),
                            MAX_METADATA_BYTES,
                        )
                        .await?;
                    ensure!(
                        scenario.scenario_id == scenario_id && scenario.work_dir == work_dir,
                        "天工桌面返回的项目身份不匹配；请核对创建结果"
                    );
                    scenario
                }
            };
            validate_id(&scenario.scenario_id)?;
            let row: TaskRow = self
                .required_json(
                    Method::POST,
                    &["data", "tasks"],
                    Some(json!({
                        "task_id": task_id,
                        "session_id": session_id,
                        "scenario_id": scenario.scenario_id,
                        "title": title,
                    })),
                    MAX_METADATA_BYTES,
                )
                .await?;
            ensure!(
                row.task_id == task_id
                    && row.session_id == session_id
                    && row.scenario_id == scenario.scenario_id,
                "天工桌面返回的任务身份不匹配；请核对创建结果"
            );
            Ok(task_from_row(&scenario, row))
        };
        tokio::time::timeout(OPERATION_TIMEOUT, operation)
            .await
            .map_err(|_| anyhow::anyhow!("天工桌面创建等待超时；请核对结果，不会自动重试"))?
    }

    pub(crate) async fn create_session_metadata(
        &self,
        metadata: &DesktopSessionMetadata,
    ) -> Result<()> {
        validate_id(&metadata.session_id)?;
        ensure!(
            (1..=MAX_SESSION_STEPS).contains(&metadata.max_steps),
            "天工会话执行步数须在 1–1000 之间"
        );
        let result: SessionCreated = self
            .required_json(
                Method::POST,
                &["data", "sessions"],
                Some(json!({
                    "session_id": metadata.session_id,
                    "title": metadata.title,
                    "user_id": metadata.user_id,
                    "agent_id": metadata.agent_id,
                    "project_id": metadata.project_id,
                    "model_name": metadata.model_name,
                    "status": metadata.status,
                    "max_steps": metadata.max_steps,
                    "extra_data": metadata.extra_data.to_string(),
                })),
                MAX_METADATA_BYTES,
            )
            .await?;
        ensure!(
            result.status == "created" && result.session_id == metadata.session_id,
            "天工桌面未确认会话登记或返回的会话身份不匹配"
        );
        Ok(())
    }

    /// The caller must merge the original extra_data object before updating it.
    /// No read/modify/write of private desktop database files is performed.
    pub(crate) async fn update_session_metadata(
        &self,
        session_id: &str,
        changes: &Value,
    ) -> Result<()> {
        validate_id(session_id)?;
        let body = metadata_update_body(changes)?;
        let result: WriteStatus = self
            .required_json(
                Method::PUT,
                &["data", "sessions", session_id],
                Some(Value::Object(body)),
                MAX_METADATA_BYTES,
            )
            .await?;
        ensure!(result.status == "updated", "天工桌面未确认会话更新");
        Ok(())
    }

    /// Return a bounded page, preserving the desktop's chronological ordering.
    pub(crate) async fn messages(
        &self,
        task_id: &str,
        before: Option<i64>,
    ) -> Result<DesktopMessagePage> {
        validate_id(task_id)?;
        ensure!(before.is_none_or(|id| id > 0), "天工消息分页身份无效");
        let mut query = vec![("limit", "100".to_owned())];
        if let Some(before) = before {
            query.push(("before", before.to_string()));
        }
        let value = self
            .request_json(
                Method::GET,
                &["data", "tasks", task_id, "messages"],
                Some(&query),
                None,
                MAX_MESSAGE_BYTES,
                false,
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("天工桌面消息响应缺失"))?;
        let page: DesktopMessagePage = decode_json(value)?;
        ensure!(
            page.messages.len() <= 100
                && page
                    .messages
                    .iter()
                    .all(|message| message.id > 0 && message.task_id == task_id),
            "天工桌面返回的消息身份或分页范围无效"
        );
        Ok(page)
    }

    /// The desktop represents an assistant response using role="system".
    pub(crate) async fn append_message(
        &self,
        task_id: &str,
        role: &str,
        content: &str,
        thinking_steps: &[Value],
    ) -> Result<i64> {
        validate_id(task_id)?;
        ensure!(matches!(role, "user" | "system"), "天工桌面消息角色无效");
        ensure!(content.len() <= 4 * 1024 * 1024, "天工桌面消息超过保存范围");
        let result: MessageCreated = self
            .required_json(
                Method::POST,
                &["data", "tasks", task_id, "messages"],
                Some(json!({
                    "role": role,
                    "content": content,
                    "thinking_steps": thinking_steps,
                })),
                MAX_METADATA_BYTES,
            )
            .await?;
        ensure!(
            result.id > 0 && result.status == "created",
            "天工桌面未确认消息保存；请核对结果，不会自动重试"
        );
        Ok(result.id)
    }

    pub(crate) async fn update_message(
        &self,
        task_id: &str,
        message_id: i64,
        content: &str,
        thinking_steps: &[Value],
    ) -> Result<()> {
        validate_id(task_id)?;
        ensure!(message_id > 0, "天工桌面消息身份无效");
        ensure!(content.len() <= 4 * 1024 * 1024, "天工桌面消息超过保存范围");
        let result: WriteStatus = self
            .required_json(
                Method::PUT,
                &[
                    "data",
                    "tasks",
                    task_id,
                    "messages",
                    &message_id.to_string(),
                ],
                Some(json!({
                    "content": content,
                    "thinking_steps": thinking_steps,
                })),
                MAX_METADATA_BYTES,
            )
            .await?;
        ensure!(result.status == "updated", "天工桌面未确认消息更新");
        Ok(())
    }

    pub(crate) async fn complete_task(&self, task_id: &str) -> Result<()> {
        validate_id(task_id)?;
        let result: WriteStatus = self
            .required_json(
                Method::POST,
                &["data", "tasks", task_id, "complete"],
                Some(json!({})),
                MAX_METADATA_BYTES,
            )
            .await?;
        ensure!(result.status == "completed", "天工桌面未确认任务完成标记");
        Ok(())
    }

    /// Rename locally; unlike generate-title this does not call a model.
    pub(crate) async fn rename_task(&self, task_id: &str, title: &str) -> Result<()> {
        validate_id(task_id)?;
        validate_text(title, 512, "天工桌面会话标题无效")?;
        let result: WriteStatus = self
            .required_json(
                Method::PUT,
                &["data", "tasks", task_id],
                Some(json!({ "title": title })),
                MAX_METADATA_BYTES,
            )
            .await?;
        ensure!(result.status == "updated", "天工桌面未确认任务标题更新");
        Ok(())
    }

    async fn scenarios(&self) -> Result<Vec<Scenario>> {
        let list: ScenarioList = self
            .required_json(Method::GET, &["data", "scenarios"], None, MAX_LIST_BYTES)
            .await?;
        ensure!(
            list.scenarios.len() <= MAX_TASKS
                && list.scenarios.iter().map(|s| s.tasks.len()).sum::<usize>() <= MAX_TASKS,
            "天工桌面会话数量超过读取范围，请先在桌面整理历史"
        );
        Ok(list.scenarios)
    }

    async fn required_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &[&str],
        body: Option<Value>,
        max_bytes: usize,
    ) -> Result<T> {
        let value = self
            .request_json(method, path, None, body, max_bytes, false)
            .await?
            .ok_or_else(|| anyhow::anyhow!("天工桌面响应缺失"))?;
        decode_json(value)
    }

    async fn request_json(
        &self,
        method: Method,
        path: &[&str],
        query: Option<&[(&str, String)]>,
        body: Option<Value>,
        max_bytes: usize,
        allow_not_found: bool,
    ) -> Result<Option<Value>> {
        self.request_json_with_budget(
            &self.http,
            REQUEST_TIMEOUT,
            method,
            path,
            query,
            body,
            max_bytes,
            allow_not_found,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn request_json_with_budget(
        &self,
        http: &Client,
        timeout: Duration,
        method: Method,
        path: &[&str],
        query: Option<&[(&str, String)]>,
        body: Option<Value>,
        max_bytes: usize,
        allow_not_found: bool,
    ) -> Result<Option<Value>> {
        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("天工桌面本地地址无效"))?
            .clear()
            .extend(path.iter().copied());
        if let Some(query) = query {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(key, value)| (*key, value.as_str())));
        }
        let mut request = http
            .request(method, url)
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(body) = body {
            let bytes = serde_json::to_vec(&body)
                .map_err(|_| anyhow::anyhow!("天工桌面保存内容格式无效"))?;
            ensure!(bytes.len() <= MAX_WRITE_BYTES, "天工桌面保存内容超过范围");
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(bytes);
        }
        let operation = async {
            let response = request.send().await.map_err(|error| {
                if error.is_timeout() {
                    anyhow::anyhow!("天工桌面本地服务响应超时；写入不会自动重试")
                } else {
                    anyhow::anyhow!("无法连接天工桌面本地服务；写入不会自动重试")
                }
            })?;
            let status = response.status();
            if allow_not_found && status == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                bail!("天工桌面运行授权已变化，请等待 Hub 自动重新连接");
            }
            ensure!(
                status.is_success(),
                "天工桌面本地服务返回 HTTP {}；写入不会自动重试",
                status.as_u16()
            );
            ensure!(
                response
                    .content_length()
                    .is_none_or(|len| len <= max_bytes as u64),
                "天工桌面响应超过读取范围"
            );
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk =
                    chunk.map_err(|_| anyhow::anyhow!("天工桌面响应中断；写入不会自动重试"))?;
                ensure!(
                    chunk.len() <= max_bytes.saturating_sub(bytes.len()),
                    "天工桌面响应超过读取范围"
                );
                bytes.extend_from_slice(&chunk);
            }
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| anyhow::anyhow!("天工桌面响应格式无效"))
        };
        tokio::time::timeout(timeout, operation)
            .await
            .map_err(|_| anyhow::anyhow!("天工桌面本地服务响应超时；写入不会自动重试"))?
    }
}

fn validate_mcp_changes(value: &Value, create: bool) -> Result<()> {
    const FIELDS: &[&str] = &[
        "server_id",
        "description",
        "server_url",
        "connect_type",
        "timeout_ms",
        "token_encrypted",
        "header_config",
        "config_param",
        "tools_json",
        "status",
        "is_connected",
        "conn_last_error",
        "retry_count",
    ];
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("天工 MCP 保存参数无效"))?;
    ensure!(
        !object.is_empty()
            && object
                .keys()
                .all(|key| FIELDS.contains(&key.as_str()) || (create && key == "server_name")),
        "天工 MCP 保存包含不支持的字段"
    );
    if create {
        validate_id(
            object
                .get("server_name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )?;
    }
    Ok(())
}

fn flatten_tasks(scenarios: Vec<Scenario>) -> Result<Vec<DesktopTask>> {
    let mut seen_tasks = HashSet::new();
    let mut seen_sessions = HashSet::new();
    let mut tasks = Vec::new();
    for mut scenario in scenarios {
        validate_id(&scenario.scenario_id)?;
        for row in std::mem::take(&mut scenario.tasks) {
            validate_id(&row.task_id)?;
            validate_id(&row.session_id)?;
            ensure!(
                row.scenario_id == scenario.scenario_id
                    && seen_tasks.insert(row.task_id.clone())
                    && seen_sessions.insert(row.session_id.clone()),
                "天工桌面会话身份重复或项目关联不一致"
            );
            tasks.push(task_from_row(&scenario, row));
        }
    }
    Ok(tasks)
}

fn validate_projects(projects: Vec<DesktopProject>) -> Result<Vec<DesktopProject>> {
    ensure!(
        projects.len() <= MAX_PROJECTS,
        "天工项目数量超过完整读取范围"
    );
    let mut ids = HashSet::new();
    for project in &projects {
        validate_id(&project.scenario_id)?;
        ensure!(
            ids.insert(&project.scenario_id),
            "天工项目身份重复，无法确认完整目录列表"
        );
        if !project.work_dir.is_empty() {
            ensure!(
                project.work_dir.len() <= 4096
                    && !project.work_dir.chars().any(char::is_control)
                    && Path::new(&project.work_dir).is_absolute(),
                "天工项目目录格式无效，无法确认完整目录列表"
            );
        }
    }
    Ok(projects)
}

/// Compare native paths without touching the filesystem. Keep the original
/// value for display and creation; only Windows identities fold case/slashes.
pub(crate) fn project_path_key(value: &str) -> String {
    project_path_key_for_platform(value, cfg!(windows))
}

fn project_path_key_for_platform(value: &str, windows: bool) -> String {
    if !windows {
        let trimmed = value.trim_end_matches('/');
        return if trimmed.is_empty() && value.starts_with('/') {
            "/".into()
        } else {
            trimmed.to_owned()
        };
    }
    let value = value.replace('\\', "/");
    let lower = value.to_lowercase();
    let value = if lower.starts_with("//?/unc/") {
        format!("//{}", &value[8..])
    } else {
        value.strip_prefix("//?/").unwrap_or(&value).to_owned()
    };
    let prefix = if value.starts_with("//") { "//" } else { "" };
    let components = value
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>();
    format!("{prefix}{}", components.join("/")).to_lowercase()
}

fn task_from_row(scenario: &Scenario, row: TaskRow) -> DesktopTask {
    DesktopTask {
        task_id: row.task_id,
        session_id: row.session_id,
        scenario_id: row.scenario_id,
        scenario_name: scenario.name.clone(),
        work_dir: scenario.work_dir.clone(),
        title: row.title,
        created_at: row.created_at,
        updated_at: row.updated_at,
        completed_at: row.completed_at,
    }
}

fn decode_json<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| anyhow::anyhow!("天工桌面响应字段格式无效"))
}

fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 256
            && !matches!(id, "." | "..")
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        "天工桌面会话或消息身份无效"
    );
    Ok(())
}

fn validate_text(text: &str, max_bytes: usize, message: &str) -> Result<()> {
    ensure!(
        !text.trim().is_empty() && text.len() <= max_bytes && !text.chars().any(char::is_control),
        "{message}"
    );
    Ok(())
}

fn default_max_steps() -> u32 {
    20
}

fn empty_extra_data() -> Value {
    json!({})
}

fn deserialize_extra_data<'de, D>(deserializer: D) -> std::result::Result<Value, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(text) => serde_json::from_str(&text)
            .map_err(|_| serde::de::Error::custom("invalid desktop session metadata")),
        Value::Null => Ok(json!({})),
        value => Ok(value),
    }
}

fn metadata_update_body(changes: &Value) -> Result<Map<String, Value>> {
    const ALLOWED: &[&str] = &[
        "title",
        "user_id",
        "agent_id",
        "project_id",
        "model_name",
        "status",
        "message_count",
        "current_step",
        "max_steps",
        "extra_data",
        "error",
    ];
    let mut body = changes
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("天工会话更新字段格式无效"))?;
    ensure!(
        !body.is_empty() && body.keys().all(|key| ALLOWED.contains(&key.as_str())),
        "天工会话包含不支持的更新字段"
    );
    if let Some(extra_data) = body.get_mut("extra_data") {
        if let Value::String(text) = extra_data {
            serde_json::from_str::<Value>(text)
                .map_err(|_| anyhow::anyhow!("天工会话附加信息格式无效"))?;
        } else {
            *extra_data = Value::String(extra_data.to_string());
        }
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_catalog_keeps_manual_projects_without_any_task_metadata() {
        let native_path = if cfg!(windows) {
            "D:/fixture/manual-project"
        } else {
            "/fixture/manual-project"
        };
        let list: ProjectList = serde_json::from_value(json!({
            "scenarios": [
                {"scenario_id":"default", "work_dir":"", "tasks":[]},
                {"scenario_id":"manual-empty", "work_dir":native_path, "tasks":[]},
                // Task fields are unrelated to project directory discovery.
                {"scenario_id":"manual-with-tasks", "work_dir":native_path,
                 "tasks":[{"unneeded_task_field":"fixture"}]},
            ],
        }))
        .unwrap();
        let projects = validate_projects(list.scenarios).unwrap();
        assert_eq!(projects.len(), 3);
        assert_eq!(projects[1].scenario_id, "manual-empty");
        assert_eq!(projects[1].work_dir, native_path);
        assert_eq!(projects[2].scenario_id, "manual-with-tasks");
    }

    #[test]
    fn project_catalog_rejects_incomplete_invalid_duplicate_and_oversized_metadata() {
        for invalid in [
            json!({"scenarios":[{"scenario_id":"manual"}]}),
            json!({"scenarios":[{"work_dir":""}]}),
        ] {
            assert!(serde_json::from_value::<ProjectList>(invalid).is_err());
        }
        let duplicate = vec![
            DesktopProject {
                scenario_id: "same".into(),
                work_dir: String::new(),
            },
            DesktopProject {
                scenario_id: "same".into(),
                work_dir: String::new(),
            },
        ];
        assert!(validate_projects(duplicate).is_err());
        for value in ["relative/project", "\ninvalid", &"x".repeat(4097)] {
            assert!(
                validate_projects(vec![DesktopProject {
                    scenario_id: "manual".into(),
                    work_dir: value.into(),
                }])
                .is_err()
            );
        }
        let oversized = (0..=MAX_PROJECTS)
            .map(|index| DesktopProject {
                scenario_id: format!("project-{index}"),
                work_dir: String::new(),
            })
            .collect();
        assert!(validate_projects(oversized).is_err());
    }

    #[test]
    fn windows_directory_identity_matches_native_and_canonical_spellings() {
        let expected = project_path_key_for_platform("D:/Projects/Example", true);
        for value in [
            r"d:\projects\EXAMPLE\",
            r"\\?\D:\Projects\Example",
            "D:/Projects//./Example/",
        ] {
            assert_eq!(project_path_key_for_platform(value, true), expected);
        }
        assert_eq!(
            project_path_key_for_platform(r"\\?\UNC\SERVER\Share\Example", true),
            project_path_key_for_platform(r"\\server\share\example\", true),
        );
        assert_ne!(
            project_path_key_for_platform("/Users/Example", false),
            project_path_key_for_platform("/Users/example", false),
        );
        assert_eq!(project_path_key_for_platform("/", false), "/");
        assert_eq!(
            project_path_key_for_platform("/Users/Example/", false),
            "/Users/Example"
        );
    }

    #[test]
    fn scenarios_preserve_real_desktop_task_and_session_ids() {
        let list: ScenarioList = serde_json::from_value(json!({
            "scenarios": [{
                "scenario_id": "sc-1",
                "name": "Desktop project",
                "work_dir": "C:/fixture/project",
                "tasks": [{
                    "task_id": "task-1", "session_id": "1234567890123456789",
                    "scenario_id": "sc-1", "title": "Existing desktop chat",
                }],
            }],
        }))
        .unwrap();
        let tasks = flatten_tasks(list.scenarios).unwrap();
        assert_eq!(tasks[0].session_id, "1234567890123456789");
        assert_eq!(tasks[0].work_dir, "C:/fixture/project");
    }

    #[test]
    fn desktop_metadata_parses_json_string_without_dropping_original_fields() {
        let metadata: DesktopSessionMetadata = serde_json::from_value(json!({
            "session_id": "123", "model_name": "fixture-model",
            "message_count": 0, "max_steps": 1000,
            "extra_data": "{\"project\":{\"id\":\"sc-1\"},\"native\":true}",
        }))
        .unwrap();
        assert_eq!(metadata.max_steps, 1000);
        let mut extra = metadata.extra_data;
        extra["hub"] = json!({"source": "TianCaiSpaceHub"});
        let body = metadata_update_body(&json!({"extra_data": extra})).unwrap();
        let saved: Value = serde_json::from_str(body["extra_data"].as_str().unwrap()).unwrap();
        assert_eq!(saved["native"], true);
        assert_eq!(saved["project"]["id"], "sc-1");
    }

    #[test]
    fn desktop_client_rejects_custom_endpoint_and_path_identifiers() {
        let auth = HeaderValue::from_static("Bearer fixture");
        assert!(DesktopClient::with_authorization("http://127.0.0.1:9999", auth).is_err());
        for id in ["..", "../task", "task/other", "task?query=1", "task\\other"] {
            assert!(validate_id(id).is_err());
        }
    }
}
