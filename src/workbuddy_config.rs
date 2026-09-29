use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ai_gateway::config::{
        ProviderConfig, ProviderType, WORKBUDDY_PROVIDER_NAME, provider_display_base_url,
    },
    config::AppConfig,
};

/// The local endpoint and token written to WorkBuddy's single-model config.
/// The Hub currently does not require the token for local requests, but a
/// stable non-empty value keeps clients that validate their config happy.
pub const DEFAULT_WORKBUDDY_URL: &str = "http://127.0.0.1:3847/ai-gateway/v1";
pub const DEFAULT_WORKBUDDY_API_KEY: &str = "workbuddy-local";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkBuddyReasoningConfig {
    pub default_effort: String,
    pub supported_efforts: Vec<String>,
    pub can_disable_thinking: bool,
}

impl Default for WorkBuddyReasoningConfig {
    fn default() -> Self {
        Self {
            default_effort: "high".to_string(),
            supported_efforts: vec!["low", "medium", "high", "xhigh"]
                .into_iter()
                .map(String::from)
                .collect(),
            can_disable_thinking: false,
        }
    }
}

impl WorkBuddyReasoningConfig {
    /// WorkBuddy exposes the same five effort levels for current Claude models.
    /// Resolve provider aliases before calling this so mixed-model providers work.
    pub fn for_model(
        protocol: &str,
        model: &str,
        compatibility: Option<&str>,
        preferred_effort: &str,
    ) -> Self {
        let name = model
            .trim()
            .rsplit('/')
            .next()
            .unwrap_or(model)
            .to_ascii_lowercase();
        let glm = protocol == "anthropic-messages"
            && matches!(
                compatibility.map(str::trim),
                Some("glm_anthropic" | "zhipu_anthropic")
            );
        let claude = protocol == "anthropic-messages"
            || ["claude", "opus", "sonnet", "haiku"]
                .iter()
                .any(|prefix| name == *prefix || name.starts_with(&format!("{prefix}-")));
        let efforts = if glm {
            vec!["high", "max"]
        } else if claude {
            vec!["low", "medium", "high", "xhigh", "max"]
        } else {
            vec!["low", "medium", "high", "xhigh"]
        };
        let preferred = preferred_effort.trim().to_ascii_lowercase();
        Self {
            default_effort: if efforts.contains(&preferred.as_str()) {
                preferred
            } else {
                "high".to_string()
            },
            supported_efforts: efforts.into_iter().map(String::from).collect(),
            can_disable_thinking: false,
        }
    }
}

