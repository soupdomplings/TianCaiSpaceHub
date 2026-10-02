//! Process-local replay of provider state that the GMClaw Chat client drops.
//!
//! Only an exact, complete visible history prefix may recover opaque state.
//! Neither provider credentials nor cached reasoning are written to logs or disk.

use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{config::ProviderConfig, workbuddy};

const MAX_ENTRIES: usize = 128;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRY_BYTES: usize = 1024 * 1024;
const TTL: Duration = Duration::from_secs(60 * 60);
const CHAT_STATE_FIELDS: [&str; 2] = ["reasoning_content", "reasoning_details"];

type ReplayKey = [u8; 32];

static CACHE: OnceLock<Arc<Mutex<ReplayCache>>> = OnceLock::new();

/// Captures the original visible request before protocol conversion or replay.
pub struct ReplayContext {
    seed: Option<Sha256>,
    history: Option<Sha256>,
    cache: Arc<Mutex<ReplayCache>>,
}

impl ReplayContext {
    pub fn new(raw: &Value, provider: &ProviderConfig, upstream_model: &str) -> Self {
        let seed = if raw.get("stream").and_then(Value::as_bool) == Some(true) {
            None
        } else {
            provider_seed(provider, upstream_model)
        };
        let history = seed.clone().and_then(|mut hash| {
            for message in raw.get("messages")?.as_array()? {
                hash_message(&mut hash, message)?;
            }
            Some(hash)
        });
        Self {
            seed,
            history,
            cache: CACHE
                .get_or_init(|| Arc::new(Mutex::new(ReplayCache::default())))
                .clone(),
        }
    }

    /// Restores only known fields the client omitted; explicit values win.
    pub fn restore_chat(&self, raw: &mut Value) {
        let keys = self.prefix_keys(raw);
        let Some(messages) = raw.get_mut("messages").and_then(Value::as_array_mut) else {
            return;
        };
        for (message, key) in messages.iter_mut().zip(keys) {
            let Some(data) = key.and_then(|key| self.lookup(&key)) else {
                continue;
            };
            let Some(object) = message.as_object_mut() else {
                continue;
            };
            for (field, value) in data.chat_fields {
                if object.get(&field).is_none_or(Value::is_null) {
                    object.insert(field, value);
                }
            }
        }
    }

    /// Keeps full-request options and instructions, replacing only matched
    /// assistant turns with their exact original Responses output items.
    pub fn to_responses(&self, raw: &Value, openai: bool) -> Result<Value, String> {
        let convert = if openai {
            workbuddy::chat_request_to_openai_responses
        } else {
            workbuddy::chat_request_to_responses
        };
        let mut converted = convert(raw)?;
        if self.seed.is_none() {
            return Ok(converted);
        }
        let messages = raw
            .get("messages")
            .and_then(Value::as_array)
            .ok_or_else(|| "messages must be an array".to_string())?;
        let mut input = Vec::new();
        for (message, key) in messages.iter().zip(self.prefix_keys(raw)) {
            if let Some(output) = key
                .and_then(|key| self.lookup(&key))
                .and_then(|data| data.responses_output)
            {
                input.extend(output);
                continue;
            }
            let single = convert(&json!({
                "model": raw["model"],
                "messages": [message],
                "stream": false,
            }))?;
            if let Some(items) = single.get("input").and_then(Value::as_array) {
                input.extend(items.iter().cloned());
            }
        }
        converted["input"] = Value::Array(input);
        Ok(converted)
    }

    pub fn remember_chat(&self, chat_response: &Value) {
        self.remember(chat_response, None);
    }

    pub fn remember_responses(&self, responses: &Value, chat_response: &Value) {
        if responses.get("error").is_some_and(|value| !value.is_null())
            || matches!(
                responses.get("status").and_then(Value::as_str),
                Some("failed" | "cancelled" | "in_progress" | "queued")
            )
        {
            return;
        }
        let Some(output) = responses.get("output").and_then(Value::as_array) else {
            return;
        };
        self.remember(chat_response, Some(output));
    }

