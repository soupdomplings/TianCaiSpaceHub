//! One same-origin `/v1` attempt after an explicit HTTP failure. A successful
//! response is validated before its URL is saved; streams commit at termination.

use std::{pin::Pin, sync::Weak};

use axum::body::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::Value;

use super::{
    config::{ProviderConfig, ProviderType, provider_base_url_with_v1, provider_endpoint},
    context::GatewayContext,
};
use crate::{app_state::AppState, config::AppConfig};

#[cfg(test)]
mod tests;

const MAX_VALIDATION_BYTES: usize = 32 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 1024 * 1024;
type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

#[derive(Clone, Copy)]
enum Wire {
    Responses,
    Chat,
    Messages,
    Other,
}

pub(super) struct Attempt {
    pub request: reqwest::Request,
    corrected_base: String,
    wire: Wire,
}

// Finish reading a small original HTTP error before spending time on the
// alternate URL. Otherwise its request timeout could expire while it is held.
pub(super) async fn retain_error(
    response: reqwest::Response,
    deadline: tokio::time::Instant,
) -> (reqwest::Response, bool) {
    let response: axum::http::Response<reqwest::Body> = response.into();
    let (parts, body) = response.into_parts();
    let mut stream: ByteStream =
        Box::pin(reqwest::Response::from(axum::http::Response::new(body)).bytes_stream());
    let mut prefix = Vec::new();
    let can_retry = loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                prefix.extend_from_slice(&chunk);
                if prefix.len() > MAX_EVENT_BYTES {
                    break false;
                }
            }
            Ok(Some(Err(error))) => {
                stream = Box::pin(futures_util::stream::once(async { Err(error) }).chain(stream));
                break false;
            }
            Ok(None) => break true,
            Err(_) => break false,
        }
    };
    let stream = futures_util::stream::once(async { Ok(Bytes::from(prefix)) }).chain(stream);
    (
        reqwest::Response::from(axum::http::Response::from_parts(
            parts,
            reqwest::Body::wrap_stream(stream),
        )),
        can_retry,
    )
}

pub(super) fn prepare(provider: &ProviderConfig, request: &reqwest::Request) -> Option<Attempt> {
    if provider.provider_type == ProviderType::ChatGptResponses
        || provider.tiangong_builtin.is_some()
        || request.headers().contains_key("upgrade")
    {
        return None;
    }
    let corrected_base = provider_base_url_with_v1(&provider.base_url)?;
    let base = url::Url::parse(&provider.base_url).ok()?;
    if request.url().origin() != base.origin() {
        return None;
    }
    let suffix = request
        .url()
        .path()
        .strip_prefix(base.path().trim_end_matches('/'))?;
    let wire = match suffix {
        "/responses" | "/responses/compact" => Wire::Responses,
        "/chat/completions" => Wire::Chat,
        "/messages" => Wire::Messages,
        "/images/generations" | "/images/edits" | "/alpha/search" => Wire::Other,
        _ => return None,
    };
    let mut retry = request.try_clone()?;
    let mut url = url::Url::parse(&provider_endpoint(&corrected_base, suffix)).ok()?;
    url.set_query(request.url().query());
    *retry.url_mut() = url;
    Some(Attempt {
        request: retry,
        corrected_base,
        wire,
    })
}

#[derive(Clone)]
struct Commit {
    state: Option<Weak<AppState>>,
    provider: ProviderConfig,
    corrected_base: String,
}

impl Commit {
    async fn save(&self) {
        let Some(state) = self.state.as_ref().and_then(Weak::upgrade) else {
            return;
        };
        let mut current = state.config.lock().await;
        let result = save_correction(&state.config_path, &self.provider, &self.corrected_base);
        match result {
            Ok(Some(mut updated)) => {
                crate::normalize_config_paths(&mut updated, &state.config_path);
                *current = updated;
                tracing::info!(provider = %self.provider.name, "confirmed API base URL corrected with /v1");
            }
            Ok(None) => {}
            Err(_) => {
                tracing::warn!(provider = %self.provider.name, "API base URL worked with /v1 but correction could not be saved; configuration retained")
            }
        }
    }
}

