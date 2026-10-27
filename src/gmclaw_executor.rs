//! Client for legacy TianGong Harness and Work run event protocols.
//!
//! The caller owns conversation routing, the global execution queue, and approval
//! identity checks. This client neither discovers credentials nor starts GMClaw.
use std::{collections::HashSet, future::Future, net::IpAddr, path::Path, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use reqwest::{Client, header::HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:7861";
/// GMClaw 1.1.1's native renderer submits 1000 steps. This compatibility
/// boundary is separate from the Hub's 1..=200 budget for new IM sessions.
pub(crate) const MAX_SESSION_STEPS: u32 = 1000;
const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub struct GmClawClient {
    http: Client,
    chat_url: Url,
    authorization: HeaderValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmClawProject {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct GmClawChatRequest {
    pub session_id: String,
    pub chat_id: String,
    /// Preserve the desktop user identity to load this real session's memory.
    pub user_id: String,
    pub query: String,
    /// The GMClaw model_configs row ID, not its upstream model_name.
    pub model_id: Option<String>,
    pub project: GmClawProject,
    pub max_steps: u32,
    pub confirmation: Option<GmClawConfirmationDecision>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GmClawConfirmationAction {
    Approve,
    Deny,
}

#[derive(Debug, Clone)]
pub struct GmClawConfirmationDecision {
    pub action: GmClawConfirmationAction,
    pub call_ids: Vec<String>,
    pub run: Option<GmClawRunIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmClawRunIdentity {
    pub run_id: String,
    pub chat_id: String,
    pub after_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmClawPendingCall {
    pub call_id: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmClawPendingConfirmation {
    #[serde(default)]
    pub prompt: String,
    pub pending_calls: Vec<GmClawPendingCall>,
    #[serde(default)]
    pub run: Option<GmClawRunIdentity>,
}

#[derive(Debug, Clone)]
pub enum GmClawEvent {
    Text(String),
    Confirmation(GmClawPendingConfirmation),
    Finished {
        answer: String,
        error: Option<String>,
    },
    /// Includes tool progress and future event types. Do not treat these as text
    /// deltas or automatically publish their potentially sensitive payloads.
    Progress {
        kind: String,
        data: Value,
    },
}

#[derive(Debug, Clone, Default)]
pub struct GmClawChatResult {
    pub answer: String,
    pub error: Option<String>,
    /// A confirm frame followed by over is a suspended turn, not completion.
    pub pending_confirmation: Option<GmClawPendingConfirmation>,
}

impl GmClawChatRequest {
    pub fn body(&self) -> Result<Value> {
        ensure!(!self.session_id.trim().is_empty(), "天工会话 ID 不能为空");
        ensure!(!self.chat_id.trim().is_empty(), "天工消息 ID 不能为空");
        ensure!(!self.user_id.trim().is_empty(), "天工用户 ID 不能为空");
        ensure!(
            (1..=MAX_SESSION_STEPS).contains(&self.max_steps),
            "天工执行步数须在 1–1000 之间"
        );
        ensure!(
            Path::new(&self.project.path).is_absolute(),
            "天工工作目录必须是绝对路径"
        );
        ensure!(
            Path::new(&self.project.path).is_dir(),
            "天工工作目录必须是已存在的文件夹"
        );
        ensure!(
            !self.query.trim().is_empty() || self.confirmation.is_some(),
            "天工消息不能为空"
        );
        let mut body = json!({
            "session_id": self.session_id,
            "chat_id": self.chat_id,
            "user_id": self.user_id,
            "query": self.query,
            "project": self.project,
            "max_steps": self.max_steps,
            "tool_execution_mode": "confirm",
        });
        if let Some(model) = self
            .model_id
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            body["model_id"] = json!(model);
        }
        if let Some(decision) = &self.confirmation {
            ensure!(
                !decision.call_ids.is_empty()
                    && decision.call_ids.iter().all(|id| !id.trim().is_empty()),
                "天工审批必须指定当前等待确认的工具调用 ID"
            );
            ensure!(
                decision.call_ids.iter().collect::<HashSet<_>>().len() == decision.call_ids.len(),
                "天工审批工具调用 ID 不能重复"
            );
            body["confirm_action"] = json!(decision.action);
            body["approved_call_ids"] = json!(decision.call_ids);
        }
        Ok(body)
    }
}

/// Restrict the connection to the local desktop process. Credentials must never
/// follow redirects, a system proxy, or a user-info/query portion of a URL.
pub fn validate_endpoint(endpoint: &str) -> Result<Url> {
    let mut url = Url::parse(endpoint.trim()).context("天工地址格式无效")?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "天工地址必须使用 HTTP 或 HTTPS"
    );
    let host = url.host_str().unwrap_or_default();
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    ensure!(loopback, "天工执行端地址必须是本机回环地址");
    if host.eq_ignore_ascii_case("localhost") {
        // Pin the local name so a hosts/DNS override cannot redirect the token.
        url.set_host(Some("127.0.0.1"))
            .context("天工回环地址无效")?;
    }
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "天工地址不能包含账号、查询参数或片段"
    );
    ensure!(
        matches!(url.path().trim_end_matches('/'), "" | "/v2/chat"),
        "请填写天工 Harness 根地址或 /v2/chat 地址"
    );
    url.set_path("/v2/chat");
    Ok(url)
}

impl GmClawClient {
    pub fn new(endpoint: &str, token: &str) -> Result<Self> {
        validate_endpoint(endpoint)?;
        ensure!(
            !token.trim().is_empty(),
            "天工连接授权尚未准备好，请在聊天中发送 /tg 自动连接"
        );
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", token.trim()))
            .context("天工连接 Token 格式无效")?;
        authorization.set_sensitive(true);
        Self::with_authorization(endpoint, authorization)
    }

    pub(crate) fn with_authorization(
        endpoint: &str,
        mut authorization: HeaderValue,
    ) -> Result<Self> {
        let chat_url = validate_endpoint(endpoint)?;
        authorization.set_sensitive(true);
        let http = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(300))
            .timeout(Duration::from_secs(900))
            .build()
            .context("无法创建天工本地连接")?;
        Ok(Self {
            http,
            chat_url,
            authorization,
        })
    }

    /// Never retry execution automatically: a lost stream may already have
    /// executed tools. Dropping the request does not confirm tool cancellation.
    pub async fn execute<F, Fut>(
        &self,
        request: &GmClawChatRequest,
        mut on_event: F,
    ) -> Result<GmClawChatResult>
    where
        F: FnMut(GmClawEvent) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let resume = request
            .confirmation
            .as_ref()
            .and_then(|decision| decision.run.as_ref());
        let response = if let Some(run) = resume {
            self.resume(request, run).await?
        } else {
            self.http
                .post(self.chat_url.clone())
                .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .json(&request.body()?)
                .send()
                .await
                .context("无法连接天工执行端，请确认天工已启动；请求不会自动重试")?
        };
        match response.status().as_u16() {
            401 | 403 => {
                bail!("天工运行授权已变化，请在聊天中发送 /tg 自动重新连接；本次请求不会重试")
            }
            status if !(200..300).contains(&status) => {
                bail!("天工执行端返回 HTTP {status}；请求不会自动重试")
            }
            _ => {}
        }

        let run_id = if let Some(run) = resume {
            Some(run.run_id.clone())
        } else {
            response
                .headers()
                .get("x-run-id")
                .map(|value| value.to_str().map(str::to_owned))
                .transpose()
                .context("天工运行标识格式无效")?
        };
        let mut decoder = EventDecoder {
            identity: StreamIdentity {
                run_id,
                session_id: request.session_id.clone(),
                chat_id: resume.map_or_else(|| request.chat_id.clone(), |run| run.chat_id.clone()),
                seq: resume.map_or(0, |run| run.after_seq),
                terminal: false,
            },
            ..Default::default()
        };
        let mut stream = response.bytes_stream();
        let mut result = GmClawChatResult::default();
        let mut finished = false;
        let mut response_bytes = 0_usize;
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("天工响应流中断；执行状态未知，请勿自动重试")?;
            response_bytes = response_bytes.saturating_add(chunk.len());
            ensure!(
                response_bytes <= MAX_RESPONSE_BYTES,
                "天工响应超过 16 MiB；执行状态未知，请勿自动重试"
            );
            for event in decoder.push(&chunk)? {
                accumulate_event(&event, &mut result, &mut finished, &mut text)?;
                on_event(event).await?;
            }
        }
        if let Some(event) = decoder.finish()? {
            accumulate_event(&event, &mut result, &mut finished, &mut text)?;
            on_event(event).await?;
        }
        validate_result(&result, finished)?;
        if let Some(run_id) = decoder.identity.run_id {
            ensure!(
                decoder.identity.terminal,
                "天工运行未完成收尾；执行状态未知，请勿自动重试"
            );
            if let Some(pending) = &mut result.pending_confirmation {
                pending.run = Some(GmClawRunIdentity {
                    run_id,
                    chat_id: decoder.identity.chat_id,
                    after_seq: decoder.identity.seq,
                });
            }
        }
        Ok(result)
    }

    async fn resume(
        &self,
        request: &GmClawChatRequest,
        run: &GmClawRunIdentity,
    ) -> Result<reqwest::Response> {
        ensure!(
            !run.run_id.is_empty()
                && run.run_id.len() <= 256
                && run
                    .run_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')),
            "天工审批运行标识无效"
        );
        let mut url = self.chat_url.clone();
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("天工运行地址无效"))?
            .extend(["runs", run.run_id.as_str()]);
        let current = self
            .http
            .get(url.clone())
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .send()
            .await
            .context("无法检查原天工审批任务；未提交审批")?;
        ensure!(
            current.status().is_success(),
            "无法确认原天工审批任务；未提交审批"
        );
        let mut bytes = Vec::new();
        let mut stream = current.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("原天工审批任务检查中断；未提交审批")?;
            ensure!(
                bytes.len() + chunk.len() <= MAX_EVENT_BYTES,
                "原天工审批任务信息过大；未提交审批"
            );
            bytes.extend_from_slice(&chunk);
        }
        let current: Value = serde_json::from_slice(&bytes).context("原天工审批任务信息无效")?;
        ensure!(
            current["run_id"].as_str() == Some(&run.run_id)
                && current["session_id"].as_str() == Some(&request.session_id)
                && current["chat_id"].as_str() == Some(&run.chat_id)
                && current["status"].as_str() == Some("waiting_confirmation")
                && current["last_seq"].as_u64() == Some(run.after_seq),
            "天工审批任务已变化，请在天工核对；本次未提交或重放"
        );
        let mut resume_url = url.clone();
        resume_url.path_segments_mut().unwrap().push("resume");
        let resumed = self
            .http
            .post(resume_url)
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .json(&request.body()?)
            .send()
            .await
            .context("天工审批提交中断；执行状态未知，请勿重试")?;
        ensure!(
            resumed.status().is_success(),
            "天工未接受原任务审批；请核对任务状态，本次不会自动重试"
        );
        url.path_segments_mut().unwrap().push("events");
        url.query_pairs_mut()
            .append_pair("after_seq", &run.after_seq.to_string());
        self.http
            .get(url)
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .send()
            .await
            .context("天工审批已提交，但结果连接失败；执行状态未知，请勿重试")
    }
}

