//! Bounded JSON/SSE collection for GMClaw's non-streaming Chat client.
//! Collection never retries after response headers or partial output arrive.

use std::{collections::BTreeMap, fmt::Display, time::Duration};

use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
};
use futures_util::{Stream, StreamExt};
use serde_json::{Map, Value, json};
use tokio::time::Instant;

use super::error::GatewayError;

const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
const MAX_ITEMS: usize = 4096;

#[derive(Clone, Copy)]
pub(super) enum Protocol {
    Responses,
    Chat,
}

pub(super) fn deadline(timeout_secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(timeout_secs.max(1))
}

pub(super) async fn collect<S, E>(
    stream: S,
    headers: &HeaderMap,
    protocol: Protocol,
    provider: &str,
    deadline: Instant,
) -> Result<Value, GatewayError>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: Display,
{
    let sse_hint = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("text/event-stream")
        });
    let future = async {
        futures_util::pin_mut!(stream);
        let mut decoder = Decoder::new(protocol, sse_hint);
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|_| failure(provider, "upstream response body disconnected"))?;
            if let Some(value) = decoder.push(&chunk, provider)? {
                return Ok(value);
            }
        }
        decoder.finish(provider)
    };
    tokio::time::timeout_at(deadline, future)
        .await
        .map_err(|_| GatewayError::upstream_timeout())?
}

fn failure(provider: &str, message: &str) -> GatewayError {
    GatewayError::upstream_provider(StatusCode::BAD_GATEWAY, provider, message, None, None)
}

fn check_error(value: &Value, provider: &str) -> Result<(), GatewayError> {
    if value.get("error").is_some_and(|v| !v.is_null()) {
        return Err(GatewayError::from_upstream_body(
            StatusCode::BAD_GATEWAY,
            provider,
            &value.to_string(),
        ));
    }
    Ok(())
}

enum Mode {
    Unknown,
    Json,
    Sse,
}

struct Decoder {
    mode: Mode,
    sse_hint: bool,
    received: usize,
    pending: Vec<u8>,
    event_name: String,
    event_data: Vec<String>,
    accumulator: Accumulator,
    terminal: Option<Value>,
}

impl Decoder {
    fn new(protocol: Protocol, sse_hint: bool) -> Self {
        Self {
            mode: Mode::Unknown,
            sse_hint,
            received: 0,
            pending: Vec::new(),
            event_name: String::new(),
            event_data: Vec::new(),
            accumulator: match protocol {
                Protocol::Responses => Accumulator::Responses(Responses::default()),
                Protocol::Chat => Accumulator::Chat(Chat::default()),
            },
            terminal: None,
        }
    }

    fn push(&mut self, bytes: &[u8], provider: &str) -> Result<Option<Value>, GatewayError> {
        if bytes.len() > MAX_BODY_BYTES.saturating_sub(self.received) {
            return Err(failure(
                provider,
                "upstream response exceeds the 32 MiB collection limit",
            ));
        }
        self.received += bytes.len();
        self.pending.extend_from_slice(bytes);
        if matches!(self.mode, Mode::Unknown) {
            let sniff = self
                .pending
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(&self.pending);
            let sniff = sniff
                .iter()
                .position(|v| !v.is_ascii_whitespace())
                .map(|i| &sniff[i..])
                .unwrap_or(&[]);
            if matches!(sniff.first().copied(), Some(b'{' | b'[')) {
                self.mode = Mode::Json;
            } else if sniff.starts_with(b"data:")
                || sniff.starts_with(b"event:")
                || sniff.starts_with(b":")
                || sniff.starts_with(b"id:")
                || sniff.starts_with(b"retry:")
            {
                self.mode = Mode::Sse;
            } else if sniff.len() >= 16 {
                return Err(failure(
                    provider,
                    if self.sse_hint {
                        "invalid upstream SSE response"
                    } else {
                        "upstream body is neither JSON nor SSE"
                    },
                ));
            }
        }
        if matches!(self.mode, Mode::Sse) {
            let mut consumed = 0;
            while let Some(position) = self.pending[consumed..].iter().position(|b| *b == b'\n') {
                let end = consumed + position;
                let line = std::str::from_utf8(&self.pending[consumed..end])
                    .map_err(|_| failure(provider, "invalid UTF-8 in upstream SSE"))?
                    .trim_end_matches('\r')
                    .trim_start_matches('\u{feff}')
                    .to_string();
                consumed = end + 1;
                self.line(&line, provider)?;
            }
            self.pending.drain(..consumed);
        }
        Ok(self.terminal.take())
    }

