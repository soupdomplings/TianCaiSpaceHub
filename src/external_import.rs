//! External imports keep tickets and unconfirmed credentials in memory only.
use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::ai_gateway::config::{
    ProviderConfig, ProviderType, is_gmclaw_provider_name, is_workbuddy_provider_name,
    provider_display_base_url,
};
use crate::config::AppConfig;

pub mod client;
#[cfg(feature = "gui")]
pub mod ipc;
pub mod registration;
#[cfg(test)]
mod tests;

pub const MAX_LINK_BYTES: usize = 8192;
pub const MAX_MODELS: usize = 2048;
#[cfg(test)]
pub const OFFICIAL_ORIGIN: &str = "https://tiancai.yc99.space";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportLink {
    pub origin: String,
    pub ticket: String,
}

impl std::fmt::Debug for ImportLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ImportLink(<redacted>)")
    }
}

fn valid_percent_encoding(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit()
            {
                return false;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    true
}

impl ImportLink {
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > MAX_LINK_BYTES
            || !raw.is_ascii()
            || raw.bytes().any(|c| c <= 32 || c == 127)
            || raw.contains('\\')
            || !valid_percent_encoding(raw)
            || raw.split('?').next() != Some("tiancaispacehub://import/v1")
        {
            return Err("导入链接格式无效 / Invalid import link".into());
        }
        let url = Url::parse(raw).map_err(|_| "导入链接格式无效 / Invalid import link")?;
        if url.scheme() != "tiancaispacehub"
            || url.host_str() != Some("import")
            || url.path() != "/v1"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
        {
            return Err("不支持的导入链接或版本 / Unsupported import link or version".into());
        }
        let mut fields = BTreeMap::new();
        for (key, value) in url.query_pairs() {
            if !matches!(key.as_ref(), "origin" | "ticket")
                || fields
                    .insert(key.into_owned(), value.into_owned())
                    .is_some()
            {
                return Err(
                    "导入链接包含重复或未知字段 / Duplicate or unknown import field".into(),
                );
            }
        }
        let origin = fields
            .remove("origin")
            .ok_or("缺少导入来源 / Missing origin")?;
        let ticket = fields
            .remove("ticket")
            .ok_or("缺少导入码 / Missing ticket")?;
        if !(43..=512).contains(&ticket.len())
            || !ticket
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err("导入码格式无效 / Invalid ticket".into());
        }
        Ok(Self {
            origin: validate_origin(&origin)?,
            ticket,
        })
    }

    pub fn identity(&self) -> String {
        hex::encode(Sha256::digest(format!("{}\0{}", self.origin, self.ticket)))
    }
}

pub fn validate_endpoint(raw: &str) -> Result<Url, String> {
    if raw.len() > 2048
        || raw.trim() != raw
        || raw.chars().any(char::is_control)
        || raw.contains('\\')
        || !valid_percent_encoding(raw)
    {
        return Err("接口地址格式无效 / Invalid endpoint".into());
    }
    let url = Url::parse(raw).map_err(|_| "接口地址格式无效 / Invalid endpoint")?;
    // Compatible sites may use local, LAN, or custom HTTP endpoints. Address
    // policy must be identical in every process, with no domain allowlist.
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(
            "接口地址须为有效的 HTTP 或 HTTPS URL / Expected an HTTP or HTTPS endpoint".into(),
        );
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(
            "接口地址不能包含用户名密码、查询参数或片段 / Endpoint credentials, query and fragment are not allowed"
                .into(),
        );
    }
    Ok(url)
}

