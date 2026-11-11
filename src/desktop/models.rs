//! Client-owned model copies. The gateway never falls back across these scopes.
use super::{
    clients::ClientId,
    targets::ClientTarget,
    transaction::{self, FileChange},
};
use crate::ai_gateway::config::{AiGatewayConfig, ProviderConfig, provider_route_id};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const PREFIX: &str = "__tiancai_client_";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub id: String,
    pub client: ClientId,
    pub target_id: String,
    pub mode: String,
    pub model: String,
    pub label: String,
    pub source_route: String,
    pub source_fingerprint: String,
    pub local_token: String,
    #[serde(default)]
    pub wire_protocol: String,
    #[serde(default)]
    pub capabilities: Value,
    #[serde(default)]
    pub native_files: Vec<ManagedFile>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManagedFile {
    pub path: std::path::PathBuf,
    pub fingerprint: String,
    #[serde(default)]
    pub owned: Option<Value>,
}

/// Apply the entry's saved default only when the client omitted a choice.
/// Call after recording the original request so upstream defaults remain visible.
pub fn apply_request_defaults(raw: &mut Value, provider: &ProviderConfig, responses: bool) {
    let Some(effort) = provider
        .desktop_binding
        .as_ref()
        .and_then(|b| b.capabilities["defaultReasoningEffort"].as_str())
        .filter(|s| !s.is_empty() && *s != "auto")
    else {
        return;
    };
    if raw.get("reasoning_effort").is_some()
        || raw.get("thinking").is_some()
        || raw.get("output_config").is_some()
        || raw.get("reasoning").is_some()
    {
        return;
    }
    if responses {
        raw["reasoning"] = json!({"effort":effort});
    } else {
        raw["reasoning_effort"] = effort.into();
    }
}
pub fn prefers_upstream_stream(provider: &ProviderConfig) -> bool {
    if let Some(parameters) = super::tiangong_models::parameters_for_provider(provider) {
        return parameters.stream;
    }
    provider
        .desktop_binding
        .as_ref()
        .is_some_and(|b| b.capabilities["preferStream"] == true)
}
pub fn name(client: ClientId, id: &str) -> Result<String> {
    ensure!(uuid::Uuid::parse_str(id).is_ok(), "客户端模型身份无效");
    Ok(format!("{PREFIX}{}:{id}", client.key()))
}
pub fn client(key: &str) -> Result<ClientId> {
    ClientId::ALL
        .into_iter()
        .find(|c| c.key() == key)
        .context("未知客户端")
}
pub fn fingerprint(source: &ProviderConfig) -> Result<String> {
    if source.tiangong_builtin.is_none() {
        return Ok(transaction::fingerprint(Some(&serde_json::to_vec(source)?)));
    }
    let mut value = serde_json::to_value(source)?;
    // Rotating a shared account authorization does not change a copied model's
    // generation settings or require the user to synchronize each client.
    if let Some(binding) = value
        .get_mut("tiangongBuiltin")
        .and_then(Value::as_object_mut)
    {
        binding.remove("credentialRef");
    }
    Ok(transaction::fingerprint(Some(&serde_json::to_vec(&value)?)))
}
pub fn copy(
    source: &ProviderConfig,
    target: &ClientTarget,
    model: &str,
    label: &str,
) -> Result<ProviderConfig> {
    super::tiangong_models::validate_provider(source)?;
    let upstream = source
        .resolve_upstream_model(model)
        .context("来源不再提供此模型，请重新选择")?;
    let id = uuid::Uuid::new_v4().to_string();
    let mut provider = source.clone();
    provider.desktop_binding = Some(Binding {
        id: id.clone(),
        client: target.client,
        target_id: target.id.clone(),
        mode: target.model_mode.clone(),
        model: model.into(),
        label: if label.trim().is_empty() {
            model.into()
        } else {
            label.trim().into()
        },
        source_route: provider_route_id(source),
        source_fingerprint: fingerprint(source)?,
        local_token: format!("tch-{}", uuid::Uuid::new_v4()),
        wire_protocol: match source.provider_type {
            crate::ai_gateway::config::ProviderType::AnthropicMessages => "anthropic-messages",
            crate::ai_gateway::config::ProviderType::OpenAiResponses
            | crate::ai_gateway::config::ProviderType::ChatGptResponses => "openai-responses",
            _ => "openai-completions",
        }
        .into(),
        capabilities: super::tiangong_models::parameters_for_provider(source)
            .map(|p| p.capabilities())
            .unwrap_or(json!({})),
        native_files: vec![],
    });
    provider.name = name(target.client, &id)?;
    provider.models = vec![upstream.into()];
    provider.model_aliases.clear();
    provider.model_aliases.insert(model.into(), upstream.into());
    provider.enabled = true;
    // Source imports and GMClaw execution parameters are not ownership of this copy.
    provider.import_source = None;
    provider.gmclaw_parameters = None;
    Ok(provider)
}
pub fn public(config: &AiGatewayConfig) -> Value {
    let copies=config.providers.iter().filter_map(|p|p.desktop_binding.as_ref().map(|b|json!({"id":b.id,"client":b.client,"targetId":b.target_id,"mode":b.mode,"model":b.model,"label":b.label,"enabled":p.enabled,"routeId":provider_route_id(p),"sourceRoute":b.source_route,"wireProtocol":b.wire_protocol,"capabilities":b.capabilities,"sourceChanged":config.providers.iter().find(|s|provider_route_id(s)==b.source_route).map(|s|fingerprint(s).ok().as_deref()!=Some(b.source_fingerprint.as_str()))}))).collect::<Vec<_>>();
    let sources=config.providers.iter().map(|p| {
        let client = p.desktop_binding.as_ref().map(|b|b.client).or_else(|| {
            if p.is_workbuddy() { Some(ClientId::WorkBuddy) } else if p.is_gmclaw() { Some(ClientId::Tiangong) } else { None }
        });
        let label = p.desktop_binding.as_ref().map(|b|b.label.as_str()).unwrap_or_else(|| {
            if p.is_workbuddy() || p.is_gmclaw() { p.model_aliases.keys().next().or_else(||p.models.first()).map(String::as_str).unwrap_or("模型配置") } else { &p.name }
        });
        json!({"routeId":provider_route_id(p),"name":label,"client":client,"type":p.provider_type,"enabled":p.enabled,"models":p.models,"aliases":p.model_aliases,"tiangongBuiltin":p.tiangong_builtin.as_ref().map(|b|b.model_id.as_str())})
    }).collect::<Vec<_>>();
    json!({"copies":copies,"sources":sources})
}
pub fn verify_later_changes(binding: &Binding, target: &ClientTarget) -> Result<()> {
    for file in &binding.native_files {
        if target.client == ClientId::ZCode {
            ensure!(
                file.path.parent() == Some(target.models_root().join("v2").as_path()),
                "ZCode 模型数据目录已变化，请核对配置位置后选择替换；原目录不会自动清理"
            );
        }
        let bytes = transaction::read(&file.path)?;
        let mut unchanged = match &file.owned {
            Some(owned) => {
                let actual = owned_section(target, &file.path, bytes.as_deref())?;
                if target.client == ClientId::DeepSeekHarness
                    && actual.is_array()
                    && owned.is_array()
                {
                    super::harness_models::same_profile_snapshot(&actual, owned)
                } else {
                    actual == *owned
                }
            }
            None => transaction::fingerprint(bytes.as_deref()) == file.fingerprint,
        };
        if !unchanged
            && target.client == ClientId::DeepSeekHarness
            && file
                .path
                .file_name()
                .is_some_and(|name| name == "settings.yaml")
            && bytes.is_none()
        {
            if let Some(owned) = &file.owned {
                unchanged = super::harness_models::settings_imported(target, owned)?;
            }
        }
        ensure!(
            unchanged,
            "客户端已独立修改此模型配置，请重新读取并核对后替换"
        );
    }
    Ok(())
}