    fn line(&mut self, line: &str, provider: &str) -> Result<(), GatewayError> {
        if line.is_empty() {
            if !self.event_data.is_empty() {
                let data = self.event_data.join("\n");
                self.event_data.clear();
                self.terminal = self
                    .accumulator
                    .event(&self.event_name, data.trim(), provider)?
                    .or(self.terminal.take());
            }
            self.event_name.clear();
        } else if let Some(data) = line.strip_prefix("data:") {
            self.event_data
                .push(data.strip_prefix(' ').unwrap_or(data).to_string());
        } else if let Some(event) = line.strip_prefix("event:") {
            self.event_name = event.trim().to_string();
        }
        Ok(())
    }

    fn finish(mut self, provider: &str) -> Result<Value, GatewayError> {
        if matches!(self.mode, Mode::Sse) {
            // An unterminated frame is a truncated stream, even if it contains valid JSON.
            if !self.pending.iter().all(u8::is_ascii_whitespace) || !self.event_data.is_empty() {
                return Err(failure(provider, "upstream SSE ended inside an event"));
            }
            return self.accumulator.finish(provider);
        }
        let bytes = self
            .pending
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(&self.pending);
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|_| failure(provider, "invalid upstream JSON response"))?;
        check_error(&value, provider)?;
        match self.accumulator {
            Accumulator::Responses(_) => validate_responses(&value, provider)?,
            Accumulator::Chat(_) => super::gmclaw::validate_chat_response(&value, provider)?,
        }
        Ok(value)
    }
}

enum Accumulator {
    Responses(Responses),
    Chat(Chat),
}

impl Accumulator {
    fn event(
        &mut self,
        name: &str,
        data: &str,
        provider: &str,
    ) -> Result<Option<Value>, GatewayError> {
        if data == "[DONE]" {
            return self.finish(provider).map(Some);
        }
        let value: Value = serde_json::from_str(data)
            .map_err(|_| failure(provider, "invalid JSON in upstream SSE event"))?;
        check_error(&value, provider)?;
        if name == "error" || value.get("type").and_then(Value::as_str) == Some("error") {
            return Err(GatewayError::from_upstream_body(
                StatusCode::BAD_GATEWAY,
                provider,
                &value.to_string(),
            ));
        }
        match self {
            Self::Responses(state) => state.event(name, value, provider),
            Self::Chat(state) => {
                state.event(value, provider)?;
                Ok(None)
            }
        }
    }

    fn finish(&mut self, provider: &str) -> Result<Value, GatewayError> {
        match self {
            Self::Responses(state) => state.completed.take().ok_or_else(|| {
                failure(
                    provider,
                    "upstream Responses stream ended without a terminal response",
                )
            }),
            Self::Chat(state) => state.finish(provider),
        }
    }
}

fn validate_responses(value: &Value, provider: &str) -> Result<(), GatewayError> {
    check_error(value, provider)?;
    if !matches!(
        value.get("status").and_then(Value::as_str),
        Some("completed" | "incomplete")
    ) || !value.get("output").is_some_and(Value::is_array)
    {
        return Err(failure(
            provider,
            "upstream Responses did not return a complete terminal object",
        ));
    }
    Ok(())
}

#[derive(Default)]
struct Responses {
    metadata: Map<String, Value>,
    items: BTreeMap<u64, (Value, bool)>,
    completed: Option<Value>,
}