pub fn validate_origin(raw: &str) -> Result<String, String> {
    if raw.split_once("://").is_none_or(|(_, authority)| {
        authority
            .split_once('/')
            .is_some_and(|(_, path)| !path.is_empty())
    }) {
        return Err("站点来源不能包含路径 / Origin must not contain a path".into());
    }
    let url = validate_endpoint(raw)?;
    if url.path() != "/" {
        return Err("站点来源不能包含路径 / Origin must not contain a path".into());
    }
    Ok(url.origin().ascii_serialization())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportSource {
    pub origin: String,
    pub key_id: String,
    pub site_name: String,
    pub key_name: String,
}

impl ImportSource {
    pub fn same_key(&self, other: &Self) -> bool {
        self.origin == other.origin && self.key_id == other.key_id
    }
}

// Do not derive Debug on data containing credentials.
#[derive(Clone, Serialize, Deserialize)]
pub struct ImportDraft {
    pub source: ImportSource,
    pub provider: ProviderConfig,
    pub model_warning: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ResolveEnvelope {
    pub code: i64,
    pub data: Option<ResolveData>,
}

#[derive(Deserialize)]
pub(super) struct ResolveData {
    pub schema_version: u32,
    pub target: String,
    pub source: ImportSource,
    pub provider: WireProvider,
}

#[derive(Deserialize)]
pub(super) struct WireProvider {
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub models_url: Option<String>,
    pub api_key: String,
    pub models: Vec<String>,
    #[serde(default)]
    pub model_aliases: BTreeMap<String, String>,
}

fn text_valid(text: &str, max: usize) -> bool {
    !text.trim().is_empty()
        && text.len() <= max
        && !text.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

pub fn normalized_models(models: &[String]) -> Result<Vec<String>, String> {
    if models.len() > MAX_MODELS {
        return Err("模型数量超出限制 / Too many models".into());
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for model in models {
        let model = model.trim();
        if !text_valid(model, 256) {
            return Err("模型名称无效 / Invalid model name".into());
        }
        if seen.insert(model.to_string()) {
            result.push(model.to_string());
        }
    }
    Ok(result)
}

impl ResolveData {
    pub(super) fn into_draft(self, link: &ImportLink) -> Result<ImportDraft, String> {
        if self.schema_version != 1 || self.target != "tiancaispace-hub" {
            return Err("导入协议版本不支持，请升级 Hub / Unsupported import schema".into());
        }
        let mut source = self.source;
        source.origin = validate_origin(&source.origin)?;
        if source.origin != link.origin
            || !text_valid(&source.key_id, 256)
            || !text_valid(&source.site_name, 256)
            || !text_valid(&source.key_name, 256)
        {
            return Err("导入来源信息不匹配或无效 / Invalid import source".into());
        }
        let wire = self.provider;
        let (provider_type, compatibility) = match wire.protocol.as_str() {
            "openai_responses" => (ProviderType::OpenAiResponses, None),
            "anthropic_messages" => (ProviderType::AnthropicMessages, Some("anthropic".into())),
            "chat_completions" => (ProviderType::ChatCompletions, Some("openai_chat".into())),
            "grok_responses" => (ProviderType::GrokResponses, None),
            _ => return Err("渠道协议不支持，请升级 Hub / Unsupported provider protocol".into()),
        };
        validate_endpoint(&wire.base_url)?;
        if let Some(url) = &wire.models_url {
            validate_endpoint(url)?;
        }
        if !text_valid(&wire.name, 256)
            || is_workbuddy_provider_name(wire.name.trim())
            || is_gmclaw_provider_name(wire.name.trim())
            || !text_valid(&wire.api_key, 8192)
            || !wire.api_key.is_ascii()
        {
            return Err("渠道名称或密钥无效 / Invalid channel name or key".into());
        }
        let models = normalized_models(&wire.models)?;
        if wire.model_aliases.len() > MAX_MODELS
            || wire
                .model_aliases
                .iter()
                .any(|(k, v)| !text_valid(k, 256) || !text_valid(v, 256) || !models.contains(v))
        {
            return Err("模型映射无效 / Invalid model aliases".into());
        }
        Ok(ImportDraft {
            provider: ProviderConfig {
                name: wire.name.trim().into(),
                enabled: false,
                provider_type,
                compatibility,
                base_url: provider_display_base_url(&wire.base_url),
                models_url: wire.models_url,
                api_key: wire.api_key,
                models,
                model_aliases: wire.model_aliases,
                import_source: Some(source.clone()),
                ..Default::default()
            },
            source,
            model_warning: None,
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct UpdateTarget {
    pub name: String,
    pub fingerprint: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CommitImport {
    pub draft: ImportDraft,
    pub name: String,
    pub update: Option<UpdateTarget>,
    pub models: Vec<String>,
    pub enabled: bool,
    pub visible_models: bool,
    pub replace_aliases: bool,
}

pub fn provider_fingerprint(provider: &ProviderConfig) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(provider).expect("provider serialization"),
    ))
}

pub fn merge_import(config: &mut AppConfig, request: &CommitImport) -> Result<(), String> {
    let name = request.name.trim();
    if !text_valid(name, 256) || is_workbuddy_provider_name(name) || is_gmclaw_provider_name(name) {
        return Err("请输入有效且非保留的渠道名称 / Invalid channel name".into());
    }
    let models = normalized_models(&request.models)?;
    if models.is_empty() && (request.enabled || request.visible_models) {
        return Err(
            "请先补全模型，空模型渠道只能禁用保存 / Models required before enabling".into(),
        );
    }
    let index = if let Some(target) = &request.update {
        let (i, existing) = config
            .ai_gateway
            .providers
            .iter()
            .enumerate()
            .find(|(_, p)| p.name == target.name)
            .ok_or("目标渠道已删除，请重新打开预览 / Channel was deleted")?;
        if existing.is_client_reserved() || provider_fingerprint(existing) != target.fingerprint {
            return Err("目标渠道已修改，请重新打开预览 / Channel changed during preview".into());
        }
        Some(i)
    } else {
        None
    };
    if config
        .ai_gateway
        .providers
        .iter()
        .enumerate()
        .any(|(i, p)| Some(i) != index && p.name.eq_ignore_ascii_case(name))
    {
        return Err("渠道名称已存在，请改名或明确选择更新 / Channel name already exists".into());
    }
    let incoming = &request.draft.provider;
    // Only imported fields are replaced. User routing priority, timeout, cache,
    // reasoning settings and aliases survive a normal update.
    let mut provider = index
        .map(|i| config.ai_gateway.providers[i].clone())
        .unwrap_or_default();
    provider.name = name.into();
    provider.enabled = request.enabled;
    provider.provider_type = incoming.provider_type.clone();
    provider.compatibility = incoming.compatibility.clone();
    provider.base_url = incoming.base_url.clone();
    provider.models_url = incoming.models_url.clone();
    provider.api_key = incoming.api_key.clone();
    provider.chatgpt_auth_id = None;
    provider.import_source = Some(request.draft.source.clone());
    provider.models = models.clone();
    if index.is_none() || request.replace_aliases {
        provider.model_aliases = incoming.model_aliases.clone();
    }
    if request.enabled
        && provider
            .model_aliases
            .values()
            .any(|value| !models.contains(value))
    {
        return Err("已有模型映射指向不在本次列表中的模型，请补全模型或选择替换映射 / Existing aliases require review".into());
    }
    match index {
        Some(i) => config.ai_gateway.providers[i] = provider,
        None => config.ai_gateway.providers.push(provider),
    }
    if request.visible_models {
        for model in models {
            if !config.ai_gateway.codex_visible_models.contains(&model) {
                config.ai_gateway.codex_visible_models.push(model);
            }
        }
    }
    Ok(())
}

pub fn validate_draft(draft: &ImportDraft) -> Result<(), String> {
    if draft.provider.model_aliases.len() > MAX_MODELS
        || draft
            .provider
            .model_aliases
            .iter()
            .any(|(k, v)| !text_valid(k, 256) || !text_valid(v, 256))
    {
        return Err("模型映射无效 / Invalid model aliases".into());
    }
    let protocol = match draft.provider.provider_type {
        ProviderType::OpenAiResponses => "openai_responses",
        ProviderType::AnthropicMessages => "anthropic_messages",
        ProviderType::ChatCompletions => "chat_completions",
        ProviderType::GrokResponses => "grok_responses",
        _ => return Err("导入协议不支持 / Unsupported import protocol".into()),
    };
    let mut wire_aliases = draft.provider.model_aliases.clone();
    // Discovery may have removed models. Their aliases are checked on enable.
    wire_aliases.retain(|_, value| draft.provider.models.contains(value));
    let data = ResolveData {
        schema_version: 1,
        target: "tiancaispace-hub".into(),
        source: draft.source.clone(),
        provider: WireProvider {
            name: draft.provider.name.clone(),
            protocol: protocol.into(),
            base_url: draft.provider.base_url.clone(),
            models_url: draft.provider.models_url.clone(),
            api_key: draft.provider.api_key.clone(),
            models: draft.provider.models.clone(),
            model_aliases: wire_aliases,
        },
    };
    let normalized = data.into_draft(&ImportLink {
        origin: draft.source.origin.clone(),
        ticket: String::new(),
    })?;
    if normalized.provider.compatibility != draft.provider.compatibility {
        return Err("导入兼容配置无效 / Invalid import compatibility".into());
    }
    Ok(())
}
