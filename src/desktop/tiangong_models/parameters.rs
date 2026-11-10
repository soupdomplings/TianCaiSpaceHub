//! Native model defaults, shared by every client copy. No credentials are read.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Parameters {
    pub context_window: u64,
    pub generation: BTreeMap<String, Value>,
    pub stream: bool,
    pub timeout_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_images: Option<bool>,
}

const OPTIONS: &[&str] = &[
    "top_p",
    "top_k",
    "min_p",
    "presence_penalty",
    "frequency_penalty",
    "seed",
    "stop",
    "reasoning_effort",
    "thinking",
    "enable_thinking",
    "response_format",
    "parallel_tool_calls",
    "repetition_penalty",
    "include_reasoning",
];

impl Parameters {
    pub(super) fn read(db: &Connection, id: &str) -> Result<Self> {
        let columns = db
            .prepare("PRAGMA table_info(model_configs)")?
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Old schemas may lack optional parameters. Use the native runtime's
        // defaults for missing/null fields, never another client's defaults.
        let field = |name: &str| {
            if columns.iter().any(|c| c == name) {
                name.to_string()
            } else {
                "NULL".into()
            }
        };
        let query = format!(
            "SELECT {},{},{} FROM model_configs WHERE model_id=?1",
            field("max_tokens"),
            field("temperature"),
            field("extra_params")
        );
        let (output, temperature, extra): (Option<i64>, Option<f64>, Option<String>) =
            db.query_row(&query, [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        let extra = match extra.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(raw) => {
                ensure!(raw.len() <= 128 * 1024, "天工模型参数过大，请检查原配置");
                serde_json::from_str::<Value>(raw)
                    .context("天工模型参数不是有效 JSON，请在天工中修正")?
            }
            None => json!({}),
        };
        ensure!(extra.is_object(), "天工模型参数需要为对象");
        let tuning = extra.get("model_tuning").cloned().unwrap_or(json!({}));
        ensure!(tuning.is_object(), "天工会话模型参数格式无效");
        let context = tuning
            .get("context_window")
            .or_else(|| extra.get("context_window"))
            .or_else(|| extra.get("contextWindow"))
            .or_else(|| extra.get("discovered_context_window"));
        let context_window = match context {
            Some(v) => v
                .as_u64()
                .filter(|v| (1024..=2_147_483_647).contains(v))
                .context("天工模型上下文窗口无效")?,
            None => 128000,
        };
        let output = output.unwrap_or(4096);
        ensure!(
            output > 0 && output <= 2_147_483_647,
            "天工模型输出上限无效"
        );
        let temperature = temperature.unwrap_or(0.7);
        ensure!(
            temperature.is_finite() && (0.0..=2.0).contains(&temperature),
            "天工模型 temperature 无效"
        );
        let options = extra.get("request_options").cloned().unwrap_or(json!({}));
        let options = options.as_object().context("天工请求参数需要为对象")?;
        let mut generation = BTreeMap::new();
        for (key, value) in options {
            ensure!(
                OPTIONS.contains(&key.as_str()),
                "天工内置模型含暂未适配的请求参数 {key}，已停止导入，未丢弃原设置"
            );
            generation.insert(key.clone(), value.clone());
        }
        ensure!(
            tuning.as_object().unwrap().keys().all(|key| matches!(
                key.as_str(),
                "thinking" | "reasoning_effort" | "context_window"
            )),
            "天工会话模型参数尚未适配，已停止导入"
        );
        let protocol = extra
            .get("reasoning_protocol")
            .map(|p| {
                p.as_str()
                    .filter(|p| {
                        matches!(
                            *p,
                            "openai" | "openai_none" | "thinking" | "enable_thinking"
                        )
                    })
                    .context("天工推理参数协议无效")
            })
            .transpose()?;
        if let Some(thinking) = tuning.get("thinking") {
            let state = thinking
                .as_str()
                .filter(|s| matches!(*s, "enabled" | "disabled"))
                .context("天工思考设置无效")?;
            let protocol = protocol.context("天工思考参数协议未指定，请先在天工确认")?;
            generation.remove("thinking");
            generation.remove("enable_thinking");
            let previous_effort = generation.get("reasoning_effort").cloned();
            if state == "disabled" {
                generation.remove("reasoning_effort");
            }
            match protocol {
                "thinking" => {
                    generation.insert("thinking".into(), json!({"type":state}));
                }
                "enable_thinking" => {
                    generation.insert("enable_thinking".into(), json!(state == "enabled"));
                }
                "openai_none" => {
                    generation.insert(
                        "reasoning_effort".into(),
                        if state == "disabled" {
                            json!("none")
                        } else {
                            tuning
                                .get("reasoning_effort")
                                .cloned()
                                .or(previous_effort.filter(|e| e != "none"))
                                .unwrap_or(json!("medium"))
                        },
                    );
                }
                _ => anyhow::bail!("天工内置模型思考协议暂未适配，未改写原参数"),
            }
        }
        if let Some(effort) = tuning.get("reasoning_effort") {
            let allowed = match protocol {
                Some("openai") => &["low", "medium", "high"][..],
                Some("openai_none") => &["minimal", "low", "medium", "high", "xhigh", "max"][..],
                _ => &[][..],
            };
            ensure!(
                effort.as_str().is_some_and(|e| allowed.contains(&e)),
                "天工内置模型未确认此推理强度，已保留原配置"
            );
            let disabled = tuning["thinking"] == "disabled"
                || generation
                    .get("thinking")
                    .is_some_and(|v| v["type"] == "disabled")
                || generation.get("enable_thinking") == Some(&Value::Bool(false))
                || (protocol == Some("openai_none")
                    && generation.get("reasoning_effort") == Some(&json!("none")));
            if !disabled {
                generation.insert("reasoning_effort".into(), effort.clone());
            }
        }
        let output_field = match extra.get("token_limit_field").and_then(Value::as_str) {
            None | Some("max_tokens") => "max_tokens",
            Some("max_completion_tokens") => "max_completion_tokens",
            _ => anyhow::bail!("天工输出长度参数名称无效"),
        };
        generation.insert(output_field.into(), json!(output));
        if extra["omit_temperature"] != true {
            generation.insert("temperature".into(), json!(temperature));
        }
        let timeout_secs = match extra.get("timeout") {
            None => 600,
            Some(v) => v
                .as_u64()
                .filter(|v| *v > 0 && *v <= 86400)
                .context("天工请求超时无效")?,
        };
        if let Some(value) = extra.get("stream") {
            ensure!(value.is_boolean(), "天工流式参数无效");
        }
        let result = Self {
            context_window,
            generation,
            stream: extra["stream"] != false,
            timeout_secs,
            supports_images: extra.get("supports_vision").and_then(Value::as_bool),
        };
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1024..=2_147_483_647).contains(&self.context_window)
                && (1..=86400).contains(&self.timeout_secs),
            "天工模型窗口或超时无效，请重新读取天工配置"
        );
        ensure!(
            serde_json::to_vec(&self.generation)?.len() <= 128 * 1024,
            "天工模型请求参数过大"
        );
        for (key, value) in &self.generation {
            ensure!(
                OPTIONS.contains(&key.as_str())
                    || matches!(
                        key.as_str(),
                        "max_tokens" | "max_completion_tokens" | "temperature"
                    ),
                "天工模型含未适配的请求参数，已停止调用，请重新读取天工配置"
            );
            match key.as_str() {
                "max_tokens" | "max_completion_tokens" => ensure!(
                    value.as_u64().is_some_and(|n| n > 0 && n <= 2_147_483_647),
                    "天工模型输出上限无效"
                ),
                "temperature" => ensure!(
                    value.as_f64().is_some_and(|n| (0.0..=2.0).contains(&n)),
                    "天工采样温度无效"
                ),
                "top_p" => ensure!(
                    value.as_f64().is_some_and(|n| (0.0..=1.0).contains(&n)),
                    "天工 top_p 无效"
                ),
                "thinking" => ensure!(
                    value.is_object()
                        && value["type"]
                            .as_str()
                            .is_some_and(|s| matches!(s, "enabled" | "disabled")),
                    "天工 thinking 参数无效"
                ),
                "enable_thinking" => ensure!(value.is_boolean(), "天工 enable_thinking 参数无效"),
                "reasoning_effort" => ensure!(
                    value
                        .as_str()
                        .is_some_and(|s| !s.trim().is_empty() && s.len() <= 64),
                    "天工推理强度无效"
                ),
                _ => {}
            }
        }
        ensure!(
            self.generation.contains_key("max_tokens")
                ^ self.generation.contains_key("max_completion_tokens"),
            "天工模型必须且只能指定一种输出长度参数"
        );
        Ok(())
    }
    pub fn output_limit(&self) -> u64 {
        self.generation
            .get("max_tokens")
            .or_else(|| self.generation.get("max_completion_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(4096)
    }
    pub fn compact_threshold(&self) -> u64 {
        self.context_window.saturating_mul(70) / 100
    }
    pub fn capabilities(&self) -> Value {
        json!({"contextWindow":self.context_window,"maxTokens":self.output_limit(),"preferStream":self.stream,"supportsImage":self.supports_images.unwrap_or(false),"defaultReasoningEffort":"auto","reasoningEfforts":[],"tiangongParameters":self})
    }
    pub fn apply(&self, body: &mut Value) {
        let Some(object) = body.as_object_mut() else {
            return;
        };
        for key in OPTIONS.iter().copied().chain([
            "max_tokens",
            "max_completion_tokens",
            "temperature",
            "reasoning",
        ]) {
            object.remove(key);
        }
        object.extend(self.generation.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
}

pub fn for_provider(provider: &ProviderConfig) -> Option<Parameters> {
    let binding = provider.tiangong_builtin.as_ref()?;
    binding.parameters.clone().or_else(|| {
        // Existing V16 bindings gain the native defaults without a destructive
        // migration or a login/network request. Bind by exact native identity.
        discover_at(&crate::gmclaw_config::config_path())
            .ok()?
            .into_iter()
            .find(|m| m.id == binding.model_id && m.upstream_model == binding.upstream_model)
            .map(|m| m.parameters)
    })
}

// Only TianGong-owned defaults are synchronized. Ordinary copies retain their
// independent parameters. All native changes go through the existing writers.
pub(super) fn sync_copies(path: &Path) -> Vec<String> {
    let result = (|| -> Result<Vec<String>> {
        let config = AppConfig::load_or_default(&path.to_path_buf())?;
        let library = crate::desktop::resources::Library::new(path)?;
        let targets = library.targets(&library.load()?)?;
        let ids = config
            .ai_gateway
            .providers
            .iter()
            .filter(|p| p.enabled && p.tiangong_builtin.is_some())
            .filter_map(|p| p.desktop_binding.as_ref().map(|b| b.target_id.clone()))
            .collect::<std::collections::BTreeSet<_>>();
        let mut errors = Vec::new();
        for id in ids {
            let Some(target) = targets.iter().find(|t| t.id == id) else {
                errors.push("部分客户端位置已变化，请重新选择模型配置位置".into());
                continue;
            };
            if let Err(_) = sync_target(path, target) {
                errors.push(format!(
                    "{}的本机配置后来有改动，未覆盖；请在该端模型页核对后保存",
                    target.client.name()
                ));
            }
        }
        let mut config = AppConfig::load_or_default(&path.to_path_buf())?;
        let entries = config
            .ai_gateway
            .providers
            .iter()
            .filter(|p| p.enabled && p.is_workbuddy() && p.tiangong_builtin.is_some())
            .cloned()
            .collect::<Vec<_>>();
        for provider in entries {
            let outcome = (|| -> Result<()> {
                let native = provider.tiangong_builtin.as_ref().unwrap();
                let source = config
                    .ai_gateway
                    .providers
                    .iter()
                    .find(|p| {
                        !p.is_client_reserved()
                            && p.desktop_binding.is_none()
                            && p.tiangong_builtin.as_ref().is_some_and(|b| {
                                b.model_id == native.model_id
                                    && b.upstream_model == native.upstream_model
                            })
                    })
                    .cloned()
                    .context("来源已变化")?;
                let id = provider.name.strip_prefix("workbuddy:").unwrap_or("legacy");
                let status = crate::workbuddy_config::load_selected(&config, Some(id))?;
                ensure!(
                    status.entries.iter().any(|e| e.entry_id == id),
                    "客户端模型条目已变化"
                );
                let mut model = status.model;
                if let Some(p) = for_provider(&source) {
                    let effort = p
                        .generation
                        .get("reasoning_effort")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let efforts = if effort.is_empty() {
                        Vec::<String>::new()
                    } else {
                        vec![effort.into()]
                    };
                    if model.upstream_protocol == "openai-chat"
                        && model.extra.get("contextWindow") == Some(&json!(p.context_window))
                        && model.extra.get("maxTokens") == Some(&json!(p.output_limit()))
                        && model.supports_images == p.supports_images.unwrap_or(false)
                        && model.reasoning.default_effort == effort
                        && model.reasoning.supported_efforts == efforts
                    {
                        return Ok(());
                    }
                }
                model.upstream_protocol = "openai-chat".into();
                crate::workbuddy_config::save_copy(
                    &crate::workbuddy_config::WorkBuddySaveRequest {
                        entry_id: Some(id.into()),
                        revision: status.revision,
                        model,
                    },
                    &source,
                    &mut config,
                    path,
                )?;
                Ok(())
            })();
            if outcome.is_err() {
                errors.push("WorkBuddy 的本机配置未同步，请在模型页重新读取后保存原条目".into());
            }
            config = AppConfig::load_or_default(&path.to_path_buf())?;
        }
        Ok(errors)
    })();
    result.unwrap_or_else(|_| vec!["客户端参数尚未同步，请重新读取天工模型后重试".into()])
}

pub(super) fn sync_target(
    path: &Path,
    target: &crate::desktop::targets::ClientTarget,
) -> Result<()> {
    let before = AppConfig::load_or_default(&path.to_path_buf())?;
    let mut next = before.clone();
    let mut changed = false;
    for provider in &mut next.ai_gateway.providers {
        if !provider.enabled {
            continue;
        }
        let Some(parameters) = for_provider(provider) else {
            continue;
        };
        let Some(binding) = provider
            .desktop_binding
            .as_mut()
            .filter(|b| b.target_id == target.id)
        else {
            continue;
        };
        if binding.capabilities == parameters.capabilities() {
            continue;
        }
        crate::desktop::models::verify_later_changes(binding, target)?;
        binding.capabilities = parameters.capabilities();
        if let Some(source) = before
            .ai_gateway
            .providers
            .iter()
            .find(|p| crate::ai_gateway::config::provider_route_id(p) == binding.source_route)
        {
            binding.source_fingerprint = crate::desktop::models::fingerprint(source)?;
        }
        changed = true;
    }
    if changed {
        crate::desktop::model_operations::commit(path, &before, &mut next, target, false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_parameters_preserve_field_meaning_and_override_client_donor_defaults() {
        let (_temp, db, _) = super::super::tests::fixture();
        let c = Connection::open(&db).unwrap();
        c.execute_batch("ALTER TABLE model_configs ADD COLUMN max_tokens INTEGER DEFAULT 128000; ALTER TABLE model_configs ADD COLUMN temperature REAL DEFAULT 0.7; ALTER TABLE model_configs ADD COLUMN extra_params TEXT DEFAULT '{}';").unwrap();
        let p = Parameters::read(&c, IDS[0].0).unwrap();
        assert_eq!(p.context_window, 128000);
        assert_eq!(p.output_limit(), 128000);
        assert_eq!(p.compact_threshold(), 89600);
        let mut request = json!({"model":"keep","messages":[],"max_tokens":8192,"temperature":1,"reasoning_effort":"high","stream":true});
        p.apply(&mut request);
        assert_eq!(request["max_tokens"], 128000);
        assert_eq!(request["temperature"], 0.7);
        assert!(request.get("reasoning_effort").is_none());
        assert_eq!(request["model"], "keep");
        c.execute("UPDATE model_configs SET extra_params=?1 WHERE model_id=?2",rusqlite::params![r#"{"context_window":64000,"model_tuning":{"context_window":96000},"token_limit_field":"max_completion_tokens","request_options":{"top_p":0.8},"stream":false}"#,IDS[0].0]).unwrap();
        let p = Parameters::read(&c, IDS[0].0).unwrap();
        assert_eq!(p.context_window, 96000);
        assert!(!p.stream);
        p.apply(&mut request);
        assert!(request.get("max_tokens").is_none());
        assert_eq!(request["max_completion_tokens"], 128000);
        assert_eq!(request["top_p"], 0.8);
        c.execute(
            "UPDATE model_configs SET extra_params=?1 WHERE model_id=?2",
            rusqlite::params![
                r#"{"request_options":{"authorization":"must-not-copy"}}"#,
                IDS[0].0
            ],
        )
        .unwrap();
        assert!(
            !Parameters::read(&c, IDS[0].0)
                .unwrap_err()
                .to_string()
                .contains("must-not-copy")
        );
    }

    #[test]
    fn native_tuning_protocols_keep_source_thinking_and_never_invent_gpt_defaults() {
        let (_temp, db, _) = super::super::tests::fixture();
        let c = Connection::open(db).unwrap();
        c.execute_batch("ALTER TABLE model_configs ADD COLUMN extra_params TEXT;")
            .unwrap();
        for (extra, expected) in [
            (
                json!({"reasoning_protocol":"openai","model_tuning":{"reasoning_effort":"high"}}),
                json!({"reasoning_effort":"high"}),
            ),
            (
                json!({"reasoning_protocol":"openai_none","model_tuning":{"thinking":"disabled","reasoning_effort":"max"}}),
                json!({"reasoning_effort":"none"}),
            ),
            (
                json!({"reasoning_protocol":"thinking","model_tuning":{"thinking":"enabled"}}),
                json!({"thinking":{"type":"enabled"}}),
            ),
            (
                json!({"reasoning_protocol":"enable_thinking","model_tuning":{"thinking":"disabled"}}),
                json!({"enable_thinking":false}),
            ),
        ] {
            c.execute(
                "UPDATE model_configs SET extra_params=?1 WHERE model_id=?2",
                rusqlite::params![extra.to_string(), IDS[0].0],
            )
            .unwrap();
            let p = Parameters::read(&c, IDS[0].0).unwrap();
            let mut body = json!({"reasoning_effort":"xhigh","thinking":{"type":"disabled"},"enable_thinking":true});
            p.apply(&mut body);
            for key in ["reasoning_effort", "thinking", "enable_thinking"] {
                assert_eq!(body.get(key), expected.get(key));
            }
        }
    }

    #[test]
    fn existing_native_copy_updates_context_through_guarded_writer_and_preserves_later_edits() {
        use crate::desktop::{
            clients::ClientId,
            model_operations, models,
            targets::{self, McpFormat},
        };
        let (temp, db, path) = super::super::tests::fixture();
        let parameters = Parameters::read(&Connection::open(&db).unwrap(), IDS[0].0).unwrap();
        let mut source = ProviderConfig {
            name: "native-source".into(),
            base_url: BASE_URL.into(),
            provider_type: ProviderType::ChatCompletions,
            model_aliases: BTreeMap::from([("native-model".into(), "native-upstream".into())]),
            tiangong_builtin: Some(BuiltinBinding {
                model_id: IDS[0].0.into(),
                upstream_model: "native-upstream".into(),
                credential_ref: uuid::Uuid::new_v4().to_string(),
                parameters: Some(parameters.clone()),
            }),
            ..Default::default()
        };
        let root = temp.path().join("vscode");
        let target = targets::target(
            ClientId::VsCode,
            "fixture",
            root.clone(),
            root.join("skills"),
            root.join("mcp.json"),
            McpFormat::Servers,
            "native",
        );
        let mut before = AppConfig::load_or_default(&path).unwrap();
        before.ai_gateway.providers.push(source.clone());
        before.save(&path).unwrap();
        let mut next = before.clone();
        let mut copy = models::copy(&source, &target, "native-model", "kept-label").unwrap();
        copy.desktop_binding.as_mut().unwrap().capabilities =
            json!({"contextWindow":32768,"maxTokens":1024});
        next.ai_gateway.providers.push(copy);
        model_operations::commit(&path, &before, &mut next, &target, false).unwrap();
        sync_target(&path, &target).unwrap();
        let current = AppConfig::load_or_default(&path).unwrap();
        let binding = current.ai_gateway.providers[1]
            .desktop_binding
            .as_ref()
            .unwrap();
        assert_eq!(binding.capabilities["contextWindow"], 128000);
        assert_eq!(binding.label, "kept-label");
        assert!(models::using(binding, &target).unwrap());
        let native_file = binding.native_files[0].path.clone();
        let mut native: Value =
            serde_json::from_slice(&std::fs::read(&native_file).unwrap()).unwrap();
        native[0]["models"][0]["contextWindow"] = 64000.into();
        std::fs::write(&native_file, serde_json::to_vec(&native).unwrap()).unwrap();
        let native_before = std::fs::read(&native_file).unwrap();
        let mut current = current;
        source
            .tiangong_builtin
            .as_mut()
            .unwrap()
            .parameters
            .as_mut()
            .unwrap()
            .context_window = 256000;
        current.ai_gateway.providers[1].tiangong_builtin = source.tiangong_builtin.clone();
        current.save(&path).unwrap();
        let config_before = std::fs::read(&path).unwrap();
        assert!(sync_target(&path, &target).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), config_before);
        assert_eq!(std::fs::read(native_file).unwrap(), native_before);
    }
}