impl Responses {
    fn event(
        &mut self,
        name: &str,
        event: Value,
        provider: &str,
    ) -> Result<Option<Value>, GatewayError> {
        let kind = event.get("type").and_then(Value::as_str).unwrap_or(name);
        if matches!(kind, "response.failed" | "response.cancelled") {
            let value = event.get("response").unwrap_or(&event);
            check_error(value, provider)?;
            return Err(failure(
                provider,
                "upstream Responses generation failed or was cancelled",
            ));
        }
        if matches!(kind, "response.created" | "response.in_progress") {
            if let Some(response) = event.get("response").and_then(Value::as_object) {
                self.metadata.extend(response.clone());
            }
        } else if matches!(
            kind,
            "response.completed" | "response.incomplete" | "response.done"
        ) {
            let mut response = event.get("response").cloned().ok_or_else(|| {
                failure(provider, "terminal Responses event is missing its response")
            })?;
            check_error(&response, provider)?;
            let response_object = response
                .as_object_mut()
                .ok_or_else(|| failure(provider, "invalid terminal Responses object"))?;
            for (key, value) in &self.metadata {
                if key != "output" && key != "status" {
                    response_object
                        .entry(key.clone())
                        .or_insert_with(|| value.clone());
                }
            }
            if response_object.get("status").is_none() && kind != "response.done" {
                response_object.insert(
                    "status".into(),
                    json!(if kind == "response.incomplete" {
                        "incomplete"
                    } else {
                        "completed"
                    }),
                );
            }
            let missing_output = response_object
                .get("output")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty);
            if missing_output && !self.items.is_empty() {
                if self.items.values().any(|(_, done)| !done) {
                    return Err(failure(
                        provider,
                        "terminal Responses event omitted unfinished output items",
                    ));
                }
                response_object.insert(
                    "output".into(),
                    Value::Array(self.items.values().map(|(item, _)| item.clone()).collect()),
                );
            }
            validate_responses(&response, provider)?;
            self.completed = Some(response.clone());
            return Ok(Some(response));
        } else if matches!(
            kind,
            "response.output_item.added" | "response.output_item.done"
        ) {
            let index = event
                .get("output_index")
                .and_then(Value::as_u64)
                .ok_or_else(|| failure(provider, "Responses output item is missing its index"))?;
            let item = event
                .get("item")
                .filter(|v| v.is_object())
                .cloned()
                .ok_or_else(|| failure(provider, "Responses output item is missing"))?;
            if self.items.len() >= MAX_ITEMS && !self.items.contains_key(&index) {
                return Err(failure(provider, "too many Responses output items"));
            }
            self.items.insert(index, (item, kind.ends_with(".done")));
        } else if kind.ends_with(".delta") || kind.ends_with(".done") || kind.ends_with(".added") {
            self.item_event(kind, &event, provider)?;
        }
        Ok(None)
    }

    fn item_event(
        &mut self,
        kind: &str,
        event: &Value,
        provider: &str,
    ) -> Result<(), GatewayError> {
        let Some(index) = event
            .get("output_index")
            .and_then(Value::as_u64)
            .or_else(|| {
                let id = event.get("item_id")?.as_str()?;
                self.items
                    .iter()
                    .find(|(_, (item, _))| item.get("id").and_then(Value::as_str) == Some(id))
                    .map(|(index, _)| *index)
            })
        else {
            return Ok(());
        };
        let Some((item, _)) = self.items.get_mut(&index) else {
            return Ok(());
        };
        match kind {
            "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
                let field = if kind.contains("arguments") {
                    "arguments"
                } else {
                    "input"
                };
                append_string(item, field, event.get("delta"), provider)?;
            }
            "response.function_call_arguments.done" | "response.custom_tool_call_input.done" => {
                let field = if kind.contains("arguments") {
                    "arguments"
                } else {
                    "input"
                };
                if let Some(value) = event.get(field) {
                    item[field] = value.clone();
                }
            }
            "response.content_part.added"
            | "response.content_part.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done" => {
                let (field, index_field) = if kind.contains("summary") {
                    ("summary", "summary_index")
                } else {
                    ("content", "content_index")
                };
                if let Some(part) = event.get("part") {
                    *part_mut(item, field, event, index_field, provider)? = part.clone();
                }
            }
            "response.output_text.delta"
            | "response.reasoning_text.delta"
            | "response.reasoning_summary_text.delta"
            | "response.refusal.delta" => {
                let (field, index_field) = if kind.contains("summary") {
                    ("summary", "summary_index")
                } else {
                    ("content", "content_index")
                };
                let text_field = if kind.contains("refusal") {
                    "refusal"
                } else {
                    "text"
                };
                let part = part_mut(item, field, event, index_field, provider)?;
                append_string(part, text_field, event.get("delta"), provider)?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn part_mut<'a>(
    item: &'a mut Value,
    field: &str,
    event: &Value,
    index_field: &str,
    provider: &str,
) -> Result<&'a mut Value, GatewayError> {
    let index = event.get(index_field).and_then(Value::as_u64).unwrap_or(0) as usize;
    if item.get(field).is_none() {
        item[field] = json!([]);
    }
    let parts = item
        .get_mut(field)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| failure(provider, "invalid Responses content parts"))?;
    if index > parts.len() || index >= MAX_ITEMS {
        return Err(failure(provider, "non-contiguous Responses content index"));
    }
    if index == parts.len() {
        parts.push(json!({}));
    }
    Ok(&mut parts[index])
}