fn validate_result(result: &GmClawChatResult, finished: bool) -> Result<()> {
    ensure!(
        finished,
        "天工响应未包含结束事件；执行状态未知，请勿自动重试"
    );
    ensure!(
        !result.answer.trim().is_empty()
            || result.pending_confirmation.is_some()
            || result.error.is_some(),
        "天工返回了空结束事件，未提供答复或待确认工具；请检查天工中的任务状态"
    );
    Ok(())
}

fn accumulate_event(
    event: &GmClawEvent,
    result: &mut GmClawChatResult,
    finished: &mut bool,
    text: &mut String,
) -> Result<()> {
    if let GmClawEvent::Progress { kind, data } = event
        && kind == "run_status"
    {
        match data.get("status").and_then(Value::as_str) {
            Some("completed") => ensure!(
                *finished && result.pending_confirmation.is_none(),
                "天工运行结束状态与答复不一致"
            ),
            Some("waiting_confirmation") => ensure!(
                *finished && result.pending_confirmation.is_some(),
                "天工审批等待状态与答复不一致"
            ),
            Some("failed" | "stopped" | "interrupted") => {
                result.error = Some(
                    data.get("error")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("天工任务已停止或执行失败")
                        .to_owned(),
                );
                result.pending_confirmation = None;
            }
            Some("running")
                if !*finished
                    || data.get("phase").and_then(Value::as_str) == Some("finalizing") => {}
            _ => bail!("天工返回了未支持的运行变化；请到天工核对，不会自动重放"),
        }
        return Ok(());
    }
    if matches!(event, GmClawEvent::Progress { kind, .. } if kind.starts_with("subagent:")) {
        return Ok(());
    }
    ensure!(!*finished, "天工在结束事件后继续返回执行事件");
    match event {
        GmClawEvent::Text(delta) => {
            ensure!(
                text.len().saturating_add(delta.len()) <= MAX_TEXT_BYTES,
                "天工累计答复超过 4 MiB"
            );
            text.push_str(delta);
        }
        GmClawEvent::Confirmation(pending) => result.pending_confirmation = Some(pending.clone()),
        GmClawEvent::Finished { answer, error } => {
            result.answer = if answer.is_empty() && error.is_none() {
                text.clone()
            } else {
                answer.clone()
            };
            result.error = error.clone();
            if error.is_some() {
                result.pending_confirmation = None;
            }
            *finished = true;
        }
        _ => {}
    }
    Ok(())
}

