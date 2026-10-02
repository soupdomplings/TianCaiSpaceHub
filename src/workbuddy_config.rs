use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    ai_gateway::config::{
        ProviderConfig, ProviderType, WORKBUDDY_PROVIDER_NAME, is_valid_workbuddy_entry_id,
        provider_display_base_url, workbuddy_provider_name,
    },
    config::AppConfig,
};

/// The legacy local endpoint and the non-secret client token.
/// The Hub currently does not require the token for local requests, but a
/// stable non-empty value keeps clients that validate their config happy.
pub const DEFAULT_WORKBUDDY_URL: &str = "http://127.0.0.1:3847/ai-gateway/v1";
pub const DEFAULT_WORKBUDDY_API_KEY: &str = "workbuddy-local";
pub const WORKBUDDY_LEGACY_ENTRY_ID: &str = "legacy";

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
    pub entries: Vec<WorkBuddyEntryStatus>,
    pub selected_entry_id: Option<String>,
    pub revision: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyEntryStatus {
    pub entry_id: String,
    pub model: WorkBuddyModelConfig,
    pub configured: bool,
    pub local_url: String,
    pub source_provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddySaveRequest {
    #[serde(default)]
    pub entry_id: Option<String>,
    pub revision: String,
    pub model: WorkBuddyModelConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyEntryRequest {
    pub entry_id: String,
    pub revision: String,
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

#[cfg(test)]
fn status_for(path: &Path, exists: bool, model: WorkBuddyModelConfig) -> WorkBuddyConfigStatus {
    let backup = backup_path_for(path);
    WorkBuddyConfigStatus {
        path: path.to_string_lossy().to_string(),
        exists,
        backup_path: backup.to_string_lossy().to_string(),
        backup_exists: backup.exists(),
        model,
        entries: Vec::new(),
        selected_entry_id: None,
        revision: String::new(),
        error: None,
    }
}

#[cfg(test)]
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

/// Legacy compatibility wrapper: only the old endpoint's row can supply a
/// cache key, matched by its id/name/providerModel. The request handler uses
/// configured_request_defaults with the exact entry's gateway mapping instead.
/// A configured source provider remains authoritative for the cache namespace;
/// Anthropic uses it internally but never sends an OpenAI prompt_cache_key.
pub fn configured_cache_key(model: &str) -> Option<String> {
    configured_request_defaults_at(&config_path(), None, model).1
}

fn model_cache_key(configured: &WorkBuddyModelConfig) -> Option<String> {
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
    configured_request_defaults_at(&config_path(), None, model).0
}

fn model_default_effort(configured: &WorkBuddyModelConfig) -> Option<String> {
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
            !item.is_client_reserved()
                && item
                    .name
                    .eq_ignore_ascii_case(model.upstream_provider.trim())
        })
        .cloned()
        .unwrap_or_default();
    provider.name = WORKBUDDY_PROVIDER_NAME.to_string();
    provider.enabled = true;
    provider.import_source = None;
    provider.gmclaw_parameters = None;
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
    provider.model_aliases.retain(|alias, _| {
        [
            model.id.as_str(),
            model.name.as_str(),
            model.provider_model.as_str(),
        ]
        .iter()
        .any(|name| alias.eq_ignore_ascii_case(name))
    });
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
        .find(|item| item.name.eq_ignore_ascii_case(WORKBUDDY_PROVIDER_NAME))
    {
        *existing = provider;
    } else {
        config.ai_gateway.providers.push(provider);
    }
    config.ai_gateway.enabled = true;
}

#[cfg(test)]
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

#[cfg(test)]
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

#[derive(Clone)]
struct ModelDocument {
    rows: Vec<Value>,
    legacy_object: bool,
}

impl ModelDocument {
    fn parse(raw: Option<&[u8]>) -> Result<Self> {
        match raw.map(serde_json::from_slice::<Value>).transpose()? {
            None => Ok(Self {
                rows: Vec::new(),
                legacy_object: false,
            }),
            Some(Value::Array(rows)) => Ok(Self {
                rows,
                legacy_object: false,
            }),
            Some(row @ Value::Object(_)) => Ok(Self {
                rows: vec![row],
                legacy_object: true,
            }),
            _ => bail!("WorkBuddy 配置必须是模型数组或旧版模型对象"),
        }
    }

    fn bytes(&self) -> Result<Vec<u8>> {
        let value = if self.legacy_object && self.rows.len() == 1 {
            self.rows[0].clone()
        } else {
            Value::Array(self.rows.clone())
        };
        let mut bytes = serde_json::to_vec_pretty(&value)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn entries(&self) -> Result<BTreeMap<String, usize>> {
        let mut entries = BTreeMap::new();
        for (index, row) in self.rows.iter().enumerate() {
            if let Some(id) = managed_entry_id(row) {
                ensure!(
                    entries.insert(id, index).is_none(),
                    "WorkBuddy 管理条目身份重复，请先核对配置"
                );
            }
        }
        Ok(entries)
    }
}

fn is_local_url(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Some(url::Host::Domain(host)) => {
            host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("localhost.")
        }
        None => false,
    }
}

fn managed_entry_id(row: &Value) -> Option<String> {
    let url = url::Url::parse(row.get("url")?.as_str()?).ok()?;
    if !matches!(url.scheme(), "http" | "https") || !is_local_url(&url) {
        return None;
    }
    let path = url.path().trim_end_matches('/');
    if path == "/ai-gateway/v1" {
        return Some(WORKBUDDY_LEGACY_ENTRY_ID.into());
    }
    let id = path
        .strip_prefix("/ai-gateway/workbuddy/")?
        .strip_suffix("/v1")?;
    (id != WORKBUDDY_LEGACY_ENTRY_ID && is_valid_workbuddy_entry_id(id)).then(|| id.to_string())
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("无法读取 WorkBuddy 配置或备份"),
    }
}

fn metadata_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.tiancaispacehub-backup.json",
        path.file_name().unwrap_or_default().to_string_lossy()
    ))
}