fn append_string(
    target: &mut Value,
    field: &str,
    delta: Option<&Value>,
    provider: &str,
) -> Result<(), GatewayError> {
    let Some(delta) = delta.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let delta = delta
        .as_str()
        .ok_or_else(|| failure(provider, "invalid text delta in upstream SSE"))?;
    let object = target
        .as_object_mut()
        .ok_or_else(|| failure(provider, "invalid upstream delta target"))?;
    let value = object
        .entry(field)
        .or_insert_with(|| Value::String(String::new()));
    if value.is_null() {
        *value = Value::String(String::new());
    }
    let Value::String(text) = value else {
        return Err(failure(provider, "invalid upstream text state"));
    };
    text.push_str(delta);
    Ok(())
}

#[derive(Default)]
struct Chat {
    metadata: Map<String, Value>,
    choices: BTreeMap<u64, ChatChoice>,
}

struct ChatChoice {
    message: Value,
    tools: BTreeMap<u64, Value>,
    finish_reason: Option<Value>,
    logprobs: Option<Value>,
}

impl Default for ChatChoice {
    fn default() -> Self {
        Self {
            message: json!({"role":"assistant","content":null}),
            tools: BTreeMap::new(),
            finish_reason: None,
            logprobs: None,
        }
    }
}

impl Chat {
    fn event(&mut self, event: Value, provider: &str) -> Result<(), GatewayError> {
        let object = event
            .as_object()
            .ok_or_else(|| failure(provider, "invalid Chat SSE object"))?;
        for (field, value) in object {
            if field != "choices" && !value.is_null() {
                self.metadata.insert(field.clone(), value.clone());
            }
        }
        let choices = object
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| failure(provider, "Chat SSE event is missing choices"))?;
        for choice in choices {
            let index = choice.get("index").and_then(Value::as_u64).unwrap_or(0);
            if self.choices.len() >= MAX_ITEMS && !self.choices.contains_key(&index) {
                return Err(failure(provider, "too many Chat choices"));
            }
            let target = self.choices.entry(index).or_default();
            if let Some(message) = choice.get("message").filter(|v| v.is_object()) {
                target.message = message.clone();
                target.tools.clear();
            }
            if let Some(delta) = choice.get("delta").and_then(Value::as_object) {
                for (field, value) in delta {
                    if value.is_null() {
                        continue;
                    }
                    match field.as_str() {
                        "tool_calls" => target.tool_deltas(value, provider)?,
                        "content" | "refusal" | "reasoning_content" | "reasoning" => {
                            append_string(&mut target.message, field, Some(value), provider)?
                        }
                        "reasoning_details" => {
                            merge_reasoning_details(&mut target.message, value, provider)?
                        }
                        _ => target.message[field] = value.clone(),
                    }
                }
            }
            if let Some(reason) = choice.get("finish_reason").filter(|v| !v.is_null()) {
                target.finish_reason = Some(reason.clone());
            }
            if let Some(logprobs) = choice.get("logprobs").filter(|v| !v.is_null()) {
                target.logprobs = Some(logprobs.clone());
            }
        }
        Ok(())
    }

    fn finish(&mut self, provider: &str) -> Result<Value, GatewayError> {
        if self.choices.is_empty()
            || self.choices.values().any(|choice| {
                choice
                    .finish_reason
                    .as_ref()
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            })
        {
            return Err(failure(
                provider,
                "upstream Chat stream ended without finish_reason",
            ));
        }
        let mut choices = Vec::new();
        for (index, choice) in &mut self.choices {
            if !choice.tools.is_empty() {
                choice.message["tool_calls"] =
                    Value::Array(choice.tools.values().cloned().collect());
            }
            if let Some(tools) = choice.message.get("tool_calls").and_then(Value::as_array) {
                for tool in tools {
                    if tool
                        .get("id")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
                        || tool
                            .pointer("/function/name")
                            .and_then(Value::as_str)
                            .is_none_or(str::is_empty)
                        || tool
                            .pointer("/function/arguments")
                            .and_then(Value::as_str)
                            .and_then(|s| serde_json::from_str::<Value>(s).ok())
                            .is_none()
                    {
                        return Err(failure(
                            provider,
                            "upstream Chat stream returned an incomplete tool call",
                        ));
                    }
                }
            }
            let mut result = json!({"index": index, "message": choice.message, "finish_reason": choice.finish_reason});
            if let Some(logprobs) = &choice.logprobs {
                result["logprobs"] = logprobs.clone();
            }
            choices.push(result);
        }
        let mut response = Value::Object(self.metadata.clone());
        response["object"] = json!("chat.completion");
        response["choices"] = Value::Array(choices);
        super::gmclaw::validate_chat_response(&response, provider)?;
        Ok(response)
    }
}