pub fn uses_prompt_cache_key(protocol: &str) -> bool {
    protocol != "anthropic-messages"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkBuddyModelConfig {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub url: String,
    pub api_key: String,
    pub upstream_url: String,
    pub upstream_api_key: String,
    pub upstream_provider: String,
    pub upstream_protocol: String,
    pub provider_model: String,
    pub upstream_auth: String,
    pub upstream_default_reasoning_effort: String,
    pub supports_tool_call: bool,
    pub supports_images: bool,
    pub supports_reasoning: bool,
    pub use_custom_protocol: bool,
    pub reasoning: WorkBuddyReasoningConfig,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for WorkBuddyModelConfig {
    fn default() -> Self {
        let mut extra = BTreeMap::new();
        extra.insert(
            "cacheKey".to_string(),
            Value::String("workbuddy:default".to_string()),
        );
        Self {
            id: "gpt-5.6-sol".to_string(),
            name: "gpt-5.6-sol".to_string(),
            vendor: "Custom".to_string(),
            url: DEFAULT_WORKBUDDY_URL.to_string(),
            api_key: DEFAULT_WORKBUDDY_API_KEY.to_string(),
            upstream_url: String::new(),
            upstream_api_key: String::new(),
            upstream_provider: String::new(),
            upstream_protocol: "openai-responses".to_string(),
            provider_model: "gpt-5.6-sol".to_string(),
            upstream_auth: "bearer".to_string(),
            upstream_default_reasoning_effort: "high".to_string(),
            supports_tool_call: true,
            supports_images: true,
            supports_reasoning: true,
            use_custom_protocol: false,
            reasoning: WorkBuddyReasoningConfig::default(),
            extra,
        }
    }
}

/// Builds the stable cache namespace used by the WorkBuddy compatibility
/// route.  The provider, rather than the selected model, identifies the
/// upstream prompt prefix so switching models within one provider keeps the
/// same cache namespace.
pub fn default_cache_key_for_provider(provider: &str) -> String {
    let mut normalized = String::new();
    for character in provider.trim().chars() {
        if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
            normalized.extend(character.to_lowercase());
        } else if !normalized.ends_with('_') {
            normalized.push('_');
        }
    }
    let normalized = normalized.trim_matches('_');
    if normalized.is_empty() {
        "workbuddy:default".to_string()
    } else {
        format!("workbuddy:{normalized}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyConfigStatus {
    pub path: String,
    pub exists: bool,
    pub backup_path: String,
    pub backup_exists: bool,
    pub model: WorkBuddyModelConfig,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WorkBuddyConfigFile {
    Models(Vec<WorkBuddyModelConfig>),
    LegacyModel(WorkBuddyModelConfig),
}

pub fn config_path() -> PathBuf {
    if let Some(path) = env::var_os("WORKBUDDY_CONFIG_PATH") {
        return PathBuf::from(path);
    }
    let home = if cfg!(target_os = "windows") {
        env::var_os("USERPROFILE")
    } else {
        env::var_os("HOME")
    }
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from("."));
    home.join(".workbuddy").join("models.json")
}

pub fn load() -> Result<WorkBuddyConfigStatus> {
    load_at(&config_path())
}

fn status_for(path: &Path, exists: bool, model: WorkBuddyModelConfig) -> WorkBuddyConfigStatus {
    let backup = backup_path_for(path);
    WorkBuddyConfigStatus {
        path: path.to_string_lossy().to_string(),
        exists,
        backup_path: backup.to_string_lossy().to_string(),
        backup_exists: backup.exists(),
        model,
    }
}

fn load_at(path: &Path) -> Result<WorkBuddyConfigStatus> {
    if !path.exists() {
        return Ok(status_for(path, false, WorkBuddyModelConfig::default()));
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("read WorkBuddy config {}", path.display()))?;
    let model = parse_config_text(&text)
        .with_context(|| format!("parse WorkBuddy config {}", path.display()))?;
    Ok(status_for(path, true, model))
}

fn parse_config_text(text: &str) -> Result<WorkBuddyModelConfig> {
    let config: WorkBuddyConfigFile =
        serde_json::from_str(text).context("invalid WorkBuddy models JSON")?;
    match config {
        WorkBuddyConfigFile::Models(mut models) => {
            if models.is_empty() {
                Err(anyhow!(
                    "WorkBuddy models.json must contain at least one model"
                ))
            } else {
                Ok(models.remove(0))
            }
        }
        WorkBuddyConfigFile::LegacyModel(model) => Ok(model),
    }
}

fn serialize_config(model: &WorkBuddyModelConfig) -> Result<String> {
    let mut model = model.clone();
    if !uses_prompt_cache_key(&model.upstream_protocol) {
        model.extra.remove("cacheKey");
        model.extra.remove("cache_key");
    }
    serde_json::to_string_pretty(&[model]).context("serialize WorkBuddy config")
}

fn backup_path_for(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.bak",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("models.json")
    ))
}

pub fn backup_exists() -> bool {
    backup_path_for(&config_path()).is_file()
}

fn restore_safety_path_for(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.before-restore.bak",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("models.json")
    ))
}