// Reload under the in-process lock, then use AppConfig's file lock and revision
// check. A late response cannot replace a deleted, edited or renamed channel.
fn save_correction(
    path: &std::path::PathBuf,
    expected: &ProviderConfig,
    corrected_base: &str,
) -> anyhow::Result<Option<AppConfig>> {
    let mut latest = AppConfig::load_or_default(path)?;
    let matches: Vec<usize> = latest
        .ai_gateway
        .providers
        .iter()
        .enumerate()
        .filter(|(_, p)| p.name == expected.name)
        .map(|(i, _)| i)
        .collect();
    let [index] = matches.as_slice() else {
        return Ok(None);
    };
    if serde_json::to_value(&latest.ai_gateway.providers[*index])?
        != serde_json::to_value(expected)?
    {
        return Ok(None);
    }
    if !latest.ai_gateway.providers[*index].confirm_base_url(corrected_base) {
        return Ok(None);
    }
    latest.save(path)?;
    Ok(Some(latest))
}

pub(super) async fn try_once(
    client: &reqwest::Client,
    attempt: Attempt,
    provider: &ProviderConfig,
    context: Option<&GatewayContext>,
    deadline: tokio::time::Instant,
) -> Option<reqwest::Response> {
    let expected_url = attempt.request.url().clone();
    let commit = Commit {
        state: context.and_then(|ctx| ctx.config_state.clone()),
        provider: provider.clone(),
        corrected_base: attempt.corrected_base,
    };
    tokio::time::timeout_at(deadline, async {
        let response = client.execute(attempt.request).await.ok()?;
        if !response.status().is_success() || response.url() != &expected_url {
            return None;
        }
        let response: axum::http::Response<reqwest::Body> = response.into();
        let (parts, body) = response.into_parts();
        let mut stream: ByteStream =
            Box::pin(reqwest::Response::from(axum::http::Response::new(body)).bytes_stream());
        let sse_hint = parts
            .headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        let mut prefix = Vec::new();
        let is_sse = loop {
            let chunk = stream.next().await?.ok()?;
            if prefix.len() + chunk.len() > MAX_VALIDATION_BYTES {
                return None;
            }
            prefix.extend_from_slice(&chunk);
            let sniff = prefix.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&prefix);
            let sniff = sniff
                .iter()
                .position(|b| !b.is_ascii_whitespace())
                .map(|i| &sniff[i..])
                .unwrap_or(&[]);
            if matches!(sniff.first(), Some(b'{' | b'[')) {
                break false;
            }
            if sniff.starts_with(b"data:")
                || sniff.starts_with(b"event:")
                || sniff.starts_with(b":")
            {
                break true;
            }
            if sniff.len() >= 16 {
                return None;
            }
            if sse_hint && !sniff.is_empty() && sniff.contains(&b'\n') {
                return None;
            }
        };
        if !is_sse {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.ok()?;
                if prefix.len() + chunk.len() > MAX_VALIDATION_BYTES {
                    return None;
                }
                prefix.extend_from_slice(&chunk);
            }
            let value: Value = serde_json::from_slice(&prefix).ok()?;
            if !valid_json(&value, attempt.wire) {
                return None;
            }
            commit.save().await;
            return Some(reqwest::Response::from(axum::http::Response::from_parts(
                parts,
                reqwest::Body::from(prefix),
            )));
        }
        let mut evidence = Evidence::new(attempt.wire);
        evidence.push(&prefix);
        // Do not accept HTML, an SSE error, comments-only output or a malformed
        // first event as evidence that the alternate endpoint works.
        while !evidence.started && !evidence.failed {
            let chunk = stream.next().await?.ok()?;
            if prefix.len() + chunk.len() > MAX_EVENT_BYTES {
                return None;
            }
            prefix.extend_from_slice(&chunk);
            evidence.push(&chunk);
        }
        if evidence.failed {
            return None;
        }
        let stream = futures_util::stream::unfold(
            (Some(Bytes::from(prefix)), stream, evidence, Some(commit)),
            |(mut prefix, mut stream, mut evidence, mut commit)| async move {
                let item = if let Some(prefix) = prefix.take() {
                    Ok(prefix)
                } else {
                    let item = stream.next().await?;
                    if let Ok(bytes) = &item {
                        evidence.push(bytes);
                    } else {
                        evidence.failed = true;
                    }
                    item
                };
                if evidence.terminal && !evidence.failed {
                    if let Some(commit) = commit.take() {
                        commit.save().await;
                    }
                }
                Some((item, (prefix, stream, evidence, commit)))
            },
        );
        Some(reqwest::Response::from(axum::http::Response::from_parts(
            parts,
            reqwest::Body::wrap_stream(stream),
        )))
    })
    .await
    .ok()
    .flatten()
}