impl ChatChoice {
    fn tool_deltas(&mut self, value: &Value, provider: &str) -> Result<(), GatewayError> {
        let tools = value
            .as_array()
            .ok_or_else(|| failure(provider, "invalid streamed tool calls"))?;
        for (position, delta) in tools.iter().enumerate() {
            let index = delta
                .get("index")
                .and_then(Value::as_u64)
                .unwrap_or(position as u64);
            if self.tools.len() >= MAX_ITEMS && !self.tools.contains_key(&index) {
                return Err(failure(provider, "too many streamed tool calls"));
            }
            let tool = self.tools.entry(index).or_insert_with(
                || json!({"type":"function","function":{"name":"","arguments":""}}),
            );
            for field in ["id", "type"] {
                if let Some(value) = delta.get(field).filter(|v| !v.is_null()) {
                    if tool.get(field).is_some_and(|old| old != value) && field == "id" {
                        return Err(failure(provider, "streamed tool call changed identity"));
                    }
                    tool[field] = value.clone();
                }
            }
            if let Some(function) = delta.get("function") {
                if let Some(name) = function.get("name") {
                    if tool["function"].get("name") != Some(name) {
                        append_string(&mut tool["function"], "name", Some(name), provider)?;
                    }
                }
                append_string(
                    &mut tool["function"],
                    "arguments",
                    function.get("arguments"),
                    provider,
                )?;
            }
        }
        Ok(())
    }
}