pub fn using(binding: &Binding, target: &ClientTarget) -> Result<bool> {
    ensure!(
        !target.models_inherited,
        "此 Profile 已沿用默认模型，请在默认配置位置管理"
    );
    if binding.native_files.is_empty() {
        return Ok(false);
    }
    if binding.native_files.iter().all(|f| !f.path.exists()) {
        return Ok(false);
    }
    verify_later_changes(binding, target)?;
    Ok(true)
}

pub fn owned_section(
    target: &ClientTarget,
    path: &std::path::Path,
    bytes: Option<&[u8]>,
) -> Result<Value> {
    let raw = bytes.unwrap_or_default();
    if target.client == ClientId::DeepSeekHarness {
        if path.file_name().is_some_and(|v| v == ".credentials.yaml") {
            let value: Value = serde_yaml_ng::from_slice(raw).unwrap_or(json!({}));
            return Ok(value["refs"]
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter(|(k, _)| k.starts_with("TIANCAI_HUB_"))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<serde_json::Map<_, _>>()
                })
                .unwrap_or_default()
                .into());
        }
        if path.file_name().is_some_and(|v| v == "settings.yaml") {
            let value: Value = if raw.is_empty() {
                json!({})
            } else {
                serde_yaml_ng::from_slice(raw)?
            };
            return Ok(value["llm-pi-ai"]["providers"]
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter(|(k, _)| k.starts_with("tiancai-hub-"))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<serde_json::Map<_, _>>()
                })
                .unwrap_or_default()
                .into());
        }
        return super::harness_models::owned_providers(raw);
    }
    let doc = super::formats::json(raw, Value::Null)?;
    Ok(match target.client {
        ClientId::VsCode => doc
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|v| {
                        v["name"]
                            .as_str()
                            .is_some_and(|s| s == format!("TianCaiSpaceHub-{}", &target.id[..12]))
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
            .into(),
        ClientId::ZCode => {
            if path
                .file_name()
                .is_some_and(|v| v == "provider_config.json")
            {
                json!({"providers":doc["config"]["providerConfigRules"]["providerRules"].as_array().map(|a|a.iter().filter(|v|v["providerId"].as_str().is_some_and(|s|s.starts_with("tiancai-hub-"))).cloned().collect::<Vec<_>>()).unwrap_or_default(),"models":doc["config"]["modelConfigRules"]["providerModelRules"].as_array().map(|a|a.iter().filter(|v|v["providerId"].as_str().is_some_and(|s|s.starts_with("tiancai-hub-"))).cloned().collect::<Vec<_>>()).unwrap_or_default()})
            } else {
                doc["provider"]
                    .as_object()
                    .map(|o| {
                        o.iter()
                            .filter(|(k, _)| k.starts_with("tiancai-hub-"))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect::<serde_json::Map<_, _>>()
                    })
                    .unwrap_or_default()
                    .into()
            }
        }
        ClientId::WebStorm => {
            if path.file_name().is_some_and(|v| v == "acp.json") {
                doc["agent_servers"]["TianCaiSpaceHub"].clone()
            } else {
                doc["provider"].clone()
            }
        }
        _ => doc,
    })
}
pub fn base(binding: &Binding, port: u16) -> String {
    format!(
        "http://127.0.0.1:{port}/ai-gateway/clients/{}/{}/v1",
        binding.client.key(),
        binding.id
    )
}