    fn remember(&self, chat_response: &Value, output: Option<&Vec<Value>>) {
        let Some(history) = &self.history else {
            return;
        };
        if chat_response
            .get("error")
            .is_some_and(|value| !value.is_null())
        {
            return;
        }
        let Some(choices) = chat_response.get("choices").and_then(Value::as_array) else {
            return;
        };
        for message in choices.iter().filter_map(|choice| choice.get("message")) {
            if message.get("role").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let mut hash = history.clone();
            if hash_message(&mut hash, message).is_none() {
                continue;
            }
            let data = ReplayData {
                chat_fields: CHAT_STATE_FIELDS
                    .iter()
                    .filter_map(|field| {
                        message
                            .get(*field)
                            .filter(|value| !value.is_null())
                            .map(|value| ((*field).to_string(), value.clone()))
                    })
                    .collect(),
                responses_output: output.cloned(),
            };
            // An oversized/new state also invalidates an older identical key.
            // Keeping the old signature would silently replay a different turn.
            let encoded = if data.chat_fields.is_empty() && data.responses_output.is_none() {
                None
            } else {
                encode_bounded(&data)
            };
            if let Ok(mut cache) = self.cache.lock() {
                cache.insert(hash.finalize().into(), encoded, Instant::now());
            }
        }
    }

    fn prefix_keys(&self, raw: &Value) -> Vec<Option<ReplayKey>> {
        let Some(messages) = raw.get("messages").and_then(Value::as_array) else {
            return Vec::new();
        };
        let mut state = self.seed.clone();
        messages
            .iter()
            .map(|message| {
                let hash = state.as_mut()?;
                if hash_message(hash, message).is_none() {
                    state = None;
                    return None;
                }
                (message.get("role").and_then(Value::as_str) == Some("assistant"))
                    .then(|| hash.clone().finalize().into())
            })
            .collect()
    }

    fn lookup(&self, key: &ReplayKey) -> Option<ReplayData> {
        self.cache.lock().ok()?.get(key, Instant::now())
    }
}

fn provider_seed(provider: &ProviderConfig, model: &str) -> Option<Sha256> {
    let provider = serde_json::to_value(provider).ok()?;
    let mut hash = Sha256::new();
    hash.update(b"tiancaispacehub-gmclaw-replay-v1\0");
    hash_value(&mut hash, &canonical_value(&provider))?;
    hash.update((model.len() as u64).to_be_bytes());
    hash.update(model.as_bytes());
    Some(hash)
}

fn hash_message(hash: &mut Sha256, message: &Value) -> Option<()> {
    let object = message.as_object()?;
    let role = object.get("role")?.as_str()?;
    let content = match object.get("content") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(text)) if text.is_empty() => Value::Null,
        Some(Value::Array(parts)) if parts.is_empty() => Value::Null,
        Some(content) => canonical_value(content),
    };
    let calls = match object.get("tool_calls") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(calls)) => calls
            .iter()
            .map(|call| {
                let function = call.get("function")?.as_object()?;
                let arguments = match function.get("arguments") {
                    Some(Value::String(arguments)) => serde_json::from_str::<Value>(arguments)
                        .unwrap_or_else(|_| Value::String(arguments.clone())),
                    Some(arguments) => arguments.clone(),
                    None => Value::Null,
                };
                Some(json!({
                    "id": canonical_call_id(call.get("id")),
                    "name": function.get("name").cloned().unwrap_or(Value::Null),
                    "arguments": canonical_value(&arguments),
                }))
            })
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    hash_value(
        hash,
        &canonical_value(&json!({
            "role": role,
            "content": content,
            "tool_calls": calls,
            "tool_call_id": if role == "tool" { canonical_call_id(object.get("tool_call_id")) } else { Value::Null },
            // GMClaw's sanitizer drops assistant/tool names entirely.
            "name": if matches!(role, "system" | "user") {
                object.get("name").cloned().unwrap_or(Value::Null)
            } else {
                Value::Null
            },
        })),
    )
}