fn merge_reasoning_details(
    message: &mut Value,
    value: &Value,
    provider: &str,
) -> Result<(), GatewayError> {
    let details = value
        .as_array()
        .ok_or_else(|| failure(provider, "invalid reasoning_details delta"))?;
    if message.get("reasoning_details").is_none() {
        message["reasoning_details"] = json!([]);
    }
    let existing = message["reasoning_details"]
        .as_array_mut()
        .ok_or_else(|| failure(provider, "invalid reasoning_details state"))?;
    for detail in details {
        let found = existing.iter().position(|old| {
            detail
                .get("index")
                .is_some_and(|index| old.get("index") == Some(index))
                || detail.get("id").is_some_and(|id| old.get("id") == Some(id))
        });
        if let Some(index) = found {
            let fields = detail
                .as_object()
                .ok_or_else(|| failure(provider, "invalid reasoning detail"))?;
            for (field, value) in fields {
                if matches!(field.as_str(), "text" | "data" | "signature") {
                    append_string(&mut existing[index], field, Some(value), provider)?;
                } else {
                    existing[index][field] = value.clone();
                }
            }
        } else {
            if existing.len() >= MAX_ITEMS {
                return Err(failure(provider, "too many reasoning details"));
            }
            existing.push(detail.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(value: Value) -> String {
        format!("data: {value}\r\n\r\n")
    }
    fn decode(text: &str, protocol: Protocol, chunk_size: usize) -> Result<Value, GatewayError> {
        let mut decoder = Decoder::new(protocol, false);
        for chunk in text.as_bytes().chunks(chunk_size) {
            if let Some(value) = decoder.push(chunk, "gmclaw")? {
                return Ok(value);
            }
        }
        decoder.finish("gmclaw")
    }

    #[test]
    fn responses_terminal_snapshot_preserves_text_tools_encryption_and_usage() {
        let response = json!({"id":"resp_1","status":"completed","output":[
            {"id":"rs_1","type":"reasoning","encrypted_content":"opaque-signature","summary":[{"type":"summary_text","text":"思考"}]},
            {"id":"msg_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"你好"}]},
            {"id":"fc_1","type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"q\":\"中文\"}"}
        ],"usage":{"input_tokens":20,"output_tokens":7,"output_tokens_details":{"reasoning_tokens":3}}});
        let wire = format!(
            ": heartbeat\r\n\r\n{}",
            event(json!({"type":"response.completed","response":response}))
        );
        for chunk_size in [1, 7, 100000] {
            assert_eq!(
                decode(&wire, Protocol::Responses, chunk_size).unwrap(),
                response
            );
        }
    }

    #[test]
    fn responses_can_recover_complete_items_from_sparse_terminal() {
        let item = json!({"id":"fc_1","type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"});
        let wire = [
            event(json!({"type":"response.output_item.added","output_index":0,"item":item})),
            event(json!({"type":"response.output_item.done","output_index":0,"item":item})),
            event(json!({"type":"response.completed","response":{"status":"completed","output":[],"usage":{"output_tokens":2}}})),
        ].join("");
        let result = decode(&wire, Protocol::Responses, 3).unwrap();
        assert_eq!(result["output"], json!([item]));
        assert_eq!(result["usage"]["output_tokens"], 2);
    }

    #[test]
    fn chat_assembles_interleaved_tools_reasoning_and_final_usage() {
        let wire = [
            event(json!({"id":"chat_1","model":"model","choices":[{"index":0,"delta":{"content":"你好","reasoning_content":"think ","reasoning_details":[{"index":0,"type":"reasoning.encrypted","data":"sig-"}],"tool_calls":[{"index":1,"id":"call_b","function":{"name":"second","arguments":"{\"b\":"}},{"index":0,"id":"call_a","function":{"name":"first","arguments":"{\"a\":"}}]},"finish_reason":null}]})),
            event(json!({"choices":[{"index":0,"delta":{"reasoning_content":"more","reasoning_details":[{"index":0,"data":"end"}],"tool_calls":[{"index":0,"function":{"arguments":"1}"}},{"index":1,"function":{"arguments":"2}"}}]},"finish_reason":"tool_calls"}]})),
            event(json!({"choices":[],"usage":{"prompt_tokens":9,"completion_tokens":8,"completion_tokens_details":{"reasoning_tokens":4}}})),
            "data: [DONE]\r\n\r\n".into(),
        ].join("");
        let result = decode(&wire, Protocol::Chat, 1).unwrap();
        let message = &result["choices"][0]["message"];
        assert_eq!(message["content"], "你好");
        assert_eq!(message["reasoning_content"], "think more");
        assert_eq!(message["reasoning_details"][0]["data"], "sig-end");
        assert_eq!(message["tool_calls"][0]["id"], "call_a");
        assert_eq!(
            message["tool_calls"][0]["function"]["arguments"],
            "{\"a\":1}"
        );
        assert_eq!(
            message["tool_calls"][1]["function"]["arguments"],
            "{\"b\":2}"
        );
        assert_eq!(
            result["usage"]["completion_tokens_details"]["reasoning_tokens"],
            4
        );
        assert_eq!(result["object"], "chat.completion");
    }

    #[test]
    fn partial_error_failed_and_unfinished_tools_are_not_success() {
        for wire in [
            event(json!({"type":"response.output_text.delta","delta":"partial","output_index":0})),
            "data: [DONE]\n\n".into(),
            event(json!({"type":"response.failed","response":{"status":"failed","error":{"message":"failed","code":"failed_generation"}}})),
            event(json!({"type":"error","message":"failure"})),
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}".into(),
        ] { assert!(decode(&wire, Protocol::Responses, 4).is_err()); }
        assert!(
            decode(
                &event(json!({"choices":[{"index":0,"delta":{"content":"partial"}}]})),
                Protocol::Chat,
                7
            )
            .is_err()
        );
        assert!(decode(&format!("{}data: [DONE]\n\n", event(json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"x","function":{"name":"f","arguments":"{"}}]},"finish_reason":"tool_calls"}]}))), Protocol::Chat, 5).is_err());
    }

    #[tokio::test]
    async fn total_deadline_and_size_cap_bound_collection() {
        let error = collect(
            futures_util::stream::pending::<Result<Bytes, std::io::Error>>(),
            &HeaderMap::new(),
            Protocol::Responses,
            "gmclaw",
            Instant::now(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, StatusCode::GATEWAY_TIMEOUT);
        let mut decoder = Decoder::new(Protocol::Responses, true);
        decoder.received = MAX_BODY_BYTES;
        assert!(decoder.push(b"x", "gmclaw").is_err());
    }

    #[tokio::test]
    async fn content_type_mismatch_uses_actual_valid_payload_and_disconnect_errors() {
        let value = json!({"status":"completed","output":[]});
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "text/event-stream".parse().unwrap());
        let chunks =
            futures_util::stream::iter([Ok::<_, std::io::Error>(Bytes::from(value.to_string()))]);
        assert_eq!(
            collect(chunks, &headers, Protocol::Responses, "gmclaw", deadline(1))
                .await
                .unwrap(),
            value
        );
        let chunks = futures_util::stream::iter([
            Ok(Bytes::from(event(
                json!({"type":"response.created","response":{"id":"r"}}),
            ))),
            Err(std::io::Error::other("disconnect")),
        ]);
        assert!(
            collect(chunks, &headers, Protocol::Responses, "gmclaw", deadline(1))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn gmclaw_unary_providers_return_json_with_the_expected_upstream_stream_mode() {
        use super::super::{
            config::{ProviderConfig, ProviderType},
            context::GatewayContext,
            providers::openai_responses,
            workbuddy,
        };
        use axum::{Json, Router, routing::post};
        use std::sync::{Arc, Mutex};

        for responses in [true, false] {
            let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
            let captured_request = captured.clone();
            let wire = if responses {
                event(json!({"type":"response.completed","response":{
                    "id":"resp_fixture","status":"completed","output":[
                        {"id":"reason_fixture","type":"reasoning","encrypted_content":"opaque","summary":[]},
                        {"id":"msg_fixture","type":"message","role":"assistant","content":[{"type":"output_text","text":"collected"}]}
                    ],"usage":{"input_tokens":1,"output_tokens":2}
                }}))
            } else {
                format!(
                    "{}data: [DONE]\n\n",
                    event(
                        json!({"id":"chat_fixture","choices":[{"index":0,"delta":{"role":"assistant","content":"collected","reasoning_content":"kept"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":2}})
                    )
                )
            };
            let app = Router::new().route(
                if responses {
                    "/v1/responses"
                } else {
                    "/v1/chat/completions"
                },
                post(move |Json(request): Json<Value>| {
                    let captured = captured_request.clone();
                    let wire = wire.clone();
                    async move {
                        captured.lock().unwrap().push(request);
                        ([("content-type", "application/json")], wire)
                    }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let provider = ProviderConfig {
                name: "gmclaw".into(),
                base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
                provider_type: if responses {
                    ProviderType::OpenAiResponses
                } else {
                    ProviderType::ChatCompletions
                },
                api_key: "fixture-key".into(),
                timeout_secs: 5,
                ..ProviderConfig::default()
            };
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let ctx = GatewayContext::extract(&HeaderMap::new(), Some("fixture-session"));
            let request = json!({"model":"test-model","stream":false,"messages":[{"role":"user","content":"hello"}]});
            let response = if responses {
                openai_responses::passthrough(
                    &client,
                    &ctx,
                    workbuddy::chat_request_to_openai_responses(&request).unwrap(),
                    "test-model",
                    &provider,
                    None,
                )
                .await
            } else {
                workbuddy::proxy_chat_completion(
                    &client,
                    &ctx,
                    request,
                    "test-model",
                    "test-model",
                    &provider,
                    None,
                )
                .await
            };
            server.abort();
            let response = response.unwrap();
            assert_eq!(response.headers()["content-type"], "application/json");
            let body = axum::body::to_bytes(response.into_body(), MAX_BODY_BYTES)
                .await
                .unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(captured.lock().unwrap()[0]["stream"], responses);
            if responses {
                assert_eq!(value["output"][0]["encrypted_content"], "opaque");
                assert_eq!(value["usage"]["output_tokens"], 2);
            } else {
                assert_eq!(value["choices"][0]["message"]["reasoning_content"], "kept");
                assert_eq!(value["usage"]["completion_tokens"], 2);
            }
        }
    }
}