/// Prepare native files without touching them. Every apply uses compare-and-swap.
pub fn plan(
    target: &ClientTarget,
    bindings: &[Binding],
    port: u16,
    using: bool,
) -> Result<Vec<FileChange>> {
    use super::formats::{self, object_at_mut};
    let selected = bindings
        .iter()
        .filter(|b| b.target_id == target.id)
        .collect::<Vec<_>>();
    ensure!(
        !target.models_inherited || selected.is_empty(),
        "此 Profile 沿用默认模型，请在“默认”配置位置管理模型；Skill 和 MCP 可独立设置"
    );
    match target.client {
        ClientId::VsCode => {
            // An inherited profile's old managed file may be removed explicitly,
            // but must never modify the default profile's catalog on removal.
            let path = target.root.join("chatLanguageModels.json");
            let before = transaction::read(&path)?;
            let mut doc = formats::json(before.as_deref().unwrap_or_default(), json!([]))?;
            let groups = doc
                .as_array_mut()
                .context("VS Code 模型配置应为数组，未修改原文件")?;
            let key = format!("TianCaiSpaceHub-{}", &target.id[..12]);
            let mut group = groups
                .iter()
                .find(|g| g.get("name").and_then(Value::as_str) == Some(&key))
                .cloned()
                .unwrap_or(json!({}));
            group["name"] = key.clone().into();
            group["vendor"] = "customendpoint".into();
            group["apiType"] = "chat-completions".into();
            group["models"] = selected.iter().map(|b| {
                let mut model = json!({"id":b.model,"name":b.label,"url":format!("{}/chat/completions",base(b,port)),"toolCalling":true,"requestHeaders":{"Authorization":format!("Bearer {}",b.local_token)}});
                for (source, field) in [("contextWindow", "contextWindow"), ("maxTokens", "maxOutputTokens"), ("supportsImage", "vision"), ("toolCalling", "toolCalling")] {
                    if let Some(value) = b.capabilities.get(source) { model[field] = value.clone(); }
                }
                if let Some(efforts) = b.capabilities.get("reasoningEfforts").filter(|v| v.as_array().is_some_and(|a| !a.is_empty())) {
                    model["supportsReasoningEffort"] = efforts.clone();
                    model["reasoningEffortFormat"] = "chat-completions".into();
                }
                model
            }).collect::<Vec<_>>().into();
            if let Some(index) = groups
                .iter()
                .position(|g| g.get("name").and_then(Value::as_str) == Some(&key))
            {
                if using && !selected.is_empty() {
                    groups[index] = group;
                } else {
                    groups.remove(index);
                }
            } else if using && !selected.is_empty() {
                groups.push(group);
            }
            Ok(vec![FileChange::plan(
                path,
                before.as_deref(),
                Some(formats::write_json(
                    before.as_deref().unwrap_or_default(),
                    &doc,
                )?),
            )])
        }
        ClientId::ZCode => {
            let mut changes = Vec::new();
            let path = target.models_root().join("v2/config.json");
            let before = transaction::read(&path)?;
            let mut doc = formats::json(before.as_deref().unwrap_or_default(), json!({}))?;
            let providers = object_at_mut(&mut doc, &["provider"])?;
            providers.retain(|k, _| !k.starts_with("tiancai-hub-"));
            for b in &selected {
                let key = format!("tiancai-hub-{}", b.id);
                providers.insert(key,json!({"name":b.label,"kind":"anthropic","enabled":true,"source":"custom","options":{"apiKey":b.local_token,"baseURL":base(b,port).trim_end_matches("/v1")},"models":{b.model.clone():{"name":b.label}}}));
            }
            changes.push(FileChange::plan(
                path,
                before.as_deref(),
                Some(formats::write_json(
                    before.as_deref().unwrap_or_default(),
                    &doc,
                )?),
            ));
            let path = target.models_root().join("v2/provider_config.json");
            let before = transaction::read(&path)?;
            let mut doc = formats::json(
                before.as_deref().unwrap_or_default(),
                json!({"schemaVersion":1,"config":{}}),
            )?;
            ensure!(
                doc.get("schemaVersion").and_then(Value::as_u64) == Some(1),
                "ZCode 模型规则版本不兼容，未修改"
            );
            let rule = object_at_mut(&mut doc, &["config", "providerConfigRules"])?;
            let rows = rule
                .entry("providerRules")
                .or_insert(json!([]))
                .as_array_mut()
                .context("ZCode providerRules 格式无效")?;
            rows.retain(|r| {
                !r.get("providerId")
                    .and_then(Value::as_str)
                    .is_some_and(|v| v.starts_with("tiancai-hub-"))
            });
            for b in &selected {
                rows.push(json!({"providerId":format!("tiancai-hub-{}",b.id),"providerName":b.label,"enabled":true,"config":{"group":"standard-personal","access":{"type":"api-key","apiKey":b.local_token},"api":{"type":"anthropic-messages","baseUrl":base(b,port).trim_end_matches("/v1")},"personalModelIds":[b.model],"modelOrder":[b.model]}}));
            }
            let rules = object_at_mut(&mut doc, &["config", "modelConfigRules"])?;
            let manual = rules
                .get("manualProviderModelRules")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let active = selected
                .iter()
                .map(|b| format!("tiancai-hub-{}", b.id))
                .collect::<std::collections::BTreeSet<_>>();
            rules.insert(
                "manualProviderModelRules".into(),
                manual
                    .iter()
                    .filter(|r| {
                        r["providerId"]
                            .as_str()
                            .is_none_or(|p| !p.starts_with("tiancai-hub-") || active.contains(p))
                    })
                    .cloned()
                    .collect::<Vec<_>>()
                    .into(),
            );
            let rows = rules
                .entry("providerModelRules")
                .or_insert(json!([]))
                .as_array_mut()
                .context("ZCode 模型规则格式无效")?;
            let old = rows.clone();
            rows.retain(|r| {
                !r["providerId"]
                    .as_str()
                    .is_some_and(|v| v.starts_with("tiancai-hub-"))
            });
            for b in &selected {
                let provider_id = format!("tiancai-hub-{}", b.id);
                if manual
                    .iter()
                    .any(|r| r["providerId"] == provider_id && r["modelId"] == b.model)
                {
                    continue;
                }
                let mut config = old
                    .iter()
                    .find(|r| r["providerId"] == provider_id && r["modelId"] == b.model)
                    .map(|r| r["config"].clone())
                    .unwrap_or(json!({}));
                for (source, key) in [
                    ("contextWindow", "contextWindow"),
                    ("supportsImage", "supportsImage"),
                ] {
                    if let Some(value) = b.capabilities.get(source) {
                        if key == "supportsImage" {
                            let input = object_at_mut(&mut config, &["properties", "inputFormat"])?;
                            input.insert(key.into(), value.clone());
                        } else {
                            object_at_mut(&mut config, &["properties"])?
                                .insert(key.into(), value.clone());
                        }
                    }
                }
                if let Some(value) = b.capabilities.get("maxTokens") {
                    object_at_mut(&mut config, &["optionSpecs"])?
                        .insert("maxOutputTokens".into(), json!({"max":value}));
                }
                if let Some(value) = b.capabilities.get("reasoningEfforts") {
                    object_at_mut(&mut config, &["optionSpecs"])?
                        .insert("reasoningLevel".into(), json!({"values":value}));
                }
                rows.push(json!({"providerId":provider_id,"modelId":b.model,"config":config}));
            }
            changes.push(FileChange::plan(
                path,
                before.as_deref(),
                Some(formats::write_json(
                    before.as_deref().unwrap_or_default(),
                    &doc,
                )?),
            ));
            Ok(changes)
        }
        ClientId::DeepSeekHarness => super::harness_models::plan(target, &selected, port),
        ClientId::WebStorm if target.model_mode == "acp" => plan_acp(target, &selected, port),
        _ => anyhow::bail!("此客户端模型需通过它的专用配置服务保存"),
    }
}