fn canonical_call_id(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(id)) => {
            Value::String(id.chars().filter(|c| !c.is_whitespace()).collect())
        }
        Some(value) => value.clone(),
        None => Value::Null,
    }
}

fn hash_value(hash: &mut Sha256, value: &Value) -> Option<()> {
    let encoded = serde_json::to_vec(value).ok()?;
    hash.update((encoded.len() as u64).to_be_bytes());
    hash.update(encoded);
    Some(())
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort_unstable();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonical_value(&object[key])))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
        value => value.clone(),
    }
}

#[derive(Serialize, Deserialize)]
struct ReplayData {
    chat_fields: Map<String, Value>,
    responses_output: Option<Vec<Value>>,
}

struct BoundedBuffer(Vec<u8>);

impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_ENTRY_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other("replay entry exceeds the memory limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode_bounded(data: &ReplayData) -> Option<Vec<u8>> {
    let mut buffer = BoundedBuffer(Vec::new());
    serde_json::to_writer(&mut buffer, data).ok()?;
    Some(buffer.0)
}

struct CacheEntry {
    key: ReplayKey,
    encoded: Box<[u8]>,
    created_at: Instant,
}

#[derive(Default)]
struct ReplayCache {
    entries: VecDeque<CacheEntry>,
    bytes: usize,
}

impl ReplayCache {
    fn expire(&mut self, now: Instant) {
        self.entries.retain(|entry| {
            if now.saturating_duration_since(entry.created_at) >= TTL {
                self.bytes -= entry.encoded.len();
                false
            } else {
                true
            }
        });
    }

    fn get(&mut self, key: &ReplayKey, now: Instant) -> Option<ReplayData> {
        self.expire(now);
        let entry = self.entries.iter().find(|entry| entry.key == *key)?;
        serde_json::from_slice(&entry.encoded).ok()
    }