fn configuration_lock(path: &Path, exclusive: bool) -> Result<fs::File> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let lock_path = path.with_file_name(format!(
        "{}.tiancaispacehub.lock",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    if exclusive {
        fs2::FileExt::lock_exclusive(&file)?;
    } else {
        fs2::FileExt::lock_shared(&file)?;
    }
    Ok(file)
}

fn fingerprint(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

fn ui_revision(path: &Path, raw: Option<&[u8]>, config: &AppConfig) -> Result<String> {
    let providers = config
        .ai_gateway
        .providers
        .iter()
        .filter(|provider| provider.is_workbuddy())
        .collect::<Vec<_>>();
    Ok(format!(
        "workbuddy-v2:{}",
        fingerprint(&(path.to_string_lossy(), raw, providers))?
    ))
}

#[derive(Clone, Serialize, Deserialize)]
struct OperationBackup {
    entry_id: String,
    before_row: Option<Value>,
    before_index: Option<usize>,
    before_exists: bool,
    before_legacy_object: bool,
    before_provider: Option<ProviderConfig>,
    before_provider_index: Option<usize>,
    after_row_hash: String,
    after_provider_hash: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct BackupMetadata {
    version: u32,
    backup: Option<OperationBackup>,
}

fn read_metadata(path: &Path) -> Result<Option<BackupMetadata>> {
    read_optional(&metadata_path(path))?
        .map(|raw| {
            let metadata: BackupMetadata = serde_json::from_slice(&raw)
                .context("WorkBuddy 备份元数据无效，请核对备份后重试")?;
            ensure!(metadata.version == 1, "不支持的 WorkBuddy 备份版本");
            Ok(metadata)
        })
        .transpose()
}

fn provider_position(config: &AppConfig, entry_id: &str) -> Result<Option<usize>> {
    let name =
        workbuddy_provider_name(entry_id).ok_or_else(|| anyhow!("WorkBuddy 条目身份无效"))?;
    let positions = config
        .ai_gateway
        .providers
        .iter()
        .enumerate()
        .filter_map(|(index, provider)| provider.name.eq_ignore_ascii_case(&name).then_some(index))
        .collect::<Vec<_>>();
    ensure!(
        positions.len() <= 1,
        "WorkBuddy 专属渠道身份重复，请先核对渠道配置"
    );
    Ok(positions.first().copied())
}

fn replace_entry_provider(
    config: &mut AppConfig,
    entry_id: &str,
    provider: Option<ProviderConfig>,
    original_index: Option<usize>,
) -> Result<()> {
    if let Some(index) = provider_position(config, entry_id)? {
        if let Some(provider) = provider {
            config.ai_gateway.providers[index] = provider;
        } else {
            config.ai_gateway.providers.remove(index);
        }
    } else if let Some(provider) = provider {
        let index = original_index
            .unwrap_or(config.ai_gateway.providers.len())
            .min(config.ai_gateway.providers.len());
        config.ai_gateway.providers.insert(index, provider);
    }
    Ok(())
}

fn local_url(config: &AppConfig, entry_id: &str) -> Result<String> {
    let address: SocketAddr = config
        .bind
        .parse()
        .context("Hub 监听地址无效，无法生成 WorkBuddy 本机地址")?;
    ensure!(
        address.port() != 0 && (address.ip().is_loopback() || address.ip().is_unspecified()),
        "WorkBuddy 需要 Hub 使用固定端口及本机或通配监听地址"
    );
    let host = if address.is_ipv6() {
        "[::1]"
    } else {
        "127.0.0.1"
    };
    let path = if entry_id == WORKBUDDY_LEGACY_ENTRY_ID {
        "/ai-gateway/v1".into()
    } else {
        format!("/ai-gateway/workbuddy/{entry_id}/v1")
    };
    Ok(format!("http://{host}:{}{path}", address.port()))
}

fn public_model(row: &Value) -> Result<WorkBuddyModelConfig> {
    let mut model: WorkBuddyModelConfig =
        serde_json::from_value(row.clone()).context("WorkBuddy 管理模型字段无效")?;
    model.upstream_api_key.clear();
    Ok(model)
}

fn status_from_raw(
    path: &Path,
    raw: Option<&[u8]>,
    config: &AppConfig,
    selected: Option<&str>,
) -> Result<WorkBuddyConfigStatus> {
    let mut status = WorkBuddyConfigStatus {
        path: path.to_string_lossy().into(),
        exists: raw.is_some(),
        backup_path: backup_path_for(path).to_string_lossy().into(),
        backup_exists: false,
        model: WorkBuddyModelConfig::default(),
        entries: Vec::new(),
        selected_entry_id: None,
        revision: ui_revision(path, raw, config)?,
        error: None,
    };
    status.backup_exists = match read_metadata(path) {
        Ok(Some(metadata)) => metadata.backup.is_some(),
        Ok(None) => backup_path_for(path).is_file(),
        Err(error) => {
            status.error = Some(error.to_string());
            false
        }
    };
    let parsed = (|| -> Result<Vec<WorkBuddyEntryStatus>> {
        let document =
            ModelDocument::parse(raw).context("WorkBuddy JSON 无效，可核对备份后还原")?;
        let positions = document.entries()?;
        let mut entries = positions
            .into_iter()
            .map(|(entry_id, index)| {
                let model = public_model(&document.rows[index])?;
                let source_provider = (!model.upstream_provider.trim().is_empty())
                    .then(|| model.upstream_provider.clone());
                let configured = provider_position(config, &entry_id)?
                    .is_some_and(|position| config.ai_gateway.providers[position].enabled);
                Ok((
                    index,
                    WorkBuddyEntryStatus {
                        entry_id,
                        local_url: model.url.clone(),
                        model,
                        source_provider,
                        configured,
                    },
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        entries.sort_by_key(|(index, _)| *index);
        Ok(entries.into_iter().map(|(_, entry)| entry).collect())
    })();
    match parsed {
        Ok(entries) => status.entries = entries,
        Err(error) => status.error = Some(error.to_string()),
    }
    let selected = if let Some(selected) = selected {
        status
            .entries
            .iter()
            .find(|entry| entry.entry_id == selected)
    } else {
        status
            .entries
            .iter()
            .find(|entry| entry.entry_id == WORKBUDDY_LEGACY_ENTRY_ID)
            .or_else(|| status.entries.first())
    };
    if let Some(entry) = selected {
        status.selected_entry_id = Some(entry.entry_id.clone());
        status.model = entry.model.clone();
    }
    Ok(status)
}

pub fn load_selected(config: &AppConfig, selected: Option<&str>) -> Result<WorkBuddyConfigStatus> {
    let path = config_path();
    // Loading an absent file must not create a user's WorkBuddy directory.
    if !path.exists() {
        return status_from_raw(&path, None, config, selected);
    }
    let _lock = configuration_lock(&path, false)?;
    status_from_raw(&path, read_optional(&path)?.as_deref(), config, selected)
}

/// Resolve the exact dedicated entry once; identical names in other entries
/// must never supply its reasoning or cache defaults.
pub fn configured_request_defaults(
    gateway: &crate::ai_gateway::config::AiGatewayConfig,
    entry_id: Option<&str>,
    model: &str,
) -> (Option<String>, Option<String>) {
    configured_gateway_request_defaults_at(&config_path(), gateway, entry_id, model)
}

fn configured_gateway_request_defaults_at(
    path: &Path,
    gateway: &crate::ai_gateway::config::AiGatewayConfig,
    entry_id: Option<&str>,
    model: &str,
) -> (Option<String>, Option<String>) {
    let Some(name) = workbuddy_provider_name(entry_id.unwrap_or(WORKBUDDY_LEGACY_ENTRY_ID)) else {
        return (None, None);
    };
    let mut providers = gateway
        .providers
        .iter()
        .filter(|provider| provider.name.eq_ignore_ascii_case(&name));
    let provider = providers.next();
    if providers.next().is_some() {
        return (None, None);
    }
    match provider {
        Some(provider) if provider.enabled => {
            configured_request_defaults_for_provider_at(path, entry_id, model, Some(provider))
        }
        // Legacy files and the old endpoint predate the dedicated provider.
        // Keep their direct field matching, without looking at another entry.
        None if entry_id.is_none() => configured_request_defaults_at(path, None, model),
        _ => (None, None),
    }
}

fn configured_request_defaults_at(
    path: &Path,
    entry_id: Option<&str>,
    model: &str,
) -> (Option<String>, Option<String>) {
    configured_request_defaults_for_provider_at(path, entry_id, model, None)
}

fn configured_request_defaults_for_provider_at(
    path: &Path,
    entry_id: Option<&str>,
    model: &str,
    provider: Option<&ProviderConfig>,
) -> (Option<String>, Option<String>) {
    let result = (|| -> Result<_> {
        let entry_id = entry_id.unwrap_or(WORKBUDDY_LEGACY_ENTRY_ID);
        ensure!(
            is_valid_workbuddy_entry_id(entry_id),
            "invalid WorkBuddy identity"
        );
        let raw = read_optional(path)?;
        let document = ModelDocument::parse(raw.as_deref())?;
        let positions = document.entries()?;
        let index = positions
            .get(entry_id)
            .ok_or_else(|| anyhow!("missing WorkBuddy entry"))?;
        let configured: WorkBuddyModelConfig =
            serde_json::from_value(document.rows[*index].clone())?;
        if let Some(provider) = provider {
            let name = workbuddy_provider_name(entry_id)
                .ok_or_else(|| anyhow!("invalid WorkBuddy identity"))?;
            ensure!(
                provider.enabled && provider.name.eq_ignore_ascii_case(&name),
                "WorkBuddy provider does not match selected entry"
            );
            let target = provider
                .resolve_upstream_model(&configured.provider_model)
                .ok_or_else(|| anyhow!("configured WorkBuddy model has no route"))?;
            let requested = provider
                .resolve_upstream_model(model)
                .ok_or_else(|| anyhow!("requested WorkBuddy model has no route"))?;
            ensure!(
                requested.eq_ignore_ascii_case(target),
                "WorkBuddy model does not match selected entry"
            );
        } else {
            ensure!(
                configured_model_matches(&configured, model),
                "WorkBuddy model does not match selected entry"
            );
        }
        Ok((
            model_default_effort(&configured),
            model_cache_key(&configured),
        ))
    })();
    result.unwrap_or((None, None))
}

fn merge_object_fields(target: &mut Value, update: Value) {
    if let (Some(target), Some(update)) = (target.as_object_mut(), update.as_object()) {
        for (key, value) in update {
            if value.is_object() && target.get(key).is_some_and(Value::is_object) {
                merge_object_fields(target.get_mut(key).unwrap(), value.clone());
            } else {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

fn prepare_model(
    config: &AppConfig,
    entry_id: &str,
    request: &WorkBuddyModelConfig,
    previous: Option<&Value>,
) -> Result<(Value, ProviderConfig)> {
    let source = config
        .ai_gateway
        .providers
        .iter()
        .find(|provider| {
            provider.enabled
                && !provider.is_client_reserved()
                && provider.name == request.upstream_provider.trim()
        })
        .ok_or_else(|| anyhow!("请选择已启用的普通来源渠道，专属渠道不能作为来源"))?;
    let requested_model = request.provider_model.trim();
    let actual_model = source
        .resolve_upstream_model(requested_model)
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| anyhow!("模型不在所选来源渠道中，请刷新后重选"))?
        .to_string();
    ensure!(
        matches!(
            request.upstream_protocol.as_str(),
            "openai-responses" | "openai-chat" | "anthropic-messages"
        ),
        "WorkBuddy 上游协议无效"
    );
    ensure!(
        source.provider_type != ProviderType::ChatGptResponses
            || request.upstream_protocol == "openai-responses",
        "ChatGPT 账号渠道仅支持 Responses 协议"
    );
    let url = url::Url::parse(source.base_url.trim())
        .map_err(|_| anyhow!("来源渠道地址无效，请先修正渠道"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host().is_some(),
        "来源渠道需要有效 HTTP/HTTPS 地址"
    );
    ensure!(
        !(is_local_url(&url) && url.port_or_known_default() == config.local_listen_port()),
        "来源渠道指向 Hub 自身，会形成循环请求"
    );
    // The UI edits source/model/protocol/reasoning only. Preserve WorkBuddy's
    // other known fields as well as unknown nested fields on an existing row.
    let mut model: WorkBuddyModelConfig = previous
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    model.upstream_protocol = request.upstream_protocol.clone();
    model.id = if entry_id == WORKBUDDY_LEGACY_ENTRY_ID {
        previous
            .and_then(|row| row.get("id"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .unwrap_or(requested_model)
            .to_string()
    } else {
        format!("tiancaispacehub-{entry_id}")
    };
    model.name = requested_model.into();
    model.provider_model = requested_model.into();
    model.url = local_url(config, entry_id)?;
    model.api_key = DEFAULT_WORKBUDDY_API_KEY.into();
    model.upstream_provider = source.name.clone();
    model.upstream_url = source.base_url.clone();
    // WorkBuddy only calls the local URL; credentials stay in the Hub provider.
    model.upstream_api_key.clear();
    model.upstream_auth = "bearer".into();
    model.reasoning = WorkBuddyReasoningConfig::for_model(
        &model.upstream_protocol,
        &actual_model,
        source.compatibility.as_deref(),
        &request.upstream_default_reasoning_effort,
    );
    model.upstream_default_reasoning_effort = model.reasoning.default_effort.clone();
    model.extra.remove("cacheKey");
    model.extra.remove("cache_key");
    if uses_prompt_cache_key(&model.upstream_protocol) {
        model.extra.insert(
            "cacheKey".into(),
            default_cache_key_for_provider(&source.name).into(),
        );
    }
    let mut row = previous
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    merge_object_fields(&mut row, serde_json::to_value(&model)?);
    if let Some(object) = row.as_object_mut() {
        object.remove("cache_key");
        if !uses_prompt_cache_key(&model.upstream_protocol) {
            object.remove("cacheKey");
        }
    }
    let mut provider = source.clone();
    provider.name =
        workbuddy_provider_name(entry_id).ok_or_else(|| anyhow!("WorkBuddy 条目身份无效"))?;
    provider.enabled = true;
    provider.import_source = None;
    provider.gmclaw_parameters = None;
    provider.provider_type = match model.upstream_protocol.as_str() {
        "anthropic-messages" => ProviderType::AnthropicMessages,
        "openai-chat" => ProviderType::ChatCompletions,
        _ if source.provider_type == ProviderType::ChatGptResponses => {
            ProviderType::ChatGptResponses
        }
        _ => ProviderType::OpenAiResponses,
    };
    if provider.provider_type != ProviderType::ChatGptResponses {
        provider.chatgpt_auth_id = None;
    }
    if provider.provider_type != ProviderType::AnthropicMessages {
        provider.compatibility = None;
    }
    provider.models = vec![actual_model.clone()];
    provider.model_aliases.clear();
    for alias in [&model.id, &model.name, &model.provider_model] {
        if alias != &actual_model {
            provider
                .model_aliases
                .insert(alias.clone(), actual_model.clone());
        }
    }
    Ok((row, provider))
}

struct FileChange {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

impl FileChange {
    fn new(path: PathBuf, after: Option<Vec<u8>>) -> Result<Self> {
        Ok(Self {
            before: read_optional(&path)?,
            path,
            after,
        })
    }
    fn apply(&self) -> Result<()> {
        ensure!(
            read_optional(&self.path)? == self.before,
            "WorkBuddy 文件在保存期间被修改，请刷新后重试"
        );
        replace_file(&self.path, self.after.as_deref())
    }
    fn compensate(&self) -> Result<()> {
        let current = read_optional(&self.path)?;
        if current == self.before {
            return Ok(());
        }
        ensure!(
            current == self.after,
            "WorkBuddy 文件已被其他程序修改，保留该改动并停止自动回滚"
        );
        replace_file(&self.path, self.before.as_deref())
    }
}

fn replace_file(path: &Path, bytes: Option<&[u8]>) -> Result<()> {
    match bytes {
        Some(bytes) => write_file_atomically(path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

fn persist_operation(
    path: &Path,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    config: &mut AppConfig,
    mut next: AppConfig,
    hub_path: &Path,
    metadata: BackupMetadata,
    restoring: bool,
    selected: Option<&str>,
) -> Result<WorkBuddyConfigStatus> {
    let backup_path = if restoring {
        restore_safety_path_for(path)
    } else {
        backup_path_for(path)
    };
    let mut changes = vec![
        FileChange::new(
            backup_path,
            Some(before.clone().unwrap_or_else(|| b"[]\n".to_vec())),
        )?,
        FileChange::new(
            metadata_path(path),
            Some(serde_json::to_vec_pretty(&metadata)?),
        )?,
    ];
    changes.push(FileChange {
        path: path.into(),
        before,
        after,
    });
    let result = (|| -> Result<()> {
        for change in &changes {
            change.apply()?;
        }
        ensure!(
            read_optional(path)? == changes.last().unwrap().after,
            "WorkBuddy 在保存期间被外部修改，请刷新状态"
        );
        next.save(&hub_path.to_path_buf())
            .context("Hub 专属渠道保存失败")?;
        Ok(())
    })();
    if let Err(error) = result {
        let mut recovered = true;
        for change in changes.iter().rev() {
            recovered &= change.compensate().is_ok();
        }
        if !recovered {
            bail!("{error}; WorkBuddy 文件或备份存在后续改动，自动回滚未完成，请检查两端配置");
        }
        return Err(error).context("WorkBuddy 操作已撤销，请刷新后重试");
    }
    *config = next;
    // A non-cooperating WorkBuddy writer may race the Hub TOML commit. Never
    // replace that writer's bytes or report a successful coherent operation.
    ensure!(
        read_optional(path)? == changes.last().unwrap().after,
        "Hub 渠道已保存，但 WorkBuddy 文件随后被外部修改，请刷新并核对两端配置"
    );
    status_from_raw(
        path,
        changes.last().unwrap().after.as_deref(),
        config,
        selected,
    )
}

pub fn save(
    request: &WorkBuddySaveRequest,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<WorkBuddyConfigStatus> {
    save_entry_at(&config_path(), request, config, hub_path)
}

fn save_entry_at(
    path: &Path,
    request: &WorkBuddySaveRequest,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<WorkBuddyConfigStatus> {
    let _lock = configuration_lock(path, true)?;
    let before = read_optional(path)?;
    ensure!(
        request.revision == ui_revision(path, before.as_deref(), config)?,
        "WorkBuddy 配置或专属渠道已变更，请刷新后重试"
    );
    let mut document = ModelDocument::parse(before.as_deref())?;
    let entries = document.entries()?;
    let requested_id = request
        .entry_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let entry_id = requested_id
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    ensure!(
        is_valid_workbuddy_entry_id(&entry_id),
        "WorkBuddy 条目身份无效"
    );
    let index = entries.get(&entry_id).copied();
    ensure!(
        requested_id.is_none() || index.is_some(),
        "WorkBuddy 条目已不存在，请刷新后重新新增"
    );
    let provider_index = provider_position(config, &entry_id)?;
    ensure!(
        requested_id.is_some() || (index.is_none() && provider_index.is_none()),
        "WorkBuddy 新条目身份冲突，请重试"
    );
    let before_row = index.map(|index| document.rows[index].clone());
    let (row, provider) = prepare_model(config, &entry_id, &request.model, before_row.as_ref())?;
    let backup = OperationBackup {
        entry_id: entry_id.clone(),
        before_row,
        before_index: index,
        before_exists: before.is_some(),
        before_legacy_object: document.legacy_object,
        before_provider: provider_index.map(|index| config.ai_gateway.providers[index].clone()),
        before_provider_index: provider_index,
        after_row_hash: fingerprint(&Some(&row))?,
        after_provider_hash: fingerprint(&Some(&provider))?,
    };
    if let Some(index) = index {
        document.rows[index] = row;
    } else {
        document.rows.push(row);
        document.legacy_object = false;
    }
    let mut next = config.clone();
    replace_entry_provider(&mut next, &entry_id, Some(provider), provider_index)?;
    next.ai_gateway.enabled = true;
    persist_operation(
        path,
        before,
        Some(document.bytes()?),
        config,
        next,
        hub_path,
        BackupMetadata {
            version: 1,
            backup: Some(backup),
        },
        false,
        Some(&entry_id),
    )
}

pub fn delete(
    request: &WorkBuddyEntryRequest,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<WorkBuddyConfigStatus> {
    delete_entry_at(&config_path(), request, config, hub_path)
}

fn delete_entry_at(
    path: &Path,
    request: &WorkBuddyEntryRequest,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<WorkBuddyConfigStatus> {
    let _lock = configuration_lock(path, true)?;
    let before = read_optional(path)?;
    ensure!(
        request.revision == ui_revision(path, before.as_deref(), config)?,
        "WorkBuddy 配置或专属渠道已变更，请刷新后重试"
    );
    let mut document = ModelDocument::parse(before.as_deref())?;
    let index = document
        .entries()?
        .get(&request.entry_id)
        .copied()
        .ok_or_else(|| anyhow!("WorkBuddy 条目已不存在，请刷新"))?;
    let provider_index = provider_position(config, &request.entry_id)?;
    let backup = OperationBackup {
        entry_id: request.entry_id.clone(),
        before_row: Some(document.rows.remove(index)),
        before_index: Some(index),
        before_exists: before.is_some(),
        before_legacy_object: document.legacy_object,
        before_provider: provider_index.map(|index| config.ai_gateway.providers[index].clone()),
        before_provider_index: provider_index,
        after_row_hash: fingerprint(&Option::<Value>::None)?,
        after_provider_hash: fingerprint(&Option::<ProviderConfig>::None)?,
    };
    document.legacy_object = false;
    let mut next = config.clone();
    replace_entry_provider(&mut next, &request.entry_id, None, None)?;
    persist_operation(
        path,
        before,
        Some(document.bytes()?),
        config,
        next,
        hub_path,
        BackupMetadata {
            version: 1,
            backup: Some(backup),
        },
        false,
        Some(""),
    )
}

pub fn restore_backup(
    revision: &str,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<WorkBuddyConfigStatus> {
    restore_entry_at(&config_path(), revision, config, hub_path)
}

fn restore_entry_at(
    path: &Path,
    revision: &str,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<WorkBuddyConfigStatus> {
    let _lock = configuration_lock(path, true)?;
    let before = read_optional(path)?;
    ensure!(
        revision == ui_revision(path, before.as_deref(), config)?,
        "WorkBuddy 配置或专属渠道已变更，请刷新后重试"
    );
    let mut next = config.clone();
    let (after, selected) = if let Some(metadata) = read_metadata(path)? {
        let backup = metadata
            .backup
            .ok_or_else(|| anyhow!("没有可还原的 WorkBuddy 操作备份"))?;
        let mut document = ModelDocument::parse(before.as_deref())
            .context("当前文件损坏，无法定点还原；请核对 .bak 与 before-restore 备份")?;
        let index = document.entries()?.get(&backup.entry_id).copied();
        let current_row = index.map(|index| &document.rows[index]);
        ensure!(
            fingerprint(&current_row)? == backup.after_row_hash,
            "WorkBuddy 目标模型已被后续修改，停止还原以保留改动"
        );
        let position = provider_position(config, &backup.entry_id)?;
        let provider = position.map(|index| &config.ai_gateway.providers[index]);
        ensure!(
            fingerprint(&provider)? == backup.after_provider_hash,
            "WorkBuddy 目标渠道已被后续修改，停止还原以保留改动"
        );
        let expected_name = workbuddy_provider_name(&backup.entry_id)
            .ok_or_else(|| anyhow!("WorkBuddy 备份条目身份无效"))?;
        ensure!(
            backup
                .before_provider
                .as_ref()
                .is_none_or(|provider| provider.name.eq_ignore_ascii_case(&expected_name)),
            "WorkBuddy 备份渠道身份不一致"
        );
        ensure!(
            backup.before_row.as_ref().is_none_or(
                |row| managed_entry_id(row).as_deref() == Some(backup.entry_id.as_str())
            ),
            "WorkBuddy 备份模型身份不一致"
        );
        if let Some(index) = index {
            document.rows.remove(index);
        }
        let selected = backup.before_row.as_ref().map(|_| backup.entry_id.clone());
        if let Some(row) = backup.before_row {
            let index = backup
                .before_index
                .unwrap_or(document.rows.len())
                .min(document.rows.len());
            document.rows.insert(index, row);
        }
        document.legacy_object = backup.before_legacy_object && document.rows.len() == 1;
        replace_entry_provider(
            &mut next,
            &backup.entry_id,
            backup.before_provider,
            backup.before_provider_index,
        )?;
        let bytes = if !backup.before_exists && document.rows.is_empty() {
            None
        } else {
            Some(document.bytes()?)
        };
        (bytes, selected)
    } else {
        // Old backups have no operation metadata. Retain their explicit full
        // file restore, but never use it to discard newer managed entries.
        ensure!(
            !config
                .ai_gateway
                .providers
                .iter()
                .any(|provider| provider.is_workbuddy()
                    && !provider.name.eq_ignore_ascii_case(WORKBUDDY_PROVIDER_NAME)),
            "旧版备份不能覆盖多模型配置，请先核对新条目"
        );
        if let Ok(document) = ModelDocument::parse(before.as_deref()) {
            ensure!(
                document
                    .entries()?
                    .keys()
                    .all(|id| id == WORKBUDDY_LEGACY_ENTRY_ID),
                "旧版备份不能覆盖新 WorkBuddy 条目"
            );
        }
        let bytes = read_optional(&backup_path_for(path))?
            .ok_or_else(|| anyhow!("没有可还原的 WorkBuddy 备份"))?;
        let document = ModelDocument::parse(Some(&bytes)).context("WorkBuddy 备份 JSON 无效")?;
        ensure!(
            !document.rows.is_empty(),
            "WorkBuddy 旧版备份为空，不能还原"
        );
        let entries = document.entries()?;
        ensure!(
            entries.keys().all(|id| id == WORKBUDDY_LEGACY_ENTRY_ID),
            "旧版备份含有缺少渠道快照的新条目，不能还原"
        );
        if let Some(index) = entries.get(WORKBUDDY_LEGACY_ENTRY_ID) {
            let model = serde_json::from_value(document.rows[*index].clone())?;
            apply_provider(&mut next, &model);
        } else {
            replace_entry_provider(&mut next, WORKBUDDY_LEGACY_ENTRY_ID, None, None)?;
        }
        (
            Some(bytes),
            entries
                .contains_key(WORKBUDDY_LEGACY_ENTRY_ID)
                .then(|| WORKBUDDY_LEGACY_ENTRY_ID.to_string()),
        )
    };
    persist_operation(
        path,
        before,
        after,
        config,
        next,
        hub_path,
        BackupMetadata {
            version: 1,
            backup: None,
        },
        true,
        selected.as_deref().or(Some("")),
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::*;
    use crate::ai_gateway::config::AiGatewayConfig;

    fn multi_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, AppConfig) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        let hub_path = directory.path().join("hub.toml");
        let mut config = AppConfig::default();
        config.ai_gateway.providers = ["source-a", "source-b"]
            .into_iter()
            .map(|name| ProviderConfig {
                name: name.into(),
                enabled: true,
                base_url: format!("https://{name}.example/v1"),
                api_key: format!("fixture-{name}-key"),
                models: vec!["same-model".into()],
                model_aliases: [("friendly".into(), "same-model".into())].into(),
                ..Default::default()
            })
            .collect();
        config.save(&hub_path).unwrap();
        (directory, path, hub_path, config)
    }

    fn current_status(path: &Path, config: &AppConfig) -> WorkBuddyConfigStatus {
        status_from_raw(path, read_optional(path).unwrap().as_deref(), config, None).unwrap()
    }

    fn save_request(
        path: &Path,
        config: &AppConfig,
        id: Option<&str>,
        source: &str,
    ) -> WorkBuddySaveRequest {
        WorkBuddySaveRequest {
            entry_id: id.map(str::to_string),
            revision: current_status(path, config).revision,
            model: WorkBuddyModelConfig {
                upstream_provider: source.into(),
                provider_model: "friendly".into(),
                upstream_url: "https://untrusted-request.example/v1".into(),
                upstream_api_key: "ignored-request-key".into(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn multiple_sources_for_same_model_keep_native_rows_and_exact_routes() {
        let (_directory, path, hub_path, mut config) = multi_fixture();
        let native = json!({"id":"native-model","url":"https://native.example/v1","privateNativeSetting":{"items":[1,2]}});
        fs::write(&path, serde_json::to_vec(&vec![native.clone()]).unwrap()).unwrap();
        let first_request = save_request(&path, &config, None, "source-a");
        let first = save_entry_at(&path, &first_request, &mut config, &hub_path).unwrap();
        let first_id = first.selected_entry_id.unwrap();
        let second_request = save_request(&path, &config, None, "source-b");
        let second = save_entry_at(&path, &second_request, &mut config, &hub_path).unwrap();
        let second_id = second.selected_entry_id.unwrap();
        assert_ne!(first_id, second_id);
        assert_eq!(first_id.len(), 32);
        assert_eq!(second.entries.len(), 2);
        let rows: Vec<Value> = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(rows[0], native);
        assert_eq!(rows[1]["id"], format!("tiancaispacehub-{first_id}"));
        assert_ne!(rows[1]["url"], rows[2]["url"]);
        assert_eq!(rows[1]["providerModel"], "friendly");
        assert_eq!(rows[1]["upstreamApiKey"], "");
        for (entry_id, source) in [(&first_id, "source-a"), (&second_id, "source-b")] {
            let index = provider_position(&config, entry_id).unwrap().unwrap();
            let provider = &config.ai_gateway.providers[index];
            assert_eq!(provider.api_key, format!("fixture-{source}-key"));
            assert_eq!(
                provider.resolve_upstream_model(&format!("tiancaispacehub-{entry_id}")),
                Some("same-model")
            );
            assert_eq!(
                provider.resolve_upstream_model("friendly"),
                Some("same-model")
            );
            assert!(!provider.matches_model("unrelated-model"));
            assert!(provider.import_source.is_none());
        }
    }

    #[test]
    fn edit_preserves_workbuddy_known_and_unknown_fields_and_other_entries() {
        let (_directory, path, hub_path, mut config) = multi_fixture();
        let request = save_request(&path, &config, None, "source-a");
        let first = save_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        let id = first.selected_entry_id.unwrap();
        let mut rows: Vec<Value> = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        rows[0]["vendor"] = json!("Native Custom Vendor");
        rows[0]["supportsImages"] = json!(false);
        rows[0]["useCustomProtocol"] = json!(true);
        rows[0]["reasoning"]["futureOption"] = json!({"preserve":true});
        rows[0]["unknownNativeField"] = json!([1, 2, 3]);
        rows.push(json!({"id":"ordinary","unknown":27}));
        fs::write(&path, serde_json::to_vec(&rows).unwrap()).unwrap();
        let mut request = save_request(&path, &config, Some(&id), "source-a");
        request.model.upstream_default_reasoning_effort = "xhigh".into();
        save_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        let updated: Vec<Value> = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(updated[0]["vendor"], rows[0]["vendor"]);
        assert_eq!(updated[0]["supportsImages"], false);
        assert_eq!(updated[0]["useCustomProtocol"], true);
        assert_eq!(
            updated[0]["reasoning"]["futureOption"],
            rows[0]["reasoning"]["futureOption"]
        );
        assert_eq!(
            updated[0]["unknownNativeField"],
            rows[0]["unknownNativeField"]
        );
        assert_eq!(updated[1], rows[1]);
        assert_eq!(
            configured_request_defaults_at(&path, Some(&id), "friendly")
                .0
                .as_deref(),
            Some("xhigh")
        );
        assert_eq!(
            configured_request_defaults_at(&path, None, "friendly"),
            (None, None)
        );
        assert_eq!(
            configured_request_defaults_at(&path, Some(&id), "different"),
            (None, None)
        );
    }

    #[test]
    fn request_defaults_accept_real_model_and_alias_without_crossing_entry_boundaries() {
        let (_directory, path, hub_path, mut config) = multi_fixture();
        let mut first_request = save_request(&path, &config, None, "source-a");
        first_request.model.upstream_default_reasoning_effort = "xhigh".into();
        let first = save_entry_at(&path, &first_request, &mut config, &hub_path).unwrap();
        let first_id = first.selected_entry_id.unwrap();
        let mut second_request = save_request(&path, &config, None, "source-b");
        second_request.model.upstream_default_reasoning_effort = "low".into();
        let second = save_entry_at(&path, &second_request, &mut config, &hub_path).unwrap();
        let second_id = second.selected_entry_id.unwrap();
        for (id, effort, cache) in [
            (&first_id, "xhigh", "workbuddy:source-a"),
            (&second_id, "low", "workbuddy:source-b"),
        ] {
            for model in [
                "friendly".to_string(),
                "same-model".to_string(),
                format!("tiancaispacehub-{id}"),
            ] {
                assert_eq!(
                    configured_gateway_request_defaults_at(
                        &path,
                        &config.ai_gateway,
                        Some(id),
                        &model
                    ),
                    (Some(effort.into()), Some(cache.into()))
                );
            }
        }
        assert_eq!(
            configured_gateway_request_defaults_at(&path, &config.ai_gateway, None, "same-model"),
            (None, None)
        );
        assert_eq!(
            configured_gateway_request_defaults_at(
                &path,
                &config.ai_gateway,
                Some("missing"),
                "same-model"
            ),
            (None, None)
        );
        assert_eq!(
            configured_gateway_request_defaults_at(
                &path,
                &config.ai_gateway,
                Some(&first_id),
                "unmapped"
            ),
            (None, None)
        );

        let position = provider_position(&config, &first_id).unwrap().unwrap();
        let other_position = provider_position(&config, &second_id).unwrap().unwrap();
        assert_eq!(
            configured_request_defaults_for_provider_at(
                &path,
                Some(&first_id),
                "same-model",
                Some(&config.ai_gateway.providers[other_position])
            ),
            (None, None)
        );
        let provider = config.ai_gateway.providers[position].clone();
        config.ai_gateway.providers[position]
            .model_aliases
            .insert("friendly".into(), "different-model".into());
        assert_eq!(
            configured_gateway_request_defaults_at(
                &path,
                &config.ai_gateway,
                Some(&first_id),
                "same-model"
            ),
            (None, None)
        );
        config.ai_gateway.providers[position] = provider.clone();
        config.ai_gateway.providers[position].enabled = false;
        assert_eq!(
            configured_gateway_request_defaults_at(
                &path,
                &config.ai_gateway,
                Some(&first_id),
                "friendly"
            ),
            (None, None)
        );
        config.ai_gateway.providers[position] = provider.clone();
        config.ai_gateway.providers.push(provider);
        assert_eq!(
            configured_gateway_request_defaults_at(
                &path,
                &config.ai_gateway,
                Some(&first_id),
                "same-model"
            ),
            (None, None)
        );
    }

    #[test]
    fn legacy_defaults_follow_legacy_mapping_and_keep_pre_provider_compatibility() {
        let (_directory, path, hub_path, mut config) = multi_fixture();
        let legacy = json!({"id":"old-visible-id","url":DEFAULT_WORKBUDDY_URL,"providerModel":"friendly","upstreamProvider":"source-a"});
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut request = save_request(&path, &config, Some("legacy"), "source-a");
        request.model.upstream_default_reasoning_effort = "xhigh".into();
        save_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        let expected = (Some("xhigh".into()), Some("workbuddy:source-a".into()));
        for model in ["old-visible-id", "friendly", "same-model"] {
            assert_eq!(
                configured_gateway_request_defaults_at(&path, &config.ai_gateway, None, model),
                expected
            );
        }
        let request = save_request(&path, &config, None, "source-b");
        save_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        assert_eq!(
            configured_gateway_request_defaults_at(&path, &config.ai_gateway, None, "same-model"),
            expected
        );
        let position = provider_position(&config, "legacy").unwrap().unwrap();
        config.ai_gateway.providers.remove(position);
        assert_eq!(
            configured_gateway_request_defaults_at(&path, &config.ai_gateway, None, "friendly"),
            expected
        );
        assert_eq!(
            configured_gateway_request_defaults_at(&path, &config.ai_gateway, None, "same-model"),
            (None, None)
        );
    }

    #[test]
    fn delete_and_targeted_undo_preserve_later_unrelated_rows_and_hub_settings() {
        let (_directory, path, hub_path, mut config) = multi_fixture();
        let request = save_request(&path, &config, None, "source-a");
        let first = save_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        let id = first.selected_entry_id.unwrap();
        let original = fs::read(&path).unwrap();
        let request = WorkBuddyEntryRequest {
            entry_id: id.clone(),
            revision: current_status(&path, &config).revision,
        };
        let deleted = delete_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        assert!(deleted.entries.is_empty());
        assert!(provider_position(&config, &id).unwrap().is_none());
        let native = json!({"id":"later-native","setting":"keep"});
        fs::write(&path, serde_json::to_vec(&vec![native.clone()]).unwrap()).unwrap();
        config.theme = Some("light".into());
        config.ai_gateway.providers[0].weight = 917;
        config.save(&hub_path).unwrap();
        let revision = current_status(&path, &config).revision;
        let restored = restore_entry_at(&path, &revision, &mut config, &hub_path).unwrap();
        assert_eq!(restored.entries.len(), 1);
        assert!(!restored.backup_exists);
        let rows: Vec<Value> = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let previous: Vec<Value> = serde_json::from_slice(&original).unwrap();
        assert_eq!(rows, vec![previous[0].clone(), native]);
        assert_eq!(config.theme.as_deref(), Some("light"));
        assert_eq!(config.ai_gateway.providers[0].weight, 917);
        assert!(restore_safety_path_for(&path).is_file());
    }

    #[test]
    fn stale_file_or_any_workbuddy_provider_blocks_edit() {
        let (_directory, path, hub_path, mut config) = multi_fixture();
        let request = save_request(&path, &config, None, "source-a");
        let first = save_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        let id = first.selected_entry_id.unwrap();
        let stale = save_request(&path, &config, Some(&id), "source-a");
        let second_request = save_request(&path, &config, None, "source-b");
        let second = save_entry_at(&path, &second_request, &mut config, &hub_path).unwrap();
        let second_id = second.selected_entry_id.unwrap();
        assert!(save_entry_at(&path, &stale, &mut config, &hub_path).is_err());
        let stale = save_request(&path, &config, Some(&id), "source-a");
        let position = provider_position(&config, &second_id).unwrap().unwrap();
        config.ai_gateway.providers[position].timeout_secs += 1;
        config.save(&hub_path).unwrap();
        assert!(save_entry_at(&path, &stale, &mut config, &hub_path).is_err());
        let stale = save_request(&path, &config, Some(&id), "source-a");
        let mut raw = fs::read(&path).unwrap();
        raw.push(b'\n');
        fs::write(&path, raw).unwrap();
        assert!(save_entry_at(&path, &stale, &mut config, &hub_path).is_err());
    }

    #[test]
    fn failed_hub_commit_restores_model_and_backup_without_overwriting_new_hub_state() {
        let (_directory, path, hub_path, mut config) = multi_fixture();
        let request = save_request(&path, &config, None, "source-a");
        let first = save_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        let id = first.selected_entry_id.unwrap();
        let previous = fs::read(&path).unwrap();
        let previous_backup = fs::read(backup_path_for(&path)).unwrap();
        let previous_metadata = fs::read(metadata_path(&path)).unwrap();
        let request = save_request(&path, &config, Some(&id), "source-b");
        let mut concurrent = AppConfig::load_or_default(&hub_path).unwrap();
        concurrent.theme = Some("concurrent-choice".into());
        concurrent.save(&hub_path).unwrap();
        assert!(save_entry_at(&path, &request, &mut config, &hub_path).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(fs::read(backup_path_for(&path)).unwrap(), previous_backup);
        assert_eq!(fs::read(metadata_path(&path)).unwrap(), previous_metadata);
        assert_eq!(
            AppConfig::load_or_default(&hub_path)
                .unwrap()
                .theme
                .as_deref(),
            Some("concurrent-choice")
        );
    }

    #[test]
    fn undo_rejects_changed_target_and_first_creation_can_be_undone() {
        let (_directory, path, hub_path, mut config) = multi_fixture();
        let request = save_request(&path, &config, None, "source-a");
        let first = save_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        let id = first.selected_entry_id.unwrap();
        let position = provider_position(&config, &id).unwrap().unwrap();
        config.ai_gateway.providers[position].weight += 1;
        config.save(&hub_path).unwrap();
        let revision = current_status(&path, &config).revision;
        assert!(restore_entry_at(&path, &revision, &mut config, &hub_path).is_err());
        config.ai_gateway.providers[position].weight -= 1;
        config.save(&hub_path).unwrap();
        let revision = current_status(&path, &config).revision;
        let restored = restore_entry_at(&path, &revision, &mut config, &hub_path).unwrap();
        assert!(!restored.exists);
        assert!(restored.entries.is_empty());
        assert!(provider_position(&config, &id).unwrap().is_none());
    }

    #[test]
    fn legacy_object_edit_preserves_shape_and_old_backup_repairs_corrupt_current_file() {
        let (_directory, path, hub_path, mut config) = multi_fixture();
        let legacy = json!({"id":"friendly","url":DEFAULT_WORKBUDDY_URL,"providerModel":"friendly","upstreamProvider":"source-a","nativeUnknown":13});
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let request = save_request(&path, &config, Some("legacy"), "source-a");
        let status = save_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        assert_eq!(status.selected_entry_id.as_deref(), Some("legacy"));
        let current: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(current.is_object());
        assert_eq!(current["nativeUnknown"], 13);
        assert!(provider_position(&config, "legacy").unwrap().is_some());
        fs::remove_file(metadata_path(&path)).unwrap();
        fs::write(&path, "{broken").unwrap();
        let damaged = current_status(&path, &config);
        assert!(damaged.error.is_some());
        assert!(damaged.backup_exists);
        let restored = restore_entry_at(&path, &damaged.revision, &mut config, &hub_path).unwrap();
        assert!(restored.error.is_none());
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(),
            legacy
        );
        assert_eq!(
            fs::read(restore_safety_path_for(&path)).unwrap(),
            b"{broken"
        );
    }

    #[test]
    fn exclusive_sources_and_deleted_ids_cannot_be_used_to_recreate_entries() {
        let (_directory, path, hub_path, mut config) = multi_fixture();
        let request = save_request(&path, &config, None, "source-a");
        let first = save_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        let id = first.selected_entry_id.unwrap();
        let request = save_request(&path, &config, None, &format!("workbuddy:{id}"));
        assert!(save_entry_at(&path, &request, &mut config, &hub_path).is_err());
        let request = WorkBuddyEntryRequest {
            entry_id: id.clone(),
            revision: current_status(&path, &config).revision,
        };
        delete_entry_at(&path, &request, &mut config, &hub_path).unwrap();
        let request = save_request(&path, &config, Some(&id), "source-a");
        assert!(save_entry_at(&path, &request, &mut config, &hub_path).is_err());
    }

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