fn acp_model(b: &Binding) -> Result<Value> {
    let mut model = json!({"name":b.label});
    let context = b.capabilities.get("contextWindow").and_then(Value::as_u64);
    let output = b.capabilities.get("maxTokens").and_then(Value::as_u64);
    if context.is_some() || output.is_some() {
        let (context, output) = (
            context.context("请在模型能力中同时填写上下文和输出上限")?,
            output.context("请在模型能力中同时填写上下文和输出上限")?,
        );
        ensure!(
            context > 0 && output > 0 && output <= context,
            "模型输出上限应大于零且不超过上下文上限"
        );
        model["limit"] = json!({"context":context,"output":output});
    }
    if let Some(images) = b.capabilities.get("supportsImage").and_then(Value::as_bool) {
        model["modalities"] =
            json!({"input":if images{vec!["text","image"]}else{vec!["text"]},"output":["text"]});
    }
    Ok(model)
}
fn plan_acp(target: &ClientTarget, bindings: &[&Binding], port: u16) -> Result<Vec<FileChange>> {
    use super::formats;
    let config = target.root.join("tiancai-opencode.json");
    let before = transaction::read(&config)?;
    let mut doc = formats::json(
        before.as_deref().unwrap_or_default(),
        json!({"$schema":"https://opencode.ai/config.json"}),
    )?;
    let providers = formats::object_at_mut(&mut doc, &["provider"])?;
    providers.retain(|k, _| !k.starts_with("tiancai-hub-"));
    for b in bindings {
        let model = acp_model(b)?;
        providers.insert(format!("tiancai-hub-{}",b.id),json!({"npm":"@ai-sdk/openai-compatible","name":b.label,"options":{"baseURL":base(b,port),"apiKey":b.local_token},"models":{b.model.clone():model}}));
    }
    let agent = target.root.join("acp.json");
    let agent_before = transaction::read(&agent)?;
    let mut agents = formats::json(agent_before.as_deref().unwrap_or_default(), json!({}))?;
    let servers = formats::object_at_mut(&mut agents, &["agent_servers"])?;
    if bindings.is_empty() {
        servers.remove("TianCaiSpaceHub");
    } else {
        let command = super::components::opencode()?;
        ensure!(
            super::components::status()?["opencode"]["installed"] == true,
            "请先在 WebStorm 接入设置中安装或修复 ACP 组件，再使用模型"
        );
        servers.insert("TianCaiSpaceHub".into(),json!({"command":command,"args":["acp"],"env":{"OPENCODE_CONFIG":config.to_string_lossy(),"OPENCODE_DISABLE_AUTOUPDATE":"true"}}));
    }
    Ok(vec![
        FileChange::plan(
            config,
            before.as_deref(),
            Some(formats::write_json(
                before.as_deref().unwrap_or_default(),
                &doc,
            )?),
        ),
        FileChange::plan(
            agent,
            agent_before.as_deref(),
            Some(formats::write_json(
                agent_before.as_deref().unwrap_or_default(),
                &agents,
            )?),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_entry_defaults_round_trip_and_preserve_explicit_client_choices() {
        let dir = tempfile::tempdir().unwrap();
        let target = super::super::targets::target(
            ClientId::VsCode,
            "fixture",
            dir.path().into(),
            dir.path().join("skills"),
            dir.path().join("mcp.json"),
            super::super::targets::McpFormat::Servers,
            "chat",
        );
        let source = ProviderConfig {
            name: "synthetic".into(),
            models: vec!["gpt-6".into()],
            ..Default::default()
        };
        let mut provider = copy(&source, &target, "gpt-6", "").unwrap();
        provider.desktop_binding.as_mut().unwrap().capabilities = json!({"defaultReasoningEffort":"xhigh","preferStream":true,"reasoningEfforts":["low","medium","high","xhigh","max"]});
        let encoded = serde_json::to_vec(&provider).unwrap();
        let provider: ProviderConfig = serde_json::from_slice(&encoded).unwrap();
        assert!(prefers_upstream_stream(&provider));
        let mut chat = json!({"model":"gpt-6","messages":[],"stream":false});
        apply_request_defaults(&mut chat, &provider, false);
        assert_eq!(chat["reasoning_effort"], "xhigh");
        assert_eq!(chat["stream"], false);
        let mut responses = json!({"model":"gpt-6","input":[]});
        apply_request_defaults(&mut responses, &provider, true);
        assert_eq!(responses["reasoning"]["effort"], "xhigh");
        for mut explicit in [
            json!({"reasoning_effort":"none"}),
            json!({"reasoning":{"effort":"low"}}),
            json!({"thinking":{"type":"disabled"}}),
        ] {
            let before = explicit.clone();
            apply_request_defaults(&mut explicit, &provider, false);
            assert_eq!(explicit, before);
        }
        let mut old = json!({"model":"gpt-6"});
        apply_request_defaults(&mut old, &source, true);
        assert!(old.get("reasoning").is_none());
        assert!(!prefers_upstream_stream(&source));
    }
    #[test]
    fn vscode_capabilities_keep_reasoning_and_existing_native_settings() {
        let dir = tempfile::tempdir().unwrap();
        let target = super::super::targets::target(
            ClientId::VsCode,
            "fixture",
            dir.path().into(),
            dir.path().join("skills"),
            dir.path().join("mcp.json"),
            super::super::targets::McpFormat::Servers,
            "chat",
        );
        let source = ProviderConfig {
            name: "source".into(),
            models: vec!["model".into()],
            ..Default::default()
        };
        let mut b = copy(&source, &target, "model", "")
            .unwrap()
            .desktop_binding
            .unwrap();
        b.capabilities = json!({"contextWindow":65536,"maxTokens":8192,"supportsImage":true,"reasoningEfforts":["low","high","max"]});
        assert_eq!(
            acp_model(&b).unwrap()["limit"],
            json!({"context":65536,"output":8192})
        );
        let mut incomplete = b.clone();
        incomplete.capabilities = json!({"contextWindow":65536});
        assert!(acp_model(&incomplete).is_err());
        incomplete.capabilities = json!({"contextWindow":1024,"maxTokens":2048});
        assert!(acp_model(&incomplete).is_err());
        let key = format!("TianCaiSpaceHub-{}", &target.id[..12]);
        std::fs::write(
            target.root.join("chatLanguageModels.json"),
            serde_json::to_vec(
                &json!([{"name":key,"settings":{"model":{"reasoningEffort":"max"}},"future":7}]),
            )
            .unwrap(),
        )
        .unwrap();
        let changes = plan(&target, &[b], 3210, true).unwrap();
        let doc =
            super::super::formats::json(changes[0].after.as_ref().unwrap(), Value::Null).unwrap();
        assert_eq!(doc[0]["settings"]["model"]["reasoningEffort"], "max");
        assert_eq!(doc[0]["future"], 7);
        assert_eq!(doc[0]["models"][0]["contextWindow"], 65536);
        assert_eq!(doc[0]["models"][0]["maxOutputTokens"], 8192);
        assert_eq!(doc[0]["models"][0]["vision"], true);
        assert_eq!(doc[0]["models"][0]["supportsReasoningEffort"][2], "max");
    }
    #[test]
    fn copies_have_independent_identity_and_do_not_route_as_ordinary_providers() {
        let dir = tempfile::tempdir().unwrap();
        let t = super::super::targets::target(
            ClientId::VsCode,
            "fixture",
            dir.path().into(),
            dir.path().join("skills"),
            dir.path().join("mcp.json"),
            super::super::targets::McpFormat::Servers,
            "chat",
        );
        let source = ProviderConfig {
            name: "source".into(),
            models: vec!["same".into()],
            ..Default::default()
        };
        let a = copy(&source, &t, "same", "").unwrap();
        let b = copy(&source, &t, "same", "").unwrap();
        assert_ne!(a.name, b.name);
        assert!(a.is_client_reserved());
        assert_ne!(provider_route_id(&a), provider_route_id(&b));
        let changes = plan(&t, &[a.desktop_binding.unwrap()], 3210, true).unwrap();
        let doc: Value = serde_json::from_slice(changes[0].after.as_ref().unwrap()).unwrap();
        assert!(
            doc[0]["models"][0]["url"]
                .as_str()
                .unwrap()
                .contains("/clients/vscode/")
        );
        assert!(source.desktop_binding.is_none());
    }
}