    fn insert(&mut self, key: ReplayKey, encoded: Option<Vec<u8>>, now: Instant) {
        self.expire(now);
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key)
            && let Some(entry) = self.entries.remove(index)
        {
            self.bytes -= entry.encoded.len();
        }
        let Some(encoded) = encoded.filter(|bytes| bytes.len() <= MAX_ENTRY_BYTES) else {
            return;
        };
        while self.entries.len() >= MAX_ENTRIES || self.bytes + encoded.len() > MAX_BYTES {
            let Some(entry) = self.entries.pop_front() else {
                return;
            };
            self.bytes -= entry.encoded.len();
        }
        self.bytes += encoded.len();
        self.entries.push_back(CacheEntry {
            key,
            encoded: encoded.into_boxed_slice(),
            created_at: now,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> ProviderConfig {
        ProviderConfig {
            name: "gmclaw".into(),
            api_key: "test-key".into(),
            base_url: "https://example.invalid/v1".into(),
            ..ProviderConfig::default()
        }
    }

    fn context(
        raw: &Value,
        provider: &ProviderConfig,
        cache: &Arc<Mutex<ReplayCache>>,
    ) -> ReplayContext {
        let mut context = ReplayContext::new(raw, provider, "actual-model");
        context.cache = cache.clone();
        context
    }

    fn completion(message: Value) -> Value {
        json!({"choices":[{"index":0,"message":message,"finish_reason":"stop"}]})
    }

    fn assistant(arguments: &str) -> Value {
        json!({
            "role":"assistant", "content":null,
            "tool_calls":[{"id":"call_same","type":"function","function":{
                "name":"lookup", "arguments":arguments,
            }}],
        })
    }

    #[test]
    fn chat_replays_all_assistant_turns_and_preserves_explicit_reasoning() {
        let cache = Arc::new(Mutex::new(ReplayCache::default()));
        let provider = provider();
        let first = json!({"model":"visible","messages":[{"role":"user","content":"hello"}]});
        let mut plain = json!({"role":"assistant","content":"hello back"});
        let mut saved_plain = plain.clone();
        saved_plain["name"] = json!("upstream-assistant-name");
        saved_plain["reasoning_content"] = json!("original thought");
        saved_plain["reasoning_details"] = json!([{"type":"reasoning.encrypted","data":"opaque"}]);
        context(&first, &provider, &cache).remember_chat(&completion(saved_plain));
        let second = json!({"model":"visible","messages":[
            first["messages"][0], plain, {"role":"user","content":"find it"},
        ]});
        let mut saved_tool = assistant(r#"{"b":2,"a":1}"#);
        saved_tool["tool_calls"][0]["id"] = json!(" call_same \n");
        saved_tool["reasoning_content"] = json!("tool thought");
        context(&second, &provider, &cache).remember_chat(&completion(saved_tool));
        plain["reasoning_content"] = json!("client value");
        let mut tool = assistant(r#"{ "a": 1, "b": 2 }"#);
        tool["content"] = json!("");
        let mut next = json!({"model":"visible","messages":[
            first["messages"][0], plain, second["messages"][2], tool,
            {"role":"tool","tool_call_id":"call_same","content":"found"},
        ]});
        context(&next, &provider, &cache).restore_chat(&mut next);
        assert_eq!(next["messages"][1]["reasoning_content"], "client value");
        assert_eq!(
            next["messages"][1]["reasoning_details"][0]["data"],
            "opaque"
        );
        assert_eq!(next["messages"][3]["reasoning_content"], "tool thought");
    }

    #[test]
    fn responses_replay_preserves_items_once_and_full_request_options() {
        let cache = Arc::new(Mutex::new(ReplayCache::default()));
        let provider = provider();
        let first = json!({"model":"visible","messages":[
            {"role":"system","content":"system one"}, {"role":"user","content":"lookup"},
        ]});
        let output = json!([
            {"id":"reason_1","type":"reasoning","encrypted_content":"signed","summary":[]},
            {"id":"msg_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"checking"}]},
            {"id":"function_item_1","type":"function_call","call_id":"call_same","name":"lookup","arguments":"{}","status":"completed"},
        ]);
        let response = json!({"status":"completed","output":output});
        let chat = workbuddy::responses_to_chat(&response, "visible");
        context(&first, &provider, &cache).remember_responses(&response, &chat);
        let next = json!({"model":"visible","max_tokens":2048,"reasoning_effort":"high","messages":[
            first["messages"][0], first["messages"][1], chat["choices"][0]["message"],
            {"role":"tool","tool_call_id":"call_same","content":"result"},
            {"role":"system","content":"system two"},
        ],"tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}]});
        for openai in [true, false] {
            let converted = context(&next, &provider, &cache)
                .to_responses(&next, openai)
                .unwrap();
            assert_eq!(converted["instructions"], "system one\n\nsystem two");
            assert_eq!(converted["max_output_tokens"], 2048);
            assert_eq!(converted["reasoning"]["effort"], "high");
            assert_eq!(converted["tools"][0]["name"], "lookup");
            let input = converted["input"].as_array().unwrap();
            assert_eq!(input.len(), 5);
            assert_eq!(&input[1..4], output.as_array().unwrap());
            assert_eq!(input[4]["type"], "function_call_output");
        }
    }

    #[test]
    fn credentials_model_full_history_and_arguments_isolate_replay() {
        let cache = Arc::new(Mutex::new(ReplayCache::default()));
        let provider = provider();
        let first = json!({"model":"visible","messages":[{"role":"user","content":"original"}]});
        let message = assistant(r#"{"q":"original"}"#);
        let mut saved = message.clone();
        saved["reasoning_content"] = json!("must stay isolated");
        context(&first, &provider, &cache).remember_chat(&completion(saved));
        let next = json!({"model":"visible","messages":[first["messages"][0],message]});
        let mut changed_key = provider.clone();
        changed_key.api_key = "different-test-key".into();
        let mut changed_url = provider.clone();
        changed_url.base_url = "https://other.invalid/v1".into();
        let mut changed_option = provider.clone();
        changed_option.chat_disable_reasoning = true;
        for other in [changed_key, changed_url, changed_option] {
            let mut request = next.clone();
            context(&request, &other, &cache).restore_chat(&mut request);
            assert!(request["messages"][1].get("reasoning_content").is_none());
        }
        let mut other_model = ReplayContext::new(&next, &provider, "different-model");
        other_model.cache = cache.clone();
        let mut request = next.clone();
        other_model.restore_chat(&mut request);
        assert!(request["messages"][1].get("reasoning_content").is_none());
        for change_arguments in [false, true] {
            let mut request = next.clone();
            if change_arguments {
                request["messages"][1]["tool_calls"][0]["function"]["arguments"] =
                    json!(r#"{"q":"different"}"#);
            } else {
                request["messages"][0]["content"] = json!("different history");
            }
            context(&request, &provider, &cache).restore_chat(&mut request);
            assert!(request["messages"][1].get("reasoning_content").is_none());
        }
    }

    #[test]
    fn no_match_and_streaming_never_invent_private_state() {
        let cache = Arc::new(Mutex::new(ReplayCache::default()));
        let provider = provider();
        let mut request = json!({"model":"visible","messages":[assistant("{}")]});
        let ctx = context(&request, &provider, &cache);
        ctx.restore_chat(&mut request);
        for openai in [true, false] {
            let expected = if openai {
                workbuddy::chat_request_to_openai_responses(&request)
            } else {
                workbuddy::chat_request_to_responses(&request)
            }
            .unwrap();
            assert_eq!(ctx.to_responses(&request, openai).unwrap(), expected);
        }
        let streaming = json!({"model":"visible","stream":true,"messages":[]});
        let mut saved = assistant("{}");
        saved["reasoning_content"] = json!("do not cache a stream");
        context(&streaming, &provider, &cache).remember_chat(&completion(saved));
        assert!(cache.lock().unwrap().entries.is_empty());
    }

    #[test]
    fn cache_limits_expiry_and_replacement_bound_retained_state() {
        let now = Instant::now();
        let data = ReplayData {
            chat_fields: [("reasoning_content".into(), json!("state"))]
                .into_iter()
                .collect(),
            responses_output: None,
        };
        let encoded = encode_bounded(&data).unwrap();
        let mut cache = ReplayCache::default();
        for id in 0..=MAX_ENTRIES {
            let key = Sha256::digest(id.to_be_bytes()).into();
            cache.insert(key, Some(encoded.clone()), now);
        }
        assert!(
            cache
                .get(&Sha256::digest(0usize.to_be_bytes()).into(), now)
                .is_none()
        );
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        assert!(
            cache
                .get(
                    &Sha256::digest(MAX_ENTRIES.to_be_bytes()).into(),
                    now + TTL - Duration::from_nanos(1)
                )
                .is_some()
        );
        assert!(
            cache
                .get(&Sha256::digest(MAX_ENTRIES.to_be_bytes()).into(), now + TTL)
                .is_none()
        );
        assert_eq!(cache.bytes, 0);
        for id in 0u8..17 {
            cache.insert([id; 32], Some(vec![b' '; MAX_ENTRY_BYTES]), now);
        }
        assert_eq!(cache.bytes, MAX_BYTES);
        assert_eq!(cache.entries.len(), 16);
        assert!(cache.entries.iter().all(|entry| entry.key != [0; 32]));
        cache.insert([16; 32], Some(encoded), now);
        assert_eq!(cache.entries.len(), 16);
        assert!(cache.bytes < MAX_BYTES);
        cache.insert([16; 32], Some(vec![b' '; MAX_ENTRY_BYTES + 1]), now);
        assert!(cache.entries.iter().all(|entry| entry.key != [16; 32]));
        let huge = ReplayData {
            chat_fields: [(
                "reasoning_content".into(),
                json!("x".repeat(MAX_ENTRY_BYTES)),
            )]
            .into_iter()
            .collect(),
            responses_output: None,
        };
        assert!(encode_bounded(&huge).is_none());
    }
}
