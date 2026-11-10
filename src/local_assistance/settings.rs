//! Opt-in configuration. Never edits the user's Codex home or TianGong install.
use crate::{
    config::AppConfig,
    desktop::transaction::{self, FileChange, TransactionStore},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const MCP_NAME: &str = "tiancai-local-assistance";
pub fn root(path: &Path) -> PathBuf {
    crate::desktop::state_root(path).join("local-assistance")
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    pub model: String,
    pub route: String,
    pub reasoning_effort: Option<String>,
    pub binary: Option<PathBuf>,
    pub allow_workspace_write: bool,
    pub auto_clear: bool,
    // Local MCP credential, never returned in the UI.
    pub token: String,
    pub native_fingerprint: Option<String>,
    pub native_disable_pending: bool,
    pub native_catalog_pending: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            model: String::new(),
            route: String::new(),
            reasoning_effort: None,
            binary: None,
            allow_workspace_write: false,
            auto_clear: true,
            token: String::new(),
            native_fingerprint: None,
            native_disable_pending: false,
            native_catalog_pending: false,
        }
    }
}
pub fn read(path: &Path) -> Result<(Settings, String)> {
    let bytes = transaction::read(&root(path).join("settings.json"))?;
    let settings = bytes
        .as_ref()
        .map(|v| serde_json::from_slice(v))
        .transpose()?
        .unwrap_or_default();
    Ok((settings, transaction::fingerprint(bytes.as_deref())))
}
pub fn write(path: &Path, settings: &Settings, revision: &str) -> Result<()> {
    write_revision(path, settings, revision).map(|_| ())
}
pub(super) fn write_revision(path: &Path, settings: &Settings, revision: &str) -> Result<String> {
    let file = root(path).join("settings.json");
    let bytes = serde_json::to_vec(settings)?;
    let next_revision = transaction::fingerprint(Some(&bytes));
    TransactionStore::new(root(path).join("transactions"))?.apply(vec![FileChange {
        path: file,
        expected: revision.into(),
        after: Some(bytes),
    }])?;
    Ok(next_revision)
}
pub fn token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub fn models(config: &AppConfig) -> Vec<Value> {
    let mut out = Vec::new();
    for model in &config.ai_gateway.codex_visible_models {
        for provider in &config.ai_gateway.providers {
            if provider.enabled && !provider.is_client_reserved() && provider.matches_model(model) {
                let mut scoped = config.ai_gateway.clone();
                scoped.providers = vec![provider.clone()];
                scoped.codex_visible_models = vec![model.clone()];
                let (catalog, _) = crate::ai_gateway::catalog::configured_models_response_with_etag(&scoped);
                let descriptor = catalog["models"].as_array().and_then(|a| a.iter().find(|m|m["slug"] == *model));
                let no_reasoning = crate::desktop::tiangong_models::parameters_for_provider(provider).is_some()
                    || scoped.codex_model_profiles.iter().any(|p|p.id.eq_ignore_ascii_case(model) && p.supports_reasoning == Some(false));
                let efforts: Vec<&str> = if no_reasoning { vec![] } else { descriptor
                    .and_then(|m| m["supported_reasoning_levels"].as_array()).map(|a|a.iter().filter_map(|v|v["effort"].as_str())
                        .filter(|v|matches!(*v,"none"|"minimal"|"low"|"medium"|"high"|"xhigh"|"max"|"ultra")).collect()).unwrap_or_default() };
                let default = if efforts.contains(&"xhigh") { Some("xhigh") } else {
                    descriptor.and_then(|m|m["default_reasoning_level"].as_str()).filter(|v|efforts.contains(v)).or_else(||efforts.first().copied())
                };
                out.push(json!({"model":model,"route":crate::ai_gateway::config::provider_route_id(provider),
                    "label":format!("{} · {}",model,provider.name),"efforts":efforts,"defaultEffort":default}));
            }
        }
    }
    out
}
pub fn resolved_effort(config: &AppConfig, settings: &Settings) -> Result<Option<String>> {
    let models = models(config);
    let model = models.iter().find(|m|m["model"] == settings.model && m["route"] == settings.route)
        .context("协助模型已变化，请重新选择")?;
    if let Some(effort) = &settings.reasoning_effort {
        ensure!(model["efforts"].as_array().is_some_and(|a|a.iter().any(|v|v == effort)),
            "协助模型当前未提供已保存的思考强度 {effort}，请重新选择；未自动降低强度");
        return Ok(Some(effort.clone()));
    }
    Ok(model["defaultEffort"].as_str().map(str::to_string))
}
pub fn validate(config: &AppConfig, settings: &Settings) -> Result<()> {
    if !settings.enabled {
        return Ok(());
    }
    ensure!(config.ai_gateway.enabled, "请先启用模型接入服务");
    ensure!(
        models(config)
            .iter()
            .any(|m| m["model"] == settings.model && m["route"] == settings.route),
        "协助模型已变化，请先在 ChatGPT / Codex 模型中接入，再重新选择"
    );
    resolved_effort(config, settings)?;
    resolve_binary(settings.binary.as_deref())?;
    Ok(())
}
pub(crate) fn source_fingerprint(
    provider: &crate::ai_gateway::config::ProviderConfig,
) -> Result<String> {
    let mut stable = provider.clone();
    stable.base_url = provider.identity_base_url().into();
    stable.base_url_repair_origin = None;
    crate::desktop::models::fingerprint(&stable)
}

/// Discovery only; no --version/help/schema subprocess is launched.
pub fn resolve_binary(selected: Option<&Path>) -> Result<PathBuf> {
    super::binary::resolve(selected)
}

pub fn native_value(settings: &Settings, port: u16) -> Value {
    json!({"server_id":MCP_NAME,"description":"遇到连续失败、能力不足或关键验证失败时，自动请求本机协助；需要有效的天工任务上下文。审批和询问只在天工处理。",
        "server_url":format!("http://127.0.0.1:{port}/api/local-assistance/mcp"),"connect_type":1,"timeout_ms":30000,
        "token_encrypted":"","header_config":{"Authorization":format!("Bearer {}",settings.token)},
        "config_param":{"transport":"streamable","requestTimeout":30,"maximumTotalTimeout":60},"status":"active",
        "tools_json":[],"is_connected":0,"conn_last_error":"","retry_count":0})
}
pub fn native_fingerprint(value: &Value) -> String {
    transaction::fingerprint(Some(
        &serde_json::to_vec(&native_fields(value)).unwrap_or_default(),
    ))
}
pub fn native_fields(value: &Value) -> Value {
    let mut v = json!({});
    for key in [
        "server_id",
        "description",
        "server_url",
        "connect_type",
        "timeout_ms",
        "token_encrypted",
        "header_config",
        "config_param",
        "status",
    ] {
        if let Some(raw) = value.get(key) {
            v[key] = if matches!(key, "header_config" | "config_param") {
                raw.as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(raw.clone())
            } else {
                raw.clone()
            };
        }
    }
    v
}