fn valid_json(value: &Value, wire: Wire) -> bool {
    if value.get("error").is_some_and(|v| !v.is_null())
        || matches!(
            value.get("status").and_then(Value::as_str),
            Some("failed" | "cancelled")
        )
    {
        return false;
    }
    match wire {
        Wire::Responses => value.get("output").is_some_and(Value::is_array),
        Wire::Chat => value
            .get("choices")
            .and_then(Value::as_array)
            .is_some_and(|v| !v.is_empty()),
        Wire::Messages => {
            value.get("type").and_then(Value::as_str) == Some("message")
                && value.get("content").is_some_and(Value::is_array)
        }
        Wire::Other => {
            value.is_object()
                && ["data", "results", "output"]
                    .iter()
                    .any(|k| value.get(k).is_some_and(Value::is_array))
        }
    }
}

struct Evidence {
    wire: Wire,
    pending: Vec<u8>,
    data: Vec<String>,
    event_name: String,
    event_bytes: usize,
    received: usize,
    started: bool,
    stopped: bool,
    terminal: bool,
    failed: bool,
}

impl Evidence {
    fn new(wire: Wire) -> Self {
        Self {
            wire,
            pending: Vec::new(),
            data: Vec::new(),
            event_name: String::new(),
            event_bytes: 0,
            received: 0,
            started: false,
            stopped: false,
            terminal: false,
            failed: false,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        if self.failed {
            return;
        }
        self.received = self.received.saturating_add(bytes.len());
        if self.received > MAX_VALIDATION_BYTES {
            self.failed = true;
            return;
        }
        self.pending.extend_from_slice(bytes);
        while let Some(index) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.pending.drain(..=index).collect();
            let Ok(line) = std::str::from_utf8(&line) else {
                self.failed = true;
                return;
            };
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                self.event();
                self.data.clear();
                self.event_name.clear();
                self.event_bytes = 0;
            } else if let Some(name) = line.strip_prefix("event:") {
                self.event_name = name.trim().to_string();
            } else if let Some(data) = line.strip_prefix("data:") {
                self.event_bytes += data.len();
                if self.event_bytes > MAX_EVENT_BYTES {
                    self.failed = true;
                    return;
                }
                self.data
                    .push(data.strip_prefix(' ').unwrap_or(data).to_string());
            }
            if self.failed {
                return;
            }
        }
        if self.pending.len() > MAX_EVENT_BYTES {
            self.failed = true;
        }
    }

    fn event(&mut self) {
        if self.event_name == "error" {
            self.failed = true;
            return;
        }
        if self.data.is_empty() {
            return;
        }
        let data = self.data.join("\n");
        if data == "[DONE]" {
            if matches!(self.wire, Wire::Chat) && self.stopped {
                self.terminal = true;
            }
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            self.failed = true;
            return;
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        if value.get("error").is_some_and(|v| !v.is_null())
            || matches!(kind, "error" | "response.failed" | "response.cancelled")
        {
            self.failed = true;
            return;
        }
        match self.wire {
            Wire::Responses => {
                self.started |= kind.starts_with("response.");
                if kind == "response.completed" {
                    self.terminal = value
                        .get("response")
                        .is_some_and(|v| valid_json(v, self.wire))
                        && self.started;
                }
            }
            Wire::Chat => {
                if let Some(choices) = value.get("choices").and_then(Value::as_array) {
                    self.started |= choices.iter().any(|c| c.get("delta").is_some());
                    self.stopped |= choices
                        .iter()
                        .any(|c| c.get("finish_reason").is_some_and(|v| !v.is_null()));
                }
            }
            Wire::Messages => {
                self.started |= kind == "message_start"
                    && value
                        .get("message")
                        .is_some_and(|v| valid_json(v, self.wire));
                self.stopped |= kind == "message_delta"
                    && value
                        .pointer("/delta/stop_reason")
                        .is_some_and(|v| !v.is_null());
                self.terminal |= kind == "message_stop" && self.started && self.stopped;
            }
            Wire::Other => {
                self.failed = true;
            }
        }
    }
}