/// Harness calls this SSE but emits one prefix|JSON record per line, without
/// standard SSE data: fields. Buffer bytes until newline so split UTF-8 survives.
#[derive(Default)]
struct EventDecoder {
    pending: Vec<u8>,
    identity: StreamIdentity,
}

#[derive(Default)]
struct StreamIdentity {
    run_id: Option<String>,
    session_id: String,
    chat_id: String,
    seq: u64,
    terminal: bool,
}

impl StreamIdentity {
    fn check(&mut self, kind: &str, data: &Value) -> Result<bool> {
        if let Some(run_id) = &self.run_id {
            ensure!(
                !self.terminal
                    && data["run_id"].as_str() == Some(run_id)
                    && data["session_id"].as_str() == Some(&self.session_id)
                    && data["chat_id"].as_str() == Some(&self.chat_id),
                "天工事件与当前运行身份不一致"
            );
            let seq = data["seq"].as_u64().context("天工运行事件缺少有效序号")?;
            ensure!(seq == self.seq + 1, "天工运行事件序号不连续，执行状态未知");
            self.seq = seq;
            let subagent = nonempty(data, "subagent_run_id") || nonempty(data, "child_session_id");
            if !subagent
                && kind == "run_status"
                && matches!(
                    data["status"].as_str(),
                    Some(
                        "completed" | "failed" | "stopped" | "interrupted" | "waiting_confirmation"
                    )
                )
            {
                self.terminal = true;
            }
            Ok(subagent)
        } else {
            Ok(nonempty(data, "run_id")
                || nonempty(data, "subagent_run_id")
                || nonempty(data, "child_session_id"))
        }
    }
}