fn write_file_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("cannot write path without a parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("create directory {}", parent.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("create temporary file in {}", parent.display()))?;
    temporary
        .write_all(contents)
        .with_context(|| format!("write temporary file for {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("sync temporary file for {}", path.display()))?;
    let persisted = temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("replace {}", path.display()))?;
    persisted
        .sync_all()
        .with_context(|| format!("sync {}", path.display()))?;
    Ok(())
}

/// Returns the stable cache namespace for the requested WorkBuddy model.
/// WorkBuddy currently stores one model object, but matching by all three
/// common identifiers keeps this compatible with hand-edited files and future
/// multi-model layouts. A configured provider is authoritative so older
/// model-based cache keys are migrated automatically. Anthropic also uses this
/// namespace internally for session context, but never sends it as an OpenAI
/// prompt_cache_key or requires a cacheKey in models.json.
pub fn configured_cache_key(model: &str) -> Option<String> {
    let status = load().ok()?;
    let configured = &status.model;
    if !configured_model_matches(configured, model) {
        return None;
    }
    let configured_key = configured
        .extra
        .get("cacheKey")
        .or_else(|| configured.extra.get("cache_key"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if !configured.upstream_provider.trim().is_empty() {
        // The GUI-managed provider namespace is authoritative. This also
        // migrates older model-based `workbuddy:<model>` values on first use.
        Some(default_cache_key_for_provider(
            &configured.upstream_provider,
        ))
    } else {
        configured_key
    }
}

/// Returns the configured default when WorkBuddy omitted `reasoning_effort`.
/// An explicit effort in the request always wins, so changing effort in
/// WorkBuddy continues to select the corresponding upstream value.
pub fn configured_default_reasoning_effort(model: &str) -> Option<String> {
    let status = load().ok()?;
    let configured = &status.model;
    if !configured_model_matches(configured, model) {
        return None;
    }
    [
        configured.upstream_default_reasoning_effort.as_str(),
        configured.reasoning.default_effort.as_str(),
    ]
    .into_iter()
    .map(str::trim)
    .find(|value| !value.is_empty())
    .map(str::to_string)
}

fn configured_model_matches(configured: &WorkBuddyModelConfig, model: &str) -> bool {
    [
        configured.id.as_str(),
        configured.name.as_str(),
        configured.provider_model.as_str(),
    ]
    .iter()
    .any(|candidate| candidate.eq_ignore_ascii_case(model))
}

/// Updates only the provider reserved for WorkBuddy. Other providers and all
/// non-gateway application settings remain untouched.
pub fn apply_provider(config: &mut AppConfig, model: &WorkBuddyModelConfig) {
    let provider_type = match model.upstream_protocol.as_str() {
        "openai-chat" => ProviderType::ChatCompletions,
        "anthropic-messages" => ProviderType::AnthropicMessages,
        _ => ProviderType::OpenAiResponses,
    };
    let mut provider = config
        .ai_gateway
        .providers
        .iter()
        .find(|item| {
            !item.is_workbuddy()
                && item
                    .name
                    .eq_ignore_ascii_case(model.upstream_provider.trim())
        })
        .cloned()
        .unwrap_or_default();
    provider.name = WORKBUDDY_PROVIDER_NAME.to_string();
    provider.enabled = true;
    // Account channels use the Responses wire format but must retain their
    // credential reference and OAuth refresh behavior when cloned for WorkBuddy.
    provider.provider_type = if provider_type == ProviderType::OpenAiResponses
        && provider.provider_type == ProviderType::ChatGptResponses
    {
        ProviderType::ChatGptResponses
    } else {
        provider_type
    };
    provider.base_url = if provider.provider_type == ProviderType::ChatGptResponses {
        crate::ai_gateway::chatgpt_auth::BASE_URL.to_string()
    } else {
        provider.chatgpt_auth_id = None;
        provider_display_base_url(&model.upstream_url)
    };
    provider.api_key = model.upstream_api_key.clone();
    provider.models = vec![model.provider_model.clone()];
    if !matches!(&provider.provider_type, ProviderType::AnthropicMessages) {
        provider.compatibility = None;
    }
    if model.upstream_provider.trim().is_empty() {
        provider.compatibility = None;
        provider.models_url = None;
        provider.model_aliases.clear();
    }
    let provider = ProviderConfig {
        name: WORKBUDDY_PROVIDER_NAME.to_string(),
        enabled: true,
        ..provider
    };
    if let Some(existing) = config
        .ai_gateway
        .providers
        .iter_mut()
        .find(|item| item.is_workbuddy())
    {
        *existing = provider;
    } else {
        config.ai_gateway.providers.push(provider);
    }
    config.ai_gateway.enabled = true;
}

pub fn save(model: &WorkBuddyModelConfig) -> Result<WorkBuddyConfigStatus> {
    save_at(&config_path(), model)
}

fn save_at(path: &Path, model: &WorkBuddyModelConfig) -> Result<WorkBuddyConfigStatus> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("WorkBuddy config path has no parent"))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create WorkBuddy config directory {}", parent.display()))?;
    let json = serialize_config(model)?;
    let contents = format!("{json}\n");
    let backup = backup_path_for(path);
    if path.exists() {
        let previous = fs::read(path)
            .with_context(|| format!("read WorkBuddy config {} before backup", path.display()))?;
        write_file_atomically(&backup, &previous).with_context(|| {
            format!(
                "backup existing WorkBuddy config {} to {}",
                path.display(),
                backup.display()
            )
        })?;
    }
    write_file_atomically(path, contents.as_bytes())
        .with_context(|| format!("write WorkBuddy config {}", path.display()))?;
    Ok(status_for(path, true, parse_config_text(&json)?))
}

pub fn restore_backup() -> Result<WorkBuddyConfigStatus> {
    restore_backup_at(&config_path())
}

fn restore_backup_at(path: &Path) -> Result<WorkBuddyConfigStatus> {
    let backup = backup_path_for(path);
    if !backup.exists() {
        return Err(anyhow!(
            "no WorkBuddy backup is available at {}",
            backup.display()
        ));
    }
    let backup_contents =
        fs::read(&backup).with_context(|| format!("read WorkBuddy backup {}", backup.display()))?;
    let backup_text =
        std::str::from_utf8(&backup_contents).context("WorkBuddy backup is not valid UTF-8")?;
    parse_config_text(backup_text)
        .map_err(|error| anyhow!("parse WorkBuddy backup {}: {error:#}", backup.display()))?;

    if path.exists() {
        let current = fs::read(path).with_context(|| {
            format!(
                "read current WorkBuddy config {} before restore",
                path.display()
            )
        })?;
        let safety_path = restore_safety_path_for(path);
        write_file_atomically(&safety_path, &current).with_context(|| {
            format!(
                "protect current WorkBuddy config in {}",
                safety_path.display()
            )
        })?;
    }
    write_file_atomically(path, &backup_contents)
        .with_context(|| format!("restore WorkBuddy backup to {}", path.display()))?;
    load_at(path)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::*;
    use crate::ai_gateway::config::AiGatewayConfig;

    fn codex_provider() -> ProviderConfig {
        ProviderConfig {
            name: "existing-codex".to_string(),
            base_url: "https://codex.example/v1".to_string(),
            api_key: "codex-secret".to_string(),
            models: vec!["gpt-5.6-sol".to_string()],
            weight: 777,
            ..ProviderConfig::default()
        }
    }

    #[test]
    fn apply_provider_preserves_codex_and_non_gateway_configuration() {
        let existing = codex_provider();
        let mut config = AppConfig {
            bind: "127.0.0.1:4999".to_string(),
            language: Some("zh-CN".to_string()),
            theme: Some("dark".to_string()),
            ai_gateway: AiGatewayConfig {
                enabled: false,
                prompt_cache_retention: Some("24h".to_string()),
                providers: vec![existing.clone()],
                codex_visible_models: vec!["gpt-5.6-sol".to_string()],
                filter_image_generation_tool: true,
                request_logging_enabled: true,
                request_log_details_enabled: true,
                ..AiGatewayConfig::default()
            },
            ..AppConfig::default()
        };
        let model = WorkBuddyModelConfig {
            upstream_url: "https://workbuddy.example/v1".to_string(),
            upstream_api_key: "workbuddy-secret".to_string(),
            ..WorkBuddyModelConfig::default()
        };

        let mut before_top_level = serde_json::to_value(&config).unwrap();
        before_top_level
            .as_object_mut()
            .unwrap()
            .remove("aiGateway");
        let existing_json = serde_json::to_value(&existing).unwrap();

        apply_provider(&mut config, &model);

        let mut after_top_level = serde_json::to_value(&config).unwrap();
        after_top_level.as_object_mut().unwrap().remove("aiGateway");
        assert_eq!(after_top_level, before_top_level);
        assert_eq!(
            serde_json::to_value(&config.ai_gateway.providers[0]).unwrap(),
            existing_json
        );
        assert_eq!(config.ai_gateway.providers.len(), 2);
        assert!(config.ai_gateway.providers[1].is_workbuddy());
        assert_eq!(
            config.ai_gateway.prompt_cache_retention.as_deref(),
            Some("24h")
        );
        assert_eq!(config.ai_gateway.codex_visible_models, vec!["gpt-5.6-sol"]);
        assert!(config.ai_gateway.filter_image_generation_tool);
        assert!(config.ai_gateway.request_logging_enabled);
        assert!(config.ai_gateway.request_log_details_enabled);
        assert!(config.ai_gateway.enabled);
    }

    #[test]
    fn apply_provider_replaces_only_existing_workbuddy_provider() {
        let existing = codex_provider();
        let old_workbuddy = ProviderConfig {
            name: "WorkBuddy".to_string(),
            base_url: "https://old.example/v1".to_string(),
            models: vec!["old-model".to_string()],
            ..ProviderConfig::default()
        };
        let mut config = AppConfig {
            ai_gateway: AiGatewayConfig {
                providers: vec![existing.clone(), old_workbuddy],
                ..AiGatewayConfig::default()
            },
            ..AppConfig::default()
        };
        let model = WorkBuddyModelConfig {
            upstream_url: "https://new.example".to_string(),
            upstream_protocol: "anthropic-messages".to_string(),
            ..WorkBuddyModelConfig::default()
        };

        apply_provider(&mut config, &model);

        assert_eq!(config.ai_gateway.providers.len(), 2);
        assert_eq!(
            serde_json::to_value(&config.ai_gateway.providers[0]).unwrap(),
            serde_json::to_value(&existing).unwrap()
        );
        let workbuddy = &config.ai_gateway.providers[1];
        assert_eq!(workbuddy.name, WORKBUDDY_PROVIDER_NAME);
        assert_eq!(workbuddy.base_url, "https://new.example/v1");
        assert_eq!(workbuddy.provider_type, ProviderType::AnthropicMessages);
    }

    #[test]
    fn apply_provider_clones_selected_upstream_compatibility_settings() {
        let source = ProviderConfig {
            name: "claude-compatible".to_string(),
            provider_type: ProviderType::AnthropicMessages,
            compatibility: Some("glm_anthropic".to_string()),
            base_url: "https://source.example/v1".to_string(),
            api_key: "source-key".to_string(),
            models: vec!["model-a".to_string(), "model-b".to_string()],
            prompt_cache_retention: Some("1h".to_string()),
            timeout_secs: 321,
            ..ProviderConfig::default()
        };
        let mut config = AppConfig {
            ai_gateway: AiGatewayConfig {
                providers: vec![source],
                ..AiGatewayConfig::default()
            },
            ..AppConfig::default()
        };
        let model = WorkBuddyModelConfig {
            upstream_provider: "claude-compatible".to_string(),
            upstream_protocol: "anthropic-messages".to_string(),
            upstream_url: "https://source.example/v1".to_string(),
            upstream_api_key: "source-key".to_string(),
            provider_model: "model-b".to_string(),
            ..WorkBuddyModelConfig::default()
        };

        apply_provider(&mut config, &model);

        let workbuddy = config
            .ai_gateway
            .providers
            .iter()
            .find(|provider| provider.is_workbuddy())
            .unwrap();
        assert_eq!(workbuddy.compatibility.as_deref(), Some("glm_anthropic"));
        assert_eq!(workbuddy.prompt_cache_retention.as_deref(), Some("1h"));
        assert_eq!(workbuddy.timeout_secs, 321);
        assert_eq!(workbuddy.models, vec!["model-b"]);
        assert_eq!(config.ai_gateway.providers[0].name, "claude-compatible");
    }

    #[test]
    fn account_provider_survives_workbuddy_roundtrip_and_protocol_switch() {
        let source = ProviderConfig {
            name: "signed-in-account".into(),
            provider_type: ProviderType::ChatGptResponses,
            chatgpt_auth_id: Some("test-credential-reference".into()),
            base_url: crate::ai_gateway::chatgpt_auth::BASE_URL.into(),
            models: vec!["gpt-6-luna".into()],
            model_aliases: [("coding".into(), "gpt-6-luna".into())].into(),
            ..Default::default()
        };
        let source_json = serde_json::to_value(&source).unwrap();
        let mut config = AppConfig::default();
        config.ai_gateway.providers.push(source);
        let model = WorkBuddyModelConfig {
            upstream_provider: "signed-in-account".into(),
            upstream_protocol: "openai-responses".into(),
            upstream_url: "https://ignored.example/v1".into(),
            provider_model: "coding".into(),
            ..Default::default()
        };
        let mut restored = parse_config_text(&serialize_config(&model).unwrap()).unwrap();
        apply_provider(&mut config, &restored);
        let workbuddy = &config.ai_gateway.providers[1];
        assert!(workbuddy.is_workbuddy());
        assert_eq!(workbuddy.provider_type, ProviderType::ChatGptResponses);
        assert_eq!(
            workbuddy.chatgpt_auth_id.as_deref(),
            Some("test-credential-reference")
        );
        assert_eq!(
            workbuddy.base_url,
            crate::ai_gateway::chatgpt_auth::BASE_URL
        );
        assert_eq!(workbuddy.models, ["coding"]);
        assert_eq!(workbuddy.model_aliases["coding"], "gpt-6-luna");

        restored.upstream_protocol = "openai-chat".into();
        restored.upstream_url = "https://api.example".into();
        restored.upstream_api_key = "api-key".into();
        apply_provider(&mut config, &restored);
        assert_eq!(config.ai_gateway.providers.len(), 2);
        let workbuddy = &config.ai_gateway.providers[1];
        assert_eq!(workbuddy.provider_type, ProviderType::ChatCompletions);
        assert!(workbuddy.chatgpt_auth_id.is_none());
        assert_eq!(workbuddy.base_url, "https://api.example/v1");
        assert_eq!(workbuddy.api_key, "api-key");
        assert_eq!(
            serde_json::to_value(&config.ai_gateway.providers[0]).unwrap(),
            source_json
        );
    }

    #[test]
    fn models_json_uses_array_shape_and_keeps_reasoning_and_cache_fields() {
        let mut model = WorkBuddyModelConfig::default();
        model
            .extra
            .insert("cacheKey".to_string(), json!("stable-cache"));

        let json = serialize_config(&model).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();

        assert!(value.is_array());
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["reasoning"]["defaultEffort"], "high");
        assert_eq!(
            value[0]["reasoning"]["supportedEfforts"],
            json!(["low", "medium", "high", "xhigh"])
        );
        assert_eq!(value[0]["cacheKey"], "stable-cache");
    }

    #[test]
    fn workbuddy_reasoning_follows_model_and_protocol_without_stale_max() {
        let claude =
            WorkBuddyReasoningConfig::for_model("anthropic-messages", "custom", None, "max");
        assert_eq!(
            claude.supported_efforts,
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(claude.default_effort, "max");
        let gpt = WorkBuddyReasoningConfig::for_model(
            "openai-responses",
            "gpt-6-luna",
            None,
            &claude.default_effort,
        );
        assert_eq!(gpt.default_effort, "high");
        assert_eq!(gpt.supported_efforts, ["low", "medium", "high", "xhigh"]);
        for name in ["claude-opus-4-8", "vendor/Claude-Sonnet-new", "opus-4-8"] {
            let config = WorkBuddyReasoningConfig::for_model("openai-chat", name, None, "xhigh");
            assert_eq!(config.supported_efforts, claude.supported_efforts);
            assert_eq!(config.default_effort, "xhigh");
        }
        for profile in ["glm_anthropic", "zhipu_anthropic"] {
            let glm = WorkBuddyReasoningConfig::for_model(
                "anthropic-messages",
                "glm-5",
                Some(profile),
                "xhigh",
            );
            assert_eq!(glm.supported_efforts, ["high", "max"]);
            assert_eq!(glm.default_effort, "high");
        }
    }

    #[test]
    fn workbuddy_anthropic_save_removes_legacy_cache_keys_and_preserves_five_efforts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        let mut model = WorkBuddyModelConfig {
            upstream_provider: "anthropic".to_string(),
            upstream_protocol: "anthropic-messages".to_string(),
            provider_model: "claude-opus-new".to_string(),
            upstream_default_reasoning_effort: "max".to_string(),
            reasoning: WorkBuddyReasoningConfig::for_model(
                "anthropic-messages",
                "claude-opus-new",
                None,
                "max",
            ),
            ..Default::default()
        };
        model.extra.insert("cache_key".into(), json!("old-key"));
        model.extra.insert("customField".into(), json!("keep"));
        let saved = save_at(&path, &model).unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(value[0].get("cacheKey").is_none());
        assert!(value[0].get("cache_key").is_none());
        assert_eq!(value[0]["customField"], "keep");
        assert_eq!(saved.model.reasoning.default_effort, "max");
        let loaded = load_at(&path).unwrap().model;
        assert_eq!(
            loaded.reasoning.supported_efforts,
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert!(!loaded.extra.contains_key("cacheKey"));
        assert_eq!(loaded.upstream_default_reasoning_effort, "max");
        assert!(!uses_prompt_cache_key(&loaded.upstream_protocol));
        assert!(uses_prompt_cache_key("openai-chat"));
        assert!(uses_prompt_cache_key("openai-responses"));
    }

    #[test]
    fn config_loader_accepts_array_and_legacy_object() {
        let model = WorkBuddyModelConfig {
            provider_model: "test-model".to_string(),
            upstream_provider: "test-provider".to_string(),
            ..WorkBuddyModelConfig::default()
        };
        let array = serde_json::to_string(&[&model]).unwrap();
        let legacy = serde_json::to_string(&model).unwrap();

        assert_eq!(
            parse_config_text(&array).unwrap().provider_model,
            "test-model"
        );
        assert_eq!(
            parse_config_text(&legacy).unwrap().upstream_provider,
            "test-provider"
        );
    }

    #[test]
    fn empty_models_array_is_rejected() {
        let error = parse_config_text("[]").unwrap_err().to_string();
        assert!(error.contains("at least one model"));
    }

    #[test]
    fn default_cache_key_is_stable_per_provider_and_not_model() {
        assert_eq!(
            default_cache_key_for_provider("Tiancai Space"),
            "workbuddy:tiancai_space"
        );
        assert_eq!(
            default_cache_key_for_provider("Tiancai Space"),
            default_cache_key_for_provider("Tiancai Space")
        );
        assert_ne!(
            default_cache_key_for_provider("openai"),
            default_cache_key_for_provider("anthropic")
        );
        assert_eq!(default_cache_key_for_provider(""), "workbuddy:default");
    }

    #[test]
    fn save_backs_up_existing_config_before_writing_array() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("models.json");
        let previous = "{\n  \"providerModel\": \"old-model\"\n}\n";
        fs::write(&path, previous).expect("write previous config");

        let model = WorkBuddyModelConfig {
            provider_model: "new-model".to_string(),
            ..WorkBuddyModelConfig::default()
        };
        let status = save_at(&path, &model).expect("save config");

        assert!(status.backup_exists);
        assert_eq!(
            fs::read(backup_path_for(&path)).unwrap(),
            previous.as_bytes()
        );
        let saved: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(saved.is_array());
        assert_eq!(saved[0]["providerModel"], "new-model");
    }

    #[test]
    fn first_save_succeeds_without_creating_a_backup() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("models.json");
        let model = WorkBuddyModelConfig::default();

        let status = save_at(&path, &model).expect("first save");

        assert!(status.exists);
        assert!(!status.backup_exists);
        assert!(!backup_path_for(&path).exists());
    }

    #[test]
    fn restore_validates_backup_and_protects_current_config() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("models.json");
        let old_model = WorkBuddyModelConfig {
            provider_model: "old-model".to_string(),
            ..WorkBuddyModelConfig::default()
        };
        let new_model = WorkBuddyModelConfig {
            provider_model: "new-model".to_string(),
            ..WorkBuddyModelConfig::default()
        };
        save_at(&path, &old_model).expect("save old model");
        save_at(&path, &new_model).expect("save new model");

        let restored = restore_backup_at(&path).expect("restore backup");

        assert_eq!(restored.model.provider_model, "old-model");
        assert_eq!(load_at(&path).unwrap().model.provider_model, "old-model");
        let safety_path = restore_safety_path_for(&path);
        let safety: Value =
            serde_json::from_str(&fs::read_to_string(safety_path).unwrap()).unwrap();
        assert_eq!(safety[0]["providerModel"], "new-model");
        assert_eq!(
            fs::read(backup_path_for(&path)).unwrap(),
            fs::read(&path).unwrap()
        );
    }

    #[test]
    fn restore_accepts_legacy_single_object_backup() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("models.json");
        let backup = backup_path_for(&path);
        let model = WorkBuddyModelConfig {
            provider_model: "legacy-model".to_string(),
            ..WorkBuddyModelConfig::default()
        };
        fs::write(&backup, serde_json::to_string(&model).unwrap()).expect("write legacy backup");

        let restored = restore_backup_at(&path).expect("restore legacy backup");

        assert_eq!(restored.model.provider_model, "legacy-model");
        let raw: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(raw.is_object());
    }

    #[test]
    fn restore_rejects_empty_backup_without_changing_current_config() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("models.json");
        let current = WorkBuddyModelConfig {
            provider_model: "current-model".to_string(),
            ..WorkBuddyModelConfig::default()
        };
        save_at(&path, &current).expect("save current model");
        fs::write(backup_path_for(&path), "[]").expect("write empty backup");

        let error = restore_backup_at(&path).unwrap_err().to_string();

        assert!(error.contains("at least one model"));
        assert_eq!(
            load_at(&path).unwrap().model.provider_model,
            "current-model"
        );
    }
}