fn nonempty(data: &Value, key: &str) -> bool {
    data.get(key)
        .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
}

impl EventDecoder {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<GmClawEvent>> {
        let mut events = Vec::new();
        for segment in chunk.split_inclusive(|byte| *byte == b'\n') {
            ensure!(
                self.pending.len() + segment.len() <= MAX_EVENT_BYTES,
                "天工单条事件过大"
            );
            self.pending.extend_from_slice(segment);
            if self.pending.last() == Some(&b'\n') {
                if let Some(event) = parse_event_bound(&self.pending, &mut self.identity)? {
                    events.push(event);
                }
                self.pending.clear();
            }
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Option<GmClawEvent>> {
        let event = parse_event_bound(&self.pending, &mut self.identity)?;
        self.pending.clear();
        Ok(event)
    }
}

#[cfg(test)]
fn parse_event(line: &[u8]) -> Result<Option<GmClawEvent>> {
    parse_event_bound(line, &mut StreamIdentity::default())
}

fn parse_event_bound(line: &[u8], identity: &mut StreamIdentity) -> Result<Option<GmClawEvent>> {
    let line = std::str::from_utf8(line)
        .context("天工响应包含无效 UTF-8")?
        .trim();
    if line.is_empty() {
        return Ok(None);
    }
    let (kind, payload) = line
        .split_once('|')
        .context("天工响应不是 prefix|JSON 事件")?;
    ensure!(!kind.is_empty(), "天工响应事件类型为空");
    let data: Value = serde_json::from_str(payload).context("天工事件 JSON 无效")?;
    ensure!(data.is_object(), "天工事件内容必须是 JSON 对象");
    if identity.check(kind, &data)? {
        ensure!(
            kind != "confirm",
            "天工子 Agent 审批暂不支持通过 IM 续跑，请在天工桌面处理"
        );
        return Ok(Some(GmClawEvent::Progress {
            kind: format!("subagent:{kind}"),
            data,
        }));
    }
    let event = match kind {
        "text" => GmClawEvent::Text(
            data.get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ),
        "confirm" => {
            let mut pending: GmClawPendingConfirmation =
                serde_json::from_value(data).context("天工审批事件格式无效")?;
            pending.run = None; // Only the verified response stream may bind a run.
            ensure!(
                !pending.pending_calls.is_empty()
                    && pending
                        .pending_calls
                        .iter()
                        .all(|call| !call.call_id.trim().is_empty()),
                "天工审批事件缺少工具调用 ID"
            );
            ensure!(
                pending
                    .pending_calls
                    .iter()
                    .map(|call| &call.call_id)
                    .collect::<HashSet<_>>()
                    .len()
                    == pending.pending_calls.len(),
                "天工审批事件包含重复工具调用 ID"
            );
            GmClawEvent::Confirmation(pending)
        }
        "over" => {
            ensure!(
                data.get("answer")
                    .is_none_or(|answer| answer.is_null() || answer.is_string()),
                "天工结束事件的答复字段无效"
            );
            let error = data
                .get("error")
                .filter(|v| !v.is_null() && v.as_str() != Some(""))
                .or_else(|| {
                    data.pointer("/extra_data/error")
                        .filter(|v| !v.is_null() && v.as_str() != Some(""))
                })
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string())
                });
            GmClawEvent::Finished {
                answer: data
                    .get("answer")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                error,
            }
        }
        _ => GmClawEvent::Progress {
            kind: kind.to_owned(),
            data,
        },
    };
    Ok(Some(event))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_unicode_confirmation_and_terminal_frame_survive_chunks() {
        let data = concat!(
            "text|{\"content\":\"请确认\"}\n",
            "confirm|{\"prompt\":\"执行工具\",\"pending_calls\":[{\"call_id\":\"call-1\",\"tool_name\":\"exec\",\"arguments\":{}}]}\r\n",
            "over|{\"answer\":\"\",\"error\":\"\"}"
        );
        let mut decoder = EventDecoder::default();
        let mut events = Vec::new();
        for chunk in data.as_bytes().chunks(2) {
            events.extend(decoder.push(chunk).unwrap());
        }
        events.extend(decoder.finish().unwrap());
        assert!(matches!(&events[0], GmClawEvent::Text(text) if text == "请确认"));
        let mut result = GmClawChatResult::default();
        let mut finished = false;
        let mut text = String::new();
        for event in &events {
            accumulate_event(event, &mut result, &mut finished, &mut text).unwrap();
        }
        assert!(finished);
        assert!(result.error.is_none());
        assert_eq!(result.answer, "请确认");
        assert_eq!(
            result.pending_confirmation.unwrap().pending_calls[0].call_id,
            "call-1"
        );
    }

    #[test]
    fn rejects_credential_forwarding_and_malformed_approval() {
        for endpoint in [
            "https://example.com",
            "http://127.0.0.1:7861?token=x",
            "http://user@localhost:7861",
            "file:///tmp/test",
            "http://localhost:7861/data",
        ] {
            assert!(validate_endpoint(endpoint).is_err(), "{endpoint}");
        }
        assert_eq!(
            validate_endpoint(DEFAULT_ENDPOINT).unwrap().as_str(),
            "http://127.0.0.1:7861/v2/chat"
        );
        assert!(validate_endpoint("http://[::1]:7861").is_ok());
        assert!(parse_event(br#"confirm|{"pending_calls":[]}"#).is_err());
        assert!(parse_event(br#"confirm|{"pending_calls":[{"call_id":""}]}"#).is_err());
        assert!(
            parse_event(br#"confirm|{"pending_calls":[{"call_id":"same"},{"call_id":"same"}]}"#)
                .is_err()
        );
        assert!(
            parse_event(
                br#"confirm|{"run_id":"child-run","pending_calls":[{"call_id":"child-call"}]}"#
            )
            .is_err()
        );
        assert!(parse_event(br#"over|{"answer":{"unexpected":true}}"#).is_err());
    }

    #[test]
    fn terminal_failure_discards_pending_approval_and_does_not_use_text_fallback() {
        let mut result = GmClawChatResult::default();
        let mut finished = false;
        let mut text = String::new();
        for line in [
            br#"text|{"content":"partial answer"}"#.as_slice(),
            br#"confirm|{"pending_calls":[{"call_id":"call-1"}]}"#.as_slice(),
            br#"over|{"extra_data":{"error":"execution failed"}}"#.as_slice(),
        ] {
            accumulate_event(
                &parse_event(line).unwrap().unwrap(),
                &mut result,
                &mut finished,
                &mut text,
            )
            .unwrap();
        }
        assert_eq!(result.error.as_deref(), Some("execution failed"));
        assert!(result.answer.is_empty());
        assert!(result.pending_confirmation.is_none());
        assert_eq!(
            serde_json::to_value(GmClawConfirmationAction::Deny).unwrap(),
            "deny"
        );
    }

    #[test]
    fn request_preserves_scoped_identity_and_uses_exact_approval_ids() {
        let workspace = tempfile::tempdir().unwrap();
        let mut request = GmClawChatRequest {
            session_id: "session-one".into(),
            chat_id: "turn-two".into(),
            user_id: "hub-im-isolated".into(),
            query: String::new(),
            model_id: Some("configured-row-id".into()),
            max_steps: 1000,
            project: GmClawProject {
                id: "hub-im-project".into(),
                name: "Hub IM".into(),
                path: workspace.path().display().to_string(),
            },
            confirmation: Some(GmClawConfirmationDecision {
                run: None,
                action: GmClawConfirmationAction::Deny,
                call_ids: vec!["call-exact".into()],
            }),
        };
        let body = request.body().unwrap();
        assert_eq!(body["user_id"], "hub-im-isolated");
        assert_eq!(body["project"]["id"], "hub-im-project");
        assert_eq!(body["model_id"], "configured-row-id");
        assert_eq!(body["max_steps"], 1000);
        assert_eq!(body["tool_execution_mode"], "confirm");
        assert_eq!(body["confirm_action"], "deny");
        assert_eq!(body["approved_call_ids"], json!(["call-exact"]));
        for invalid_steps in [0, 1001, u32::MAX] {
            request.max_steps = invalid_steps;
            assert!(request.body().is_err());
        }
        request.max_steps = 1000;
        request.confirmation.as_mut().unwrap().call_ids.clear();
        assert!(request.body().is_err());
        request.confirmation = None;
        assert!(request.body().is_err());
    }

    #[test]
    fn truncated_or_empty_completion_is_not_a_successful_turn() {
        let mut decoder = EventDecoder::default();
        decoder.push(br#"over|{"answer":"unfinished"#).unwrap();
        assert!(decoder.finish().is_err());
        let mut result = GmClawChatResult::default();
        let mut finished = false;
        let mut text = String::new();
        assert!(validate_result(&result, finished).is_err());
        let over = parse_event(br#"over|{}"#).unwrap().unwrap();
        accumulate_event(&over, &mut result, &mut finished, &mut text).unwrap();
        assert!(validate_result(&result, finished).is_err());
        assert!(
            accumulate_event(
                &GmClawEvent::Text("late".into()),
                &mut result,
                &mut finished,
                &mut text
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod work_protocol_tests {
    use super::*;

    fn bound() -> EventDecoder {
        EventDecoder {
            identity: StreamIdentity {
                run_id: Some("run-main".into()),
                session_id: "session".into(),
                chat_id: "chat".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn frame(seq: u64, kind: &str, mut data: Value) -> String {
        data["seq"] = json!(seq);
        data["run_id"] = json!("run-main");
        data["session_id"] = json!("session");
        data["chat_id"] = json!("chat");
        format!("{kind}|{data}\n")
    }
    fn collect_frames(frames: Vec<String>) -> Result<GmClawChatResult> {
        let mut decoder = bound();
        let mut result = GmClawChatResult::default();
        let mut finished = false;
        let mut text = String::new();
        for chunk in frames.join("").as_bytes().chunks(3) {
            for event in decoder.push(chunk)? {
                accumulate_event(&event, &mut result, &mut finished, &mut text)?;
            }
        }
        ensure!(decoder.identity.terminal, "missing run terminal");
        validate_result(&result, finished)?;
        Ok(result)
    }
    #[test]
    fn work_accepts_finalizing_and_terminal_but_not_more_execution() {
        let result = collect_frames(vec![
            frame(1, "run_status", json!({"status":"running"})),
            frame(2, "text", json!({"content":"你好"})),
            frame(3, "over", json!({})),
            frame(
                4,
                "run_status",
                json!({"status":"running","phase":"finalizing"}),
            ),
            frame(5, "run_status", json!({"status":"completed"})),
        ])
        .unwrap();
        assert_eq!(result.answer, "你好");
        assert!(
            collect_frames(vec![
                frame(1, "over", json!({"answer":"ok"})),
                frame(2, "text", json!({"content":"unexpected"}))
            ])
            .is_err()
        );
        assert!(collect_frames(vec![frame(1, "over", json!({"answer":"ok"}))]).is_err());
    }
    #[test]
    fn work_main_approval_and_subagent_identity_are_distinct() {
        let result = collect_frames(vec![
            frame(1, "confirm", json!({"pending_calls":[{"call_id":"tool"}]})),
            frame(2, "over", json!({})),
            frame(3, "run_status", json!({"status":"waiting_confirmation"})),
        ])
        .unwrap();
        assert_eq!(
            result.pending_confirmation.unwrap().pending_calls[0].call_id,
            "tool"
        );
        for key in ["subagent_run_id", "child_session_id"] {
            let mut pending = json!({"pending_calls":[{"call_id":"tool"}]});
            pending[key] = json!("child");
            assert!(
                bound()
                    .push(frame(1, "confirm", pending).as_bytes())
                    .is_err()
            );
        }
        let result = collect_frames(vec![
            frame(
                1,
                "over",
                json!({"answer":"child answer","subagent_run_id":"child"}),
            ),
            frame(
                2,
                "run_status",
                json!({"status":"completed","subagent_run_id":"child"}),
            ),
            frame(3, "over", json!({"answer":"parent answer"})),
            frame(4, "run_status", json!({"status":"completed"})),
        ])
        .unwrap();
        assert_eq!(result.answer, "parent answer");
    }
    #[test]
    fn work_rejects_cross_session_out_of_order_and_failure_is_not_success() {
        for (seq, data) in [(2, json!({})), (1, json!({"subagent_run_id":1}))] {
            let mut decoder = bound();
            let mut wire = frame(seq, "over", data);
            if seq == 1 {
                wire = wire.replace("\"session\"", "\"another-session\"");
            }
            assert!(decoder.push(wire.as_bytes()).is_err());
        }
        let result = collect_frames(vec![
            frame(1, "over", json!({"answer":"partial"})),
            frame(
                2,
                "run_status",
                json!({"status":"failed","error":"save failed"}),
            ),
        ])
        .unwrap();
        assert_eq!(result.error.as_deref(), Some("save failed"));
    }
    #[tokio::test]
    async fn work_resume_uses_the_exact_run_and_checks_cursor_before_writing() {
        use axum::{
            Json, Router,
            http::StatusCode,
            routing::{get, post},
        };
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        for stale in [false, true] {
            let posts = Arc::new(AtomicUsize::new(0));
            let count = posts.clone();
            let app=Router::new()
                .route("/v2/chat/runs/run-main",get(move || async move { Json(json!({"run_id":"run-main","session_id":"session","chat_id":"chat","last_seq":if stale {4} else {3},"status":"waiting_confirmation"})) }))
                .route("/v2/chat/runs/run-main/resume",post(move |Json(body):Json<Value>| {let count=count.clone();async move {assert_eq!(body["approved_call_ids"][0],"tool");count.fetch_add(1,Ordering::SeqCst);StatusCode::ACCEPTED}}))
                .route("/v2/chat/runs/run-main/events",get(|| async {
                    [frame(4,"run_status",json!({"status":"running"})),frame(5,"over",json!({"answer":"resumed"})),frame(6,"run_status",json!({"status":"completed"}))].join("")
                }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let client = GmClawClient::new(&format!("http://{address}"), "synthetic").unwrap();
            let directory = tempfile::tempdir().unwrap();
            let request = GmClawChatRequest {
                session_id: "session".into(),
                chat_id: "new-approval-message".into(),
                user_id: "local".into(),
                query: String::new(),
                model_id: Some("model".into()),
                project: GmClawProject {
                    id: "project".into(),
                    name: "synthetic".into(),
                    path: directory.path().to_string_lossy().into_owned(),
                },
                max_steps: 30,
                confirmation: Some(GmClawConfirmationDecision {
                    action: GmClawConfirmationAction::Approve,
                    call_ids: vec!["tool".into()],
                    run: Some(GmClawRunIdentity {
                        run_id: "run-main".into(),
                        chat_id: "chat".into(),
                        after_seq: 3,
                    }),
                }),
            };
            let result = client.execute(&request, |_| async { Ok(()) }).await;
            assert_eq!(posts.load(Ordering::SeqCst), usize::from(!stale));
            if stale {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap().answer, "resumed");
            }
            server.abort();
        }
    }
}
