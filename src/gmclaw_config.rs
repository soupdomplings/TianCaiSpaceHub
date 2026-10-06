//! GMClaw 1.1.1 model settings. Only dedicated model rows and active flags
//! are managed; the application database and its schema must already exist.
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use rusqlite::{
    Connection, OpenFlags, TransactionBehavior, params, params_from_iter,
    types::{Value, ValueRef},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    ai_gateway::config::{
        ProviderConfig, ProviderType, gmclaw_provider_name, is_valid_gmclaw_entry_id,
    },
    ai_gateway::gmclaw::GmClawParameters,
    config::AppConfig,
};

pub const GMCLAW_MODEL_ID: &str = "tiancaispacehub";
pub const GMCLAW_LEGACY_ENTRY_ID: &str = "legacy";
pub const GMCLAW_LOCAL_KEY: &str = "gmclaw-local";
const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;
const METADATA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GmClawSaveRequest {
    /// None/empty creates a new independent entry; legacy must be explicit.
    pub entry_id: Option<String>,
    pub make_active: bool,
    pub source_provider: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f64,
    pub revision: String,
    #[serde(default)]
    pub parameters: GmClawParameters,
}

impl Default for GmClawSaveRequest {
    fn default() -> Self {
        Self {
            entry_id: None,
            make_active: true,
            source_provider: String::new(),
            model: String::new(),
            max_tokens: 8192,
            temperature: 0.7,
            revision: String::new(),
            parameters: GmClawParameters::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmClawEntryRequest {
    pub entry_id: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmClawEntryStatus {
    pub entry_id: String,
    pub model_id: String,
    pub model: GmClawModelConfig,
    pub source_provider: Option<String>,
    pub parameters: GmClawParameters,
    pub active: bool,
    pub configured: bool,
    pub local_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmClawModelConfig {
    pub local_url: String,
    pub model_name: String,
    pub max_tokens: u32,
    pub temperature: f64,
    pub context_window: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmClawConfigStatus {
    #[serde(default)]
    pub entries: Vec<GmClawEntryStatus>,
    #[serde(default)]
    pub selected_entry_id: Option<String>,
    pub path: String,
    pub exists: bool,
    pub schema_supported: bool,
    pub backup_path: String,
    pub backup_exists: bool,
    pub configured: bool,
    pub active: bool,
    pub local_url: String,
    pub model: Option<GmClawModelConfig>,
    pub source_provider: Option<String>,
    pub active_model_id: Option<String>,
    pub restart_required: bool,
    pub revision: String,
    pub error: Option<String>,
    #[serde(default)]
    pub parameters: GmClawParameters,
}

// Explicit SQLite types preserve unknown columns without treating blobs or
// SQL NULL as JSON strings. These snapshots are never returned by the API.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
enum StoredValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}
type ModelRow = BTreeMap<String, StoredValue>;

impl StoredValue {
    fn sql_value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Integer(v) => Value::Integer(*v),
            Self::Real(v) => Value::Real(*v),
            Self::Text(v) => Value::Text(v.clone()),
            Self::Blob(v) => Value::Blob(v.clone()),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Snapshot {
    row: Option<ModelRow>,
    active_ids: Vec<String>,
}

#[derive(Clone, Serialize)]
struct ManagedSnapshot {
    rows: BTreeMap<String, ModelRow>,
    active_ids: Vec<String>,
}

impl ManagedSnapshot {
    fn entry(&self, entry_id: &str) -> Snapshot {
        Snapshot {
            row: self.rows.get(entry_id).cloned(),
            active_ids: self.active_ids.clone(),
        }
    }
}

fn legacy_entry_id() -> String {
    GMCLAW_LEGACY_ENTRY_ID.into()
}

#[derive(Clone, Serialize, Deserialize)]
struct Backup {
    #[serde(default = "legacy_entry_id")]
    entry_id: String,
    before: Snapshot,
    provider: Option<ProviderConfig>,
    provider_index: Option<usize>,
    source_provider: Option<String>,
    // Keep the metadata v1 database-only hash compatible with existing backups.
    after_revision: String,
    after_provider_fingerprint: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Metadata {
    version: u32,
    database_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_provider: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    row_fingerprint: String,
    #[serde(default)]
    entries: BTreeMap<String, EntryMetadata>,
    backup: Option<Backup>,
}

#[derive(Clone, Serialize, Deserialize)]
struct EntryMetadata {
    source_provider: Option<String>,
    row_fingerprint: String,
}

fn model_id(entry_id: &str) -> String {
    if entry_id == GMCLAW_LEGACY_ENTRY_ID {
        GMCLAW_MODEL_ID.into()
    } else {
        format!("{GMCLAW_MODEL_ID}-{entry_id}")
    }
}

fn entry_id_from_model_id(model: &str) -> Option<&str> {
    if model == GMCLAW_MODEL_ID {
        return Some(GMCLAW_LEGACY_ENTRY_ID);
    }
    model.strip_prefix("tiancaispacehub-").filter(|entry_id| {
        *entry_id != GMCLAW_LEGACY_ENTRY_ID && is_valid_gmclaw_entry_id(entry_id)
    })
}

fn validate_entry_id(entry_id: &str) -> Result<()> {
    ensure!(
        is_valid_gmclaw_entry_id(entry_id),
        "天工 Claw 条目身份无效，请刷新后重试"
    );
    Ok(())
}

pub fn config_path() -> PathBuf {
    if let Some(path) = env::var_os("GMCLAW_CONFIG_PATH").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    native_config_path()
}

pub(crate) fn native_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    let root = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    #[cfg(target_os = "macos")]
    let root = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/Application Support");
    // No Linux integration is offered. This fallback only keeps shared code
    // portable; opening a missing database never creates application data.
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let root = PathBuf::from(".");
    root.join("tiangong-desktop/electron-data.db")
}

fn metadata_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tiancaispacehub-backup.json");
    PathBuf::from(name)
}

fn fingerprint(value: &impl Serialize) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

fn canonical_identity(path: &Path) -> Result<String> {
    Ok(fs::canonicalize(path)
        .context("无法定位天工 Claw 数据库")?
        .to_string_lossy()
        .into_owned())
}

fn open_existing(path: &Path, writable: bool) -> Result<Connection> {
    ensure!(
        path.is_file(),
        "未找到天工 Claw 数据库，请先安装并打开天工 Claw，或设置 GMCLAW_CONFIG_PATH"
    );
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(path, flags)
        .context("无法打开天工 Claw 数据库，请确认文件可访问且未被独占锁定")?;
    connection.busy_timeout(Duration::from_secs(3))?;
    if writable {
        connection.pragma_update(None, "foreign_keys", true)?;
    }
    Ok(connection)
}

fn schema_columns(connection: &Connection) -> Result<Vec<String>> {
    let mut statement = connection.prepare("PRAGMA table_info(model_configs)")?;
    let fields = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let required = [
        ("model_id", "TEXT"),
        ("provider", "TEXT"),
        ("base_url", "TEXT"),
        ("model_name", "TEXT"),
        ("api_key", "TEXT"),
        ("max_tokens", "INTEGER"),
        ("temperature", "REAL"),
        ("is_active", "INTEGER"),
        ("extra_params", "TEXT"),
        ("created_at", "TEXT"),
        ("updated_at", "TEXT"),
    ];
    for (name, kind) in required {
        ensure!(
            fields
                .iter()
                .any(|(column, ty, _)| column == name && ty.eq_ignore_ascii_case(kind)),
            "天工 Claw 数据库结构不兼容：缺少或改变字段 {name}；未修改数据库"
        );
    }
    ensure!(
        fields
            .iter()
            .any(|(name, _, primary)| name == "model_id" && *primary == 1),
        "天工 Claw model_id 主键结构不兼容；未修改数据库"
    );
    Ok(fields.into_iter().map(|(name, _, _)| name).collect())
}

fn identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn snapshot(connection: &Connection, columns: &[String]) -> Result<ManagedSnapshot> {
    let sql = format!(
        "SELECT {} FROM model_configs WHERE model_id = ?1 OR model_id GLOB ?2 ORDER BY model_id",
        columns
            .iter()
            .map(|name| identifier(name))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query([GMCLAW_MODEL_ID, "tiancaispacehub-*"])?;
    let mut managed = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let mut values = BTreeMap::new();
        for (index, name) in columns.iter().enumerate() {
            let value = match row.get_ref(index)? {
                ValueRef::Null => StoredValue::Null,
                ValueRef::Integer(value) => StoredValue::Integer(value),
                ValueRef::Real(value) => StoredValue::Real(value),
                ValueRef::Text(value) => StoredValue::Text(
                    std::str::from_utf8(value)
                        .context("天工 Claw 配置含无效文本")?
                        .to_owned(),
                ),
                ValueRef::Blob(value) => StoredValue::Blob(value.to_vec()),
            };
            values.insert(name.clone(), value);
        }
        if let Some(entry_id) = text(&values, "model_id").and_then(entry_id_from_model_id) {
            managed.insert(entry_id.to_owned(), values);
        }
    }
    let mut active = connection
        .prepare("SELECT model_id FROM model_configs WHERE is_active = 1 ORDER BY model_id")?;
    let active_ids = active
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    ensure!(
        active_ids.len() <= 1,
        "天工 Claw 数据库存在多个默认模型，请先在天工 Claw 中修复"
    );
    Ok(ManagedSnapshot {
        rows: managed,
        active_ids,
    })
}

fn text<'a>(row: &'a ModelRow, name: &str) -> Option<&'a str> {
    match row.get(name) {
        Some(StoredValue::Text(value)) => Some(value),
        _ => None,
    }
}

fn integer(row: &ModelRow, name: &str) -> Option<i64> {
    match row.get(name) {
        Some(StoredValue::Integer(value)) => Some(*value),
        _ => None,
    }
}

fn real(row: &ModelRow, name: &str) -> Option<f64> {
    match row.get(name) {
        Some(StoredValue::Real(value)) => Some(*value),
        Some(StoredValue::Integer(value)) => Some(*value as f64),
        _ => None,
    }
}

fn row_extra(row: Option<&ModelRow>) -> Result<serde_json::Map<String, serde_json::Value>> {
    let raw = row
        .and_then(|row| text(row, "extra_params"))
        .unwrap_or("{}");
    let value: serde_json::Value =
        serde_json::from_str(if raw.trim().is_empty() { "{}" } else { raw })
            .context("天工 Claw 专用模型 extra_params 格式无效，请先修复后重试")?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("天工 Claw 专用模型 extra_params 必须为 JSON 对象"))
}

fn context_window(row: &ModelRow) -> u32 {
    row_extra(Some(row))
        .ok()
        .and_then(|value| {
            value
                .get("context_window")
                .or_else(|| value.get("contextWindow"))
                .and_then(serde_json::Value::as_u64)
        })
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_CONTEXT_WINDOW)
}

fn read_metadata(path: &Path) -> Result<(Option<Vec<u8>>, Option<Metadata>)> {
    let bytes = match fs::read(metadata_path(path)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((None, None)),
        Err(error) => return Err(error).context("无法读取天工 Claw 接入备份"),
    };
    let mut metadata: Metadata = serde_json::from_slice(&bytes)
        .context("天工 Claw 接入备份格式无效，已停止操作以保留恢复信息")?;
    ensure!(
        matches!(metadata.version, 1 | METADATA_VERSION),
        "天工 Claw 接入备份版本不支持"
    );
    ensure!(
        metadata.database_path == canonical_identity(path)?,
        "天工 Claw 接入备份属于其他数据库，已停止操作"
    );
    if metadata.version == 1 {
        metadata.entries.insert(
            legacy_entry_id(),
            EntryMetadata {
                source_provider: metadata.source_provider.clone(),
                row_fingerprint: metadata.row_fingerprint.clone(),
            },
        );
    }
    Ok((Some(bytes), Some(metadata)))
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("无法创建天工 Claw 备份临时文件")?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .context("无法原子保存天工 Claw 接入备份")?;
    Ok(())
}

fn restore_metadata(path: &Path, previous: Option<&[u8]>) -> Result<()> {
    let path = metadata_path(path);
    if let Some(bytes) = previous {
        atomic_write(&path, bytes)
    } else {
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("无法撤销未完成的天工 Claw 备份"),
        }
    }
}

fn empty_status(path: &Path) -> GmClawConfigStatus {
    GmClawConfigStatus {
        entries: Vec::new(),
        selected_entry_id: None,
        path: path.to_string_lossy().into_owned(),
        exists: path.is_file(),
        schema_supported: false,
        backup_path: metadata_path(path).to_string_lossy().into_owned(),
        backup_exists: false,
        configured: false,
        active: false,
        local_url: String::new(),
        model: None,
        source_provider: None,
        active_model_id: None,
        restart_required: false,
        revision: "missing".into(),
        error: None,
        parameters: GmClawParameters::default(),
    }
}

fn status_from_snapshot(
    path: &Path,
    snapshot: &ManagedSnapshot,
    metadata: Option<&Metadata>,
    config: &AppConfig,
    selected: Option<&str>,
) -> Result<GmClawConfigStatus> {
    let mut status = empty_status(path);
    status.exists = true;
    status.schema_supported = true;
    status.revision = ui_revision(snapshot, config)?;
    status.backup_exists = metadata.is_some_and(|metadata| metadata.backup.is_some());
    status.active_model_id = snapshot.active_ids.first().cloned();
    for (entry_id, row) in &snapshot.rows {
        let local_url = text(row, "base_url").unwrap_or_default().to_owned();
        let model = GmClawModelConfig {
            local_url: local_url.clone(),
            model_name: text(row, "model_name").unwrap_or_default().into(),
            max_tokens: integer(row, "max_tokens")
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(8192),
            temperature: real(row, "temperature").unwrap_or(0.7),
            context_window: context_window(row),
        };
        let provider =
            provider_position(config, entry_id)?.map(|index| &config.ai_gateway.providers[index]);
        let source_provider = known_source(metadata, snapshot, entry_id);
        let configured = text(row, "provider") == Some("openai")
            && text(row, "api_key") == Some(GMCLAW_LOCAL_KEY)
            && provider.is_some_and(|provider| {
                provider.enabled && provider.matches_model(&model.model_name)
            })
            && url::Url::parse(&local_url).is_ok_and(|url| {
                url.scheme() == "http"
                    && matches!(
                        url.host_str(),
                        Some("127.0.0.1" | "[::1]" | "::1" | "localhost")
                    )
                    && url.path() == endpoint_path(entry_id)
            });
        status.entries.push(GmClawEntryStatus {
            entry_id: entry_id.clone(),
            model_id: model_id(entry_id),
            model,
            source_provider,
            parameters: provider
                .and_then(|provider| provider.gmclaw_parameters.clone())
                .unwrap_or_default(),
            active: snapshot
                .active_ids
                .iter()
                .any(|id| id == &model_id(entry_id)),
            configured,
            local_url,
        });
    }
    let selected_index = match selected {
        Some("") => None,
        Some(entry_id) => {
            validate_entry_id(entry_id)?;
            Some(
                status
                    .entries
                    .iter()
                    .position(|entry| entry.entry_id == entry_id)
                    .ok_or_else(|| anyhow!("所选天工 Claw 条目已不存在，请刷新后重试"))?,
            )
        }
        None => status
            .entries
            .iter()
            .position(|entry| entry.active)
            .or_else(|| {
                status
                    .entries
                    .iter()
                    .position(|entry| entry.entry_id == GMCLAW_LEGACY_ENTRY_ID)
            })
            .or_else(|| (!status.entries.is_empty()).then_some(0)),
    };
    if let Some(index) = selected_index {
        let entry = &status.entries[index];
        status.selected_entry_id = Some(entry.entry_id.clone());
        status.configured = entry.configured;
        status.active = entry.active;
        status.local_url = entry.local_url.clone();
        status.model = Some(entry.model.clone());
        status.source_provider = entry.source_provider.clone();
        status.parameters = entry.parameters.clone();
    }
    Ok(status)
}

pub fn load(config: &AppConfig) -> Result<GmClawConfigStatus> {
    load_selected(config, None)
}

/// Session model choices include native desktop rows as well as Hub-managed rows.
/// Read only public selection fields: never load keys, endpoints, or backups.
pub(crate) fn model_choices() -> Result<(
    Vec<crate::im::core::thread::ThreadModelChoice>,
    Option<String>,
)> {
    model_choices_at(&config_path())
}

/// Public model identity fields used to resume a real desktop task. No keys.
pub(crate) fn session_model_metadata() -> Result<Vec<(String, String, bool)>> {
    let path = config_path();
    // Keep the same ID/default validation as the shared model chooser.
    model_choices_at(&path)?;
    let connection = open_existing(&path, false)?;
    let mut statement = connection
        .prepare("SELECT model_id, model_name, is_active FROM model_configs ORDER BY model_id")?;
    Ok(statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? == 1))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

fn model_choices_at(
    path: &Path,
) -> Result<(
    Vec<crate::im::core::thread::ThreadModelChoice>,
    Option<String>,
)> {
    let connection = open_existing(path, false)?;
    let mut statement = connection
        .prepare("SELECT model_id, model_name, is_active FROM model_configs ORDER BY is_active DESC, model_name, model_id")
        .context("无法读取天工模型列表，请确认已完成天工模型配置")?;
    let mut rows = statement.query([])?;
    let mut choices = Vec::new();
    let mut active_model = None;
    while let Some(row) = rows.next()? {
        let model_id = row.get::<_, String>(0)?;
        ensure!(
            !model_id.trim().is_empty()
                && model_id.trim() == model_id
                && model_id.len() <= 256
                && !model_id.chars().any(char::is_control)
                && !matches!(
                    model_id.as_str(),
                    "__default__" | "__custom__" | "default" | "默认"
                ),
            "天工模型列表包含无效模型 ID，请先在天工中修复"
        );
        let model_name = row
            .get::<_, String>(1)?
            .chars()
            .filter(|character| !character.is_control())
            .take(120)
            .collect::<String>();
        let label = if model_name.trim().is_empty() || model_name == model_id {
            model_id.clone()
        } else {
            format!("{} · {model_id}", model_name.trim())
        };
        if row.get::<_, i64>(2)? == 1 {
            ensure!(
                active_model.is_none(),
                "天工模型列表存在多个默认模型，请先在天工中修复"
            );
            active_model = Some(model_id.clone());
        }
        choices.push(crate::im::core::thread::ThreadModelChoice {
            label,
            value: model_id,
        });
    }
    Ok((choices, active_model))
}

pub fn load_selected(config: &AppConfig, entry_id: Option<&str>) -> Result<GmClawConfigStatus> {
    load_at(&config_path(), config, entry_id)
}

fn load_at(path: &Path, config: &AppConfig, selected: Option<&str>) -> Result<GmClawConfigStatus> {
    let mut status = empty_status(path);
    if !status.exists {
        status.error = Some(
            "未找到天工 Claw 数据库，请先安装并打开天工 Claw，或设置 GMCLAW_CONFIG_PATH".into(),
        );
        return Ok(status);
    }
    let result = (|| {
        let mut connection = open_existing(path, false)?;
        let columns = schema_columns(&connection)?;
        let transaction = connection.transaction()?;
        let snapshot = snapshot(&transaction, &columns)?;
        let (_, metadata) = read_metadata(path)?;
        status_from_snapshot(path, &snapshot, metadata.as_ref(), config, selected)
    })();
    match result {
        Ok(status) => Ok(status),
        Err(error) => {
            status.revision = "unavailable".into();
            status.error = Some(error.to_string());
            Ok(status)
        }
    }
}

fn ui_revision(snapshot: &ManagedSnapshot, config: &AppConfig) -> Result<String> {
    let mut providers = BTreeMap::new();
    for provider in config
        .ai_gateway
        .providers
        .iter()
        .filter(|provider| provider.is_gmclaw())
    {
        ensure!(
            providers
                .insert(provider.name.to_ascii_lowercase(), provider)
                .is_none(),
            "Hub 中存在重复天工 Claw 专用渠道，请先修复配置"
        );
    }
    // Entire managed set plus active selection; unrelated channels and provider
    // positions are excluded. Older page tokens must refresh before mutation.
    Ok(format!(
        "gmclaw-v3:{}",
        fingerprint(&(snapshot, providers))?
    ))
}

fn check_revision(expected: &str, current: &ManagedSnapshot, config: &AppConfig) -> Result<()> {
    ensure!(
        !expected.is_empty() && expected == ui_revision(current, config)?,
        "天工 Claw 模型配置或 Hub 专用渠道已变化，请刷新后重试"
    );
    Ok(())
}

fn provider_position(config: &AppConfig, entry_id: &str) -> Result<Option<usize>> {
    let name = gmclaw_provider_name(entry_id).ok_or_else(|| anyhow!("天工 Claw 条目身份无效"))?;
    let positions = config
        .ai_gateway
        .providers
        .iter()
        .enumerate()
        .filter(|(_, provider)| provider.name.eq_ignore_ascii_case(&name))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    ensure!(
        positions.len() <= 1,
        "Hub 中存在重复天工 Claw 专用渠道，请先修复配置"
    );
    Ok(positions.first().copied())
}

fn replace_provider(
    config: &mut AppConfig,
    entry_id: &str,
    provider: Option<ProviderConfig>,
    position: Option<usize>,
) {
    let name = gmclaw_provider_name(entry_id).expect("validated entry identity");
    config
        .ai_gateway
        .providers
        .retain(|provider| !provider.name.eq_ignore_ascii_case(&name));
    if let Some(provider) = provider {
        config.ai_gateway.providers.insert(
            position
                .unwrap_or(config.ai_gateway.providers.len())
                .min(config.ai_gateway.providers.len()),
            provider,
        );
    }
}

fn endpoint_path(entry_id: &str) -> String {
    if entry_id == GMCLAW_LEGACY_ENTRY_ID {
        "/ai-gateway/gmclaw/v1".into()
    } else {
        format!("/ai-gateway/gmclaw/{entry_id}/v1")
    }
}

fn local_url(config: &AppConfig, entry_id: &str) -> Result<String> {
    let address: SocketAddr = config
        .bind
        .parse()
        .context("Hub 监听地址无效，无法生成天工 Claw 本机地址")?;
    ensure!(
        address.port() != 0,
        "Hub 必须使用固定本机端口才能接入天工 Claw"
    );
    ensure!(
        address.ip().is_loopback() || address.ip().is_unspecified(),
        "Hub 当前仅监听指定网络地址，请改为本机或通配监听地址后接入天工 Claw"
    );
    let host = if address.is_ipv6() {
        "[::1]"
    } else {
        "127.0.0.1"
    };
    Ok(format!(
        "http://{host}:{}{}",
        address.port(),
        endpoint_path(entry_id)
    ))
}
fn validate_source(provider: &ProviderConfig, config: &AppConfig) -> Result<()> {
    if provider.provider_type == ProviderType::ChatGptResponses {
        bail!("天工 Claw 账号登录渠道暂未开放，请选择普通 API 渠道");
    }
    let url = url::Url::parse(provider.base_url.trim())
        .map_err(|_| anyhow!("所选来源渠道的上游地址无效，请先修正渠道配置"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host().is_some(),
        "所选来源渠道必须使用有效的 HTTP 或 HTTPS 上游地址"
    );
    let local_host = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Some(url::Host::Domain(host)) => {
            host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("localhost.")
        }
        None => false,
    };
    ensure!(
        !(local_host && url.port_or_known_default() == config.local_listen_port()),
        "所选来源渠道指向 Hub 自身，会形成循环请求，请改用真实上游渠道"
    );
    Ok(())
}

pub fn save(
    request: &GmClawSaveRequest,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<GmClawConfigStatus> {
    save_at(&config_path(), request, config, hub_path)
}

fn save_at(
    path: &Path,
    request: &GmClawSaveRequest,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<GmClawConfigStatus> {
    ensure!(request.max_tokens > 0, "最大输出 token 数必须大于 0");
    ensure!(
        request.temperature.is_finite() && (0.0..=2.0).contains(&request.temperature),
        "temperature 必须为 0 到 2 之间的数值"
    );
    let model = request.model.trim();
    ensure!(!model.is_empty(), "请选择天工 Claw 使用的模型");
    let source = config
        .ai_gateway
        .providers
        .iter()
        .find(|provider| {
            provider.name == request.source_provider.trim()
                && provider.enabled
                && !provider.is_client_reserved()
        })
        .cloned()
        .ok_or_else(|| anyhow!("来源渠道不可用，请选择一个已启用的普通 Hub 渠道"))?;
    request
        .parameters
        .validate_for(&source)
        .map_err(anyhow::Error::msg)?;
    ensure!(
        source.matches_model(model),
        "模型不在所选渠道的模型或别名列表中，请刷新后重选"
    );
    validate_source(&source, config)?;
    let requested_id = request
        .entry_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    if let Some(entry_id) = requested_id {
        validate_entry_id(entry_id)?;
    }

    let mut connection = open_existing(path, true)?;
    let columns = schema_columns(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .context("天工 Claw 数据库正在写入，请稍后重试")?;
    let before = snapshot(&transaction, &columns)?;
    check_revision(&request.revision, &before, config)?;
    let (previous_bytes, previous) = read_metadata(path)?;
    let entry_id = if let Some(entry_id) = requested_id {
        ensure!(
            before.rows.contains_key(entry_id),
            "所选天工 Claw 条目已不存在，请刷新后重试"
        );
        entry_id.to_owned()
    } else {
        // An independent identity permits identical model names and aliases in
        // different channels. A missing selection always means create, not edit.
        let entry_id = uuid::Uuid::new_v4().simple().to_string();
        ensure!(
            !before.rows.contains_key(&entry_id) && provider_position(config, &entry_id)?.is_none(),
            "新增条目身份冲突，请重试"
        );
        entry_id
    };
    let url = local_url(config, &entry_id)?;
    let position = provider_position(config, &entry_id)?;
    let before_provider = position.map(|index| config.ai_gateway.providers[index].clone());
    let mut provider = source.clone();
    provider.name = gmclaw_provider_name(&entry_id).expect("validated entry identity");
    provider.enabled = true;
    provider.import_source = None;
    provider.gmclaw_parameters = Some(request.parameters.clone());
    let mut next_config = config.clone();
    replace_provider(
        &mut next_config,
        &entry_id,
        Some(provider.clone()),
        position,
    );
    next_config.ai_gateway.enabled = true;

    let target = before.entry(&entry_id);
    let mut extra = row_extra(target.row.as_ref())?;
    if target.row.is_none() {
        extra.insert("context_window".into(), DEFAULT_CONTEXT_WINDOW.into());
    }
    let model_id = model_id(&entry_id);
    let active = request.make_active || before.active_ids.iter().any(|id| id == &model_id);
    if request.make_active {
        transaction.execute(
            "UPDATE model_configs SET is_active = 0 WHERE is_active = 1",
            [],
        )?;
    }
    // UPDATE preserves unknown NOT NULL columns that an upsert may reject.
    let sql = if target.row.is_some() {
        "UPDATE model_configs SET provider='openai',base_url=?2,model_name=?3,api_key=?4,max_tokens=?5,temperature=?6,is_active=?8,extra_params=?7,updated_at=datetime('now') WHERE model_id=?1"
    } else {
        "INSERT INTO model_configs (model_id,provider,base_url,model_name,api_key,max_tokens,temperature,is_active,extra_params,updated_at) VALUES (?1,'openai',?2,?3,?4,?5,?6,?8,?7,datetime('now'))"
    };
    transaction
        .execute(
            sql,
            params![
                model_id,
                url,
                model,
                GMCLAW_LOCAL_KEY,
                i64::from(request.max_tokens),
                request.temperature,
                serde_json::to_string(&extra)?,
                i64::from(active)
            ],
        )
        .context("无法保存天工 Claw 模型；数据库结构可能已变化")?;
    let after = snapshot(&transaction, &columns)?;
    let backup = Backup {
        entry_id: entry_id.clone(),
        before: target,
        provider: before_provider,
        provider_index: position,
        source_provider: known_source(previous.as_ref(), &before, &entry_id),
        after_revision: fingerprint(&after.entry(&entry_id))?,
        after_provider_fingerprint: fingerprint(&Some(provider))?,
    };
    let metadata = next_metadata(
        path,
        &before,
        &after,
        previous.as_ref(),
        &entry_id,
        Some(source.name),
        Some(backup),
    )?;
    persist_changes(
        path,
        transaction,
        &after,
        metadata,
        previous_bytes.as_deref(),
        config,
        next_config,
        hub_path,
        Some(&entry_id),
    )
}

const SOURCE_FINGERPRINT_PREFIX: &str = "gmclaw-source-v1:";

fn source_row_fingerprint(row: &ModelRow) -> Result<String> {
    // GMClaw's model upsert updates its timestamp even when only selecting the
    // default. Keep all configuration and unknown columns in this association.
    let mut row = row.clone();
    row.remove("is_active");
    row.remove("updated_at");
    Ok(format!(
        "{SOURCE_FINGERPRINT_PREFIX}{}",
        fingerprint(&Some(row))?
    ))
}

fn source_row_matches(row: &ModelRow, expected: &str) -> bool {
    if expected.starts_with(SOURCE_FINGERPRINT_PREFIX) {
        return source_row_fingerprint(row).ok().as_deref() == Some(expected);
    }
    // Metadata v1 and early v2 stored the complete row. Accept that hash with
    // either valid active flag, without weakening the match for any other field.
    // An old timestamp cannot be recovered from a hash, so a changed timestamp
    // needs one explicit re-save to establish the new association fingerprint.
    if fingerprint(&Some(row)).ok().as_deref() == Some(expected) {
        return true;
    }
    let alternate = match integer(row, "is_active") {
        Some(0) => 1,
        Some(1) => 0,
        _ => return false,
    };
    let mut row = row.clone();
    row.insert("is_active".into(), StoredValue::Integer(alternate));
    fingerprint(&Some(row)).ok().as_deref() == Some(expected)
}

fn known_source(
    metadata: Option<&Metadata>,
    snapshot: &ManagedSnapshot,
    entry_id: &str,
) -> Option<String> {
    metadata
        .and_then(|metadata| metadata.entries.get(entry_id))
        .filter(|metadata| {
            snapshot
                .rows
                .get(entry_id)
                .is_some_and(|row| source_row_matches(row, &metadata.row_fingerprint))
        })
        .and_then(|metadata| metadata.source_provider.clone())
}

fn next_metadata(
    path: &Path,
    before: &ManagedSnapshot,
    after: &ManagedSnapshot,
    previous: Option<&Metadata>,
    target: &str,
    source: Option<String>,
    backup: Option<Backup>,
) -> Result<Metadata> {
    let mut entries = BTreeMap::new();
    for (entry_id, row) in &after.rows {
        // Preserve proven associations for other entries, including when the
        // default was changed directly in GMClaw since our previous operation.
        entries.insert(
            entry_id.clone(),
            EntryMetadata {
                source_provider: if entry_id == target {
                    source.clone()
                } else {
                    known_source(previous, before, entry_id)
                },
                row_fingerprint: source_row_fingerprint(row)?,
            },
        );
    }
    Ok(Metadata {
        version: METADATA_VERSION,
        database_path: canonical_identity(path)?,
        source_provider: None,
        row_fingerprint: String::new(),
        entries,
        backup,
    })
}

pub fn activate(
    request: &GmClawEntryRequest,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<GmClawConfigStatus> {
    mutate_entry_at(&config_path(), request, config, hub_path, false)
}

pub fn delete(
    request: &GmClawEntryRequest,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<GmClawConfigStatus> {
    mutate_entry_at(&config_path(), request, config, hub_path, true)
}

fn mutate_entry_at(
    path: &Path,
    request: &GmClawEntryRequest,
    config: &mut AppConfig,
    hub_path: &Path,
    delete: bool,
) -> Result<GmClawConfigStatus> {
    let entry_id = request.entry_id.trim();
    validate_entry_id(entry_id)?;
    let mut connection = open_existing(path, true)?;
    let columns = schema_columns(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .context("天工 Claw 数据库正在写入，请稍后重试")?;
    let before = snapshot(&transaction, &columns)?;
    check_revision(&request.revision, &before, config)?;
    let row = before
        .rows
        .get(entry_id)
        .ok_or_else(|| anyhow!("所选天工 Claw 条目已不存在，请刷新后重试"))?;
    let (previous_bytes, previous) = read_metadata(path)?;
    let position = provider_position(config, entry_id)?;
    let provider = position.map(|index| config.ai_gateway.providers[index].clone());
    let mut next_config = config.clone();
    if delete {
        transaction.execute(
            "DELETE FROM model_configs WHERE model_id=?1",
            [model_id(entry_id)],
        )?;
        replace_provider(&mut next_config, entry_id, None, position);
    } else {
        ensure!(
            provider.as_ref().is_some_and(|provider| provider.enabled
                && provider.matches_model(text(row, "model_name").unwrap_or_default())),
            "所选条目的 Hub 渠道不可用，请先重新保存该条目"
        );
        transaction.execute("UPDATE model_configs SET is_active=0 WHERE is_active=1", [])?;
        transaction.execute(
            "UPDATE model_configs SET is_active=1 WHERE model_id=?1",
            [model_id(entry_id)],
        )?;
    }
    let after = snapshot(&transaction, &columns)?;
    let source = known_source(previous.as_ref(), &before, entry_id);
    let backup = Backup {
        entry_id: entry_id.to_owned(),
        before: before.entry(entry_id),
        provider: provider.clone(),
        provider_index: position,
        source_provider: source.clone(),
        after_revision: fingerprint(&after.entry(entry_id))?,
        after_provider_fingerprint: fingerprint(&if delete { None } else { provider })?,
    };
    let metadata = next_metadata(
        path,
        &before,
        &after,
        previous.as_ref(),
        entry_id,
        source,
        Some(backup),
    )?;
    persist_changes(
        path,
        transaction,
        &after,
        metadata,
        previous_bytes.as_deref(),
        config,
        next_config,
        hub_path,
        if delete { None } else { Some(entry_id) },
    )
}

// SQLite and TOML have no shared transaction. Hold the SQLite write lock until
// the versioned Hub save succeeds; compensate on commit failure without
// overwriting a concurrent Hub writer. No client restart or runtime token access.
fn persist_changes(
    path: &Path,
    transaction: rusqlite::Transaction<'_>,
    after: &ManagedSnapshot,
    metadata: Metadata,
    previous_bytes: Option<&[u8]>,
    config: &mut AppConfig,
    mut next_config: AppConfig,
    hub_path: &Path,
    selected: Option<&str>,
) -> Result<GmClawConfigStatus> {
    let hub_path = hub_path.to_path_buf();
    let result = status_from_snapshot(path, after, Some(&metadata), &next_config, selected)?;
    atomic_write(&metadata_path(path), &serde_json::to_vec_pretty(&metadata)?)?;
    if let Err(error) = next_config.save(&hub_path) {
        let recovery = restore_metadata(path, previous_bytes);
        transaction
            .rollback()
            .context("Hub 保存失败且天工 Claw 事务回滚失败，请重新读取两端配置")?;
        recovery.context("Hub 保存失败；天工 Claw 已回滚，但备份文件恢复失败")?;
        return Err(error).context("Hub 渠道保存失败，天工 Claw 配置已撤销");
    }
    if transaction.commit().is_err() {
        let mut rollback = config.clone();
        rollback.revision = next_config.revision.clone();
        let hub_recovered = rollback.save(&hub_path).is_ok();
        let metadata_recovered = restore_metadata(path, previous_bytes).is_ok();
        *config = if hub_recovered { rollback } else { next_config };
        if !hub_recovered || !metadata_recovered {
            bail!("天工 Claw 数据库提交失败，Hub 或备份自动恢复未完成，请刷新并检查两端配置");
        }
        bail!("天工 Claw 数据库提交失败，Hub 配置与备份已恢复，请重试");
    }
    *config = next_config;
    Ok(result)
}

pub fn restore_backup(
    revision: &str,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<GmClawConfigStatus> {
    restore_at(&config_path(), revision, config, hub_path)
}

fn restore_at(
    path: &Path,
    revision: &str,
    config: &mut AppConfig,
    hub_path: &Path,
) -> Result<GmClawConfigStatus> {
    let mut connection = open_existing(path, true)?;
    let columns = schema_columns(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .context("天工 Claw 数据库正在写入，请稍后重试")?;
    let current = snapshot(&transaction, &columns)?;
    check_revision(revision, &current, config)?;
    let (previous_bytes, metadata) = read_metadata(path)?;
    let metadata = metadata.ok_or_else(|| anyhow!("没有可还原的天工 Claw 接入备份"))?;
    let backup = metadata
        .backup
        .as_ref()
        .ok_or_else(|| anyhow!("没有可还原的天工 Claw 接入备份"))?;
    let entry_id = &backup.entry_id;
    validate_entry_id(entry_id)?;
    let target = current.entry(entry_id);
    // This remains the metadata v1 database-only target hash, not a UI token.
    ensure!(
        backup.after_revision == fingerprint(&target)?,
        "天工 Claw 目标模型或默认模型已被后续修改，已停止还原以保留这些改动"
    );
    let position = provider_position(config, entry_id)?;
    let provider = position.map(|index| config.ai_gateway.providers[index].clone());
    ensure!(
        backup.after_provider_fingerprint == fingerprint(&provider)?,
        "Hub 天工 Claw 目标渠道已被后续修改，已停止还原以保留这些改动"
    );
    let target_model_id = model_id(entry_id);
    if let Some(row) = &backup.before.row {
        ensure!(
            row.len() == columns.len() && row.keys().all(|name| columns.contains(name)),
            "备份字段与当前数据库不一致，不能安全还原"
        );
        ensure!(
            text(row, "model_id") == Some(target_model_id.as_str()),
            "备份模型身份不正确，已停止还原"
        );
    }
    let provider_name = gmclaw_provider_name(entry_id).expect("validated entry identity");
    ensure!(
        backup
            .provider
            .as_ref()
            .is_none_or(|provider| provider.name.eq_ignore_ascii_case(&provider_name)),
        "备份渠道身份不正确，已停止还原"
    );
    ensure!(
        backup.before.active_ids.len() <= 1,
        "备份默认模型状态不正确，已停止还原"
    );
    transaction.execute("UPDATE model_configs SET is_active=0 WHERE is_active=1", [])?;
    if let Some(row) = &backup.before.row {
        if target.row.is_some() {
            let assignments = row
                .keys()
                .filter(|name| name.as_str() != "model_id")
                .map(|name| format!("{}=?", identifier(name)))
                .collect::<Vec<_>>()
                .join(",");
            let values = row
                .iter()
                .filter(|(name, _)| name.as_str() != "model_id")
                .map(|(_, value)| value.sql_value())
                .chain(std::iter::once(Value::Text(target_model_id.clone())));
            transaction
                .execute(
                    &format!("UPDATE model_configs SET {assignments} WHERE model_id=?"),
                    params_from_iter(values),
                )
                .context("无法还原天工 Claw 目标模型")?;
        } else {
            // Undo deletion using the complete typed row, including timestamps
            // and unknown columns; never recreate the database or its schema.
            let names = row
                .keys()
                .map(|name| identifier(name))
                .collect::<Vec<_>>()
                .join(",");
            let placeholders = vec!["?"; row.len()].join(",");
            transaction
                .execute(
                    &format!("INSERT INTO model_configs ({names}) VALUES ({placeholders})"),
                    params_from_iter(row.values().map(StoredValue::sql_value)),
                )
                .context("无法恢复已删除的天工 Claw 模型")?;
        }
    } else {
        transaction.execute(
            "DELETE FROM model_configs WHERE model_id=?1",
            [&target_model_id],
        )?;
    }
    for id in &backup.before.active_ids {
        ensure!(
            transaction.execute(
                "UPDATE model_configs SET is_active=1 WHERE model_id=?1",
                [id]
            )? == 1,
            "之前的天工 Claw 默认模型已删除，不能安全还原"
        );
    }
    let after = snapshot(&transaction, &columns)?;
    let mut next_config = config.clone();
    replace_provider(
        &mut next_config,
        entry_id,
        backup.provider.clone(),
        backup.provider_index,
    );
    let next = next_metadata(
        path,
        &current,
        &after,
        Some(&metadata),
        entry_id,
        backup.source_provider.clone(),
        None,
    )?;
    persist_changes(
        path,
        transaction,
        &after,
        next,
        previous_bytes.as_deref(),
        config,
        next_config,
        hub_path,
        after
            .rows
            .contains_key(entry_id)
            .then_some(entry_id.as_str()),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_gateway::config::GMCLAW_PROVIDER_NAME;
    use crate::ai_gateway::gmclaw::TemperatureMode;

    #[test]
    fn session_model_choices_read_native_and_managed_ids_without_secret_columns() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("models.db");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch(
            "CREATE TABLE model_configs (model_id TEXT PRIMARY KEY, model_name TEXT NOT NULL, is_active INTEGER NOT NULL);
             INSERT INTO model_configs VALUES ('native-row', 'shared-name', 1);
             INSERT INTO model_configs VALUES ('tiancaispacehub-managed', 'shared-name', 0);"
        ).unwrap();
        drop(connection);

        let (choices, active) = model_choices_at(&database).unwrap();
        assert_eq!(active.as_deref(), Some("native-row"));
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].value, "native-row");
        assert_eq!(choices[1].value, "tiancaispacehub-managed");
        assert_ne!(choices[0].label, choices[1].label);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn session_model_choices_reject_ambiguous_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("models.db");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch(
            "CREATE TABLE model_configs (model_id TEXT PRIMARY KEY, model_name TEXT NOT NULL, is_active INTEGER NOT NULL);
             INSERT INTO model_configs VALUES ('row-one', 'model', 1);
             INSERT INTO model_configs VALUES ('row-two', 'model', 1);"
        ).unwrap();
        drop(connection);
        assert!(model_choices_at(&database).is_err());
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, AppConfig) {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("electron-data.db");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("CREATE TABLE model_configs (model_id TEXT PRIMARY KEY,provider TEXT NOT NULL DEFAULT 'openai',base_url TEXT NOT NULL DEFAULT '',model_name TEXT NOT NULL DEFAULT '',api_key TEXT NOT NULL DEFAULT '',max_tokens INTEGER DEFAULT 128000,temperature REAL DEFAULT 0.7,is_active INTEGER NOT NULL DEFAULT 0,extra_params TEXT DEFAULT '{}',created_at TEXT NOT NULL DEFAULT (datetime('now')),updated_at TEXT NOT NULL DEFAULT (datetime('now')),future_field TEXT DEFAULT 'preserved'); CREATE UNIQUE INDEX idx_model_active ON model_configs(is_active) WHERE is_active=1; INSERT INTO model_configs(model_id,model_name,is_active) VALUES('existing','other-model',1);").unwrap();
        let hub_path = directory.path().join("hub.toml");
        let mut config = AppConfig::default();
        config.ai_gateway.providers.push(ProviderConfig {
            name: "source".into(),
            base_url: "https://example.invalid/v1".into(),
            models: vec!["real-model".into()],
            model_aliases: BTreeMap::from([("alias".into(), "real-model".into())]),
            api_key: "test-upstream-only".into(),
            ..Default::default()
        });
        config.save(&hub_path).unwrap();
        (directory, database, hub_path, config)
    }

    fn request(path: &Path, config: &AppConfig, entry_id: Option<&str>) -> GmClawSaveRequest {
        GmClawSaveRequest {
            entry_id: entry_id.map(str::to_owned),
            source_provider: "source".into(),
            model: "alias".into(),
            revision: load_at(path, config, None).unwrap().revision,
            ..Default::default()
        }
    }

    fn entry_request(path: &Path, config: &AppConfig, entry_id: &str) -> GmClawEntryRequest {
        GmClawEntryRequest {
            entry_id: entry_id.into(),
            revision: load_at(path, config, None).unwrap().revision,
        }
    }

    fn read_snapshot(path: &Path) -> ManagedSnapshot {
        let connection = Connection::open(path).unwrap();
        snapshot(&connection, &schema_columns(&connection).unwrap()).unwrap()
    }

    #[test]
    fn create_same_model_on_independent_channels_and_keep_existing_default() {
        let (_directory, path, hub_path, mut config) = fixture();
        let mut first = request(&path, &config, None);
        first.make_active = false;
        let saved = save_at(&path, &first, &mut config, &hub_path).unwrap();
        let first_id = saved.selected_entry_id.unwrap();
        assert_eq!(first_id.len(), 32);
        assert!(!saved.active && !saved.restart_required);
        assert_eq!(saved.active_model_id.as_deref(), Some("existing"));

        let mut second_source = config.ai_gateway.providers[0].clone();
        second_source.name = "second-source".into();
        second_source.base_url = "https://second.example.invalid/v1".into();
        config.ai_gateway.providers.push(second_source);
        config.save(&hub_path).unwrap();
        let mut second = request(&path, &config, None);
        second.source_provider = "second-source".into();
        let saved = save_at(&path, &second, &mut config, &hub_path).unwrap();
        let second_id = saved.selected_entry_id.clone().unwrap();
        assert_ne!(first_id, second_id);
        assert_eq!(saved.entries.len(), 2);
        assert_eq!(saved.entries.iter().filter(|entry| entry.active).count(), 1);
        assert_eq!(saved.active_model_id, Some(model_id(&second_id)));
        let first = saved
            .entries
            .iter()
            .find(|entry| entry.entry_id == first_id)
            .unwrap();
        let second = saved
            .entries
            .iter()
            .find(|entry| entry.entry_id == second_id)
            .unwrap();
        assert_eq!(first.model.model_name, second.model.model_name);
        assert_ne!(first.local_url, second.local_url);
        assert_eq!(first.source_provider.as_deref(), Some("source"));
        assert_eq!(second.source_provider.as_deref(), Some("second-source"));
        assert!(first.local_url.ends_with(&format!("/gmclaw/{first_id}/v1")));
        assert!(
            second
                .local_url
                .ends_with(&format!("/gmclaw/{second_id}/v1"))
        );
        assert_eq!(
            config
                .ai_gateway
                .providers
                .iter()
                .filter(|provider| provider.is_gmclaw())
                .count(),
            2
        );
        let snapshot = read_snapshot(&path);
        for row in snapshot.rows.values() {
            assert_eq!(text(row, "api_key"), Some(GMCLAW_LOCAL_KEY));
        }
        let connection = Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT model_name FROM model_configs WHERE model_id='existing'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "other-model"
        );
    }

    #[test]
    fn external_default_switch_keeps_sources_but_not_stale_write_or_undo_tokens() {
        let (_directory, path, hub_path, mut config) = fixture();
        let first = save_at(
            &path,
            &request(&path, &config, None),
            &mut config,
            &hub_path,
        )
        .unwrap();
        let first_id = first.selected_entry_id.unwrap();
        let mut second = request(&path, &config, None);
        second.make_active = false;
        let second = save_at(&path, &second, &mut config, &hub_path).unwrap();
        let second_id = second.selected_entry_id.unwrap();
        let stale = request(&path, &config, Some(&first_id));
        let connection = Connection::open(&path).unwrap();
        connection
            .execute("UPDATE model_configs SET is_active=0", [])
            .unwrap();
        connection
            .execute(
                "UPDATE model_configs SET is_active=1,updated_at='external-default-change' WHERE model_id=?1",
                [model_id(&second_id)],
            )
            .unwrap();
        let current = load_at(&path, &config, None).unwrap();
        assert_ne!(current.revision, stale.revision);
        assert!(
            current
                .entries
                .iter()
                .all(|entry| { entry.source_provider.as_deref() == Some("source") })
        );
        assert!(save_at(&path, &stale, &mut config, &hub_path).is_err());
        // Association hashes are deliberately weaker than undo/write guards.
        let before_failed_restore = fingerprint(&read_snapshot(&path)).unwrap();
        assert!(restore_at(&path, &current.revision, &mut config, &hub_path).is_err());
        assert_eq!(
            fingerprint(&read_snapshot(&path)).unwrap(),
            before_failed_restore
        );

        let mut update = request(&path, &config, Some(&first_id));
        update.make_active = false;
        let saved = save_at(&path, &update, &mut config, &hub_path).unwrap();
        assert_eq!(saved.active_model_id, Some(model_id(&second_id)));
        assert!(
            saved
                .entries
                .iter()
                .all(|entry| { entry.source_provider.as_deref() == Some("source") })
        );
        // A route/model edit still invalidates the source; unknown fields are
        // also conservative even though they are not managed by this version.
        for (column, value) in [
            ("model_name", "changed-model"),
            ("base_url", "http://127.0.0.1:9999/changed"),
            ("future_field", "externally-changed"),
        ] {
            let before = read_snapshot(&path).rows[&first_id].clone();
            connection
                .execute(
                    &format!(
                        "UPDATE model_configs SET {}=?2 WHERE model_id=?1",
                        identifier(column)
                    ),
                    params![model_id(&first_id), value],
                )
                .unwrap();
            let changed = load_at(&path, &config, Some(&first_id)).unwrap();
            assert!(changed.source_provider.is_none(), "{column}");
            connection
                .execute(
                    &format!(
                        "UPDATE model_configs SET {}=?2 WHERE model_id=?1",
                        identifier(column)
                    ),
                    params![model_id(&first_id), before[column].sql_value()],
                )
                .unwrap();
        }
    }

    #[test]
    fn legacy_source_hashes_match_active_flags_but_not_unknown_old_timestamps() {
        for version in [1, 2] {
            let (_directory, path, hub_path, mut config) = fixture();
            let connection = Connection::open(&path).unwrap();
            connection
                .execute(
                    "INSERT INTO model_configs(model_id,model_name) VALUES(?1,'original')",
                    [GMCLAW_MODEL_ID],
                )
                .unwrap();
            save_at(
                &path,
                &request(&path, &config, Some(GMCLAW_LEGACY_ENTRY_ID)),
                &mut config,
                &hub_path,
            )
            .unwrap();
            let (_, metadata) = read_metadata(&path).unwrap();
            let mut metadata = serde_json::to_value(metadata.unwrap()).unwrap();
            let legacy_hash =
                fingerprint(&read_snapshot(&path).entry(GMCLAW_LEGACY_ENTRY_ID).row).unwrap();
            metadata["version"] = serde_json::json!(version);
            if version == 1 {
                metadata["source_provider"] = serde_json::json!("source");
                metadata["row_fingerprint"] = serde_json::json!(legacy_hash);
                metadata.as_object_mut().unwrap().remove("entries");
            } else {
                metadata["entries"][GMCLAW_LEGACY_ENTRY_ID]["row_fingerprint"] =
                    serde_json::json!(legacy_hash);
            }
            atomic_write(
                &metadata_path(&path),
                &serde_json::to_vec(&metadata).unwrap(),
            )
            .unwrap();
            connection.execute_batch("UPDATE model_configs SET is_active=0; UPDATE model_configs SET is_active=1 WHERE model_id='existing';").unwrap();
            let current = load_at(&path, &config, Some(GMCLAW_LEGACY_ENTRY_ID)).unwrap();
            assert_eq!(current.source_provider.as_deref(), Some("source"));
            connection
                .execute(
                    "UPDATE model_configs SET updated_at='unknown-new-timestamp' WHERE model_id=?1",
                    [GMCLAW_MODEL_ID],
                )
                .unwrap();
            let changed = load_at(&path, &config, Some(GMCLAW_LEGACY_ENTRY_ID)).unwrap();
            assert!(changed.source_provider.is_none());
            // Explicit save upgrades the association without weakening restore.
            let saved = save_at(
                &path,
                &request(&path, &config, Some(GMCLAW_LEGACY_ENTRY_ID)),
                &mut config,
                &hub_path,
            )
            .unwrap();
            assert_eq!(saved.source_provider.as_deref(), Some("source"));
            let (_, upgraded) = read_metadata(&path).unwrap();
            assert!(
                upgraded.unwrap().entries[GMCLAW_LEGACY_ENTRY_ID]
                    .row_fingerprint
                    .starts_with(SOURCE_FINGERPRINT_PREFIX)
            );
        }
    }

    #[test]
    fn update_only_selected_entry_preserves_unknown_fields_and_other_parameters() {
        let (_directory, path, hub_path, mut config) = fixture();
        let first = save_at(
            &path,
            &request(&path, &config, None),
            &mut config,
            &hub_path,
        )
        .unwrap();
        let first_id = first.selected_entry_id.unwrap();
        let mut second_request = request(&path, &config, None);
        second_request.make_active = false;
        second_request.parameters.reasoning_effort = Some("high".into());
        let second = save_at(&path, &second_request, &mut config, &hub_path).unwrap();
        let second_id = second.selected_entry_id.unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE model_configs SET future_field='keep',extra_params=?2 WHERE model_id=?1",
                params![
                    model_id(&first_id),
                    r#"{"context_window":65536,"futureOption":true}"#
                ],
            )
            .unwrap();
        let before = read_snapshot(&path);
        let mut update = request(&path, &config, Some(&first_id));
        update.make_active = false;
        update.max_tokens = 4096;
        update.parameters.temperature_mode = TemperatureMode::Omit;
        let updated = save_at(&path, &update, &mut config, &hub_path).unwrap();
        assert!(updated.active);
        assert_eq!(updated.model.unwrap().context_window, 65536);
        let after = read_snapshot(&path);
        assert_eq!(
            text(after.rows.get(&first_id).unwrap(), "future_field"),
            Some("keep")
        );
        assert_eq!(
            fingerprint(&before.rows.get(&second_id)).unwrap(),
            fingerprint(&after.rows.get(&second_id)).unwrap()
        );
        let extra = row_extra(after.rows.get(&first_id)).unwrap();
        assert_eq!(
            extra,
            serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                serde_json::json!({"context_window":65536,"futureOption":true})
            )
            .unwrap()
        );
        assert_eq!(updated.parameters.temperature_mode, TemperatureMode::Omit);
        let second = load_at(&path, &config, Some(&second_id)).unwrap();
        assert_eq!(second.parameters.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(second.entries.len(), 2);
        let create_view = load_at(&path, &config, Some("")).unwrap();
        assert!(create_view.selected_entry_id.is_none() && create_view.model.is_none());
        assert_eq!(create_view.entries.len(), 2);
    }

    #[test]
    fn activate_delete_and_restore_target_without_changing_other_entries() {
        let (_directory, path, hub_path, mut config) = fixture();
        let first = save_at(
            &path,
            &request(&path, &config, None),
            &mut config,
            &hub_path,
        )
        .unwrap();
        let first_id = first.selected_entry_id.unwrap();
        let mut second = request(&path, &config, None);
        second.make_active = false;
        let second = save_at(&path, &second, &mut config, &hub_path).unwrap();
        let second_id = second.selected_entry_id.unwrap();
        let before = fingerprint(&read_snapshot(&path)).unwrap();
        let activated = mutate_entry_at(
            &path,
            &entry_request(&path, &config, &second_id),
            &mut config,
            &hub_path,
            false,
        )
        .unwrap();
        assert_eq!(activated.active_model_id, Some(model_id(&second_id)));
        assert!(!activated.restart_required);
        let restored = restore_at(&path, &activated.revision, &mut config, &hub_path).unwrap();
        assert_eq!(restored.active_model_id, Some(model_id(&first_id)));
        assert_eq!(fingerprint(&read_snapshot(&path)).unwrap(), before);
        assert!(!restored.backup_exists);

        let deleted = mutate_entry_at(
            &path,
            &entry_request(&path, &config, &second_id),
            &mut config,
            &hub_path,
            true,
        )
        .unwrap();
        assert_eq!(deleted.entries.len(), 1);
        assert_eq!(deleted.active_model_id, Some(model_id(&first_id)));
        assert!(provider_position(&config, &second_id).unwrap().is_none());
        let restored = restore_at(&path, &deleted.revision, &mut config, &hub_path).unwrap();
        assert_eq!(
            restored.selected_entry_id.as_deref(),
            Some(second_id.as_str())
        );
        assert_eq!(fingerprint(&read_snapshot(&path)).unwrap(), before);
        assert!(provider_position(&config, &second_id).unwrap().is_some());

        let deleted_active = mutate_entry_at(
            &path,
            &entry_request(&path, &config, &first_id),
            &mut config,
            &hub_path,
            true,
        )
        .unwrap();
        assert!(deleted_active.active_model_id.is_none());
        let restored = restore_at(&path, &deleted_active.revision, &mut config, &hub_path).unwrap();
        assert_eq!(restored.active_model_id, Some(model_id(&first_id)));
        assert!(!restored.restart_required);
    }

    #[test]
    fn undo_new_entry_removes_only_that_entry_and_restores_original_default() {
        let (_directory, path, hub_path, mut config) = fixture();
        let before = load_at(&path, &config, None).unwrap().revision;
        let saved = save_at(
            &path,
            &request(&path, &config, None),
            &mut config,
            &hub_path,
        )
        .unwrap();
        let restored = restore_at(&path, &saved.revision, &mut config, &hub_path).unwrap();
        assert!(restored.entries.is_empty());
        assert!(!restored.configured && !restored.backup_exists && !restored.restart_required);
        assert_eq!(restored.revision, before);
        assert_eq!(restored.active_model_id.as_deref(), Some("existing"));
        assert_eq!(config.ai_gateway.providers.len(), 1);
    }

    #[test]
    fn old_metadata_v1_and_legacy_provider_restore_with_new_ui_revision() {
        let (_directory, path, hub_path, mut config) = fixture();
        let connection = Connection::open(&path).unwrap();
        connection.execute("INSERT INTO model_configs(model_id,model_name,extra_params,future_field) VALUES(?1,'original',?2,'keep')", params![GMCLAW_MODEL_ID, r#"{"context_window":65536,"futureOption":true}"#]).unwrap();
        let mut provider = config.ai_gateway.providers[0].clone();
        provider.name = GMCLAW_PROVIDER_NAME.into();
        config.ai_gateway.providers.push(provider);
        config.save(&hub_path).unwrap();
        let before = fingerprint(&read_snapshot(&path)).unwrap();
        let saved = save_at(
            &path,
            &request(&path, &config, Some(GMCLAW_LEGACY_ENTRY_ID)),
            &mut config,
            &hub_path,
        )
        .unwrap();
        assert_eq!(
            saved.selected_entry_id.as_deref(),
            Some(GMCLAW_LEGACY_ENTRY_ID)
        );
        assert!(saved.local_url.ends_with("/ai-gateway/gmclaw/v1"));
        let position = provider_position(&config, GMCLAW_LEGACY_ENTRY_ID)
            .unwrap()
            .unwrap();
        config.ai_gateway.providers[position].gmclaw_parameters = None;
        config.save(&hub_path).unwrap();
        let (_, metadata) = read_metadata(&path).unwrap();
        let mut metadata = metadata.unwrap();
        metadata.backup.as_mut().unwrap().after_provider_fingerprint =
            fingerprint(&Some(&config.ai_gateway.providers[position])).unwrap();
        let target = read_snapshot(&path).entry(GMCLAW_LEGACY_ENTRY_ID);
        assert_eq!(
            metadata.backup.as_ref().unwrap().after_revision,
            fingerprint(&target).unwrap()
        );
        let mut old = serde_json::to_value(&metadata).unwrap();
        old["version"] = serde_json::json!(1);
        old["source_provider"] = serde_json::json!("source");
        old["row_fingerprint"] = serde_json::json!(fingerprint(&target.row).unwrap());
        old.as_object_mut().unwrap().remove("entries");
        old["backup"].as_object_mut().unwrap().remove("entry_id");
        let historical = serde_json::to_vec_pretty(&old).unwrap();
        assert!(!String::from_utf8_lossy(&historical).contains("gmclawParameters"));
        atomic_write(&metadata_path(&path), &historical).unwrap();
        let current = load_at(&path, &config, None).unwrap();
        assert_eq!(current.source_provider.as_deref(), Some("source"));
        let restored = restore_at(&path, &current.revision, &mut config, &hub_path).unwrap();
        assert!(!restored.backup_exists);
        assert_eq!(fingerprint(&read_snapshot(&path)).unwrap(), before);
        assert!(
            config.ai_gateway.providers[position]
                .gmclaw_parameters
                .is_none()
        );
    }

    #[test]
    fn managed_provider_change_blocks_all_stale_operations_without_writes() {
        let (_directory, path, hub_path, mut config) = fixture();
        let first = save_at(
            &path,
            &request(&path, &config, None),
            &mut config,
            &hub_path,
        )
        .unwrap();
        let first_id = first.selected_entry_id.unwrap();
        let second = save_at(
            &path,
            &request(&path, &config, None),
            &mut config,
            &hub_path,
        )
        .unwrap();
        let second_id = second.selected_entry_id.unwrap();
        let stale_save = request(&path, &config, Some(&second_id));
        let stale_action = entry_request(&path, &config, &second_id);
        let position = provider_position(&config, &first_id).unwrap().unwrap();
        config.ai_gateway.providers[position]
            .gmclaw_parameters
            .as_mut()
            .unwrap()
            .reasoning_effort = Some("high".into());
        config.save(&hub_path).unwrap();
        let current = load_at(&path, &config, None).unwrap();
        assert_ne!(current.revision, stale_save.revision);
        let db_before = fingerprint(&read_snapshot(&path)).unwrap();
        let hub_before = fs::read(&hub_path).unwrap();
        let backup_before = fs::read(metadata_path(&path)).unwrap();
        assert!(
            save_at(&path, &stale_save, &mut config, &hub_path)
                .unwrap_err()
                .to_string()
                .contains("请刷新后重试")
        );
        for delete in [false, true] {
            assert!(
                mutate_entry_at(&path, &stale_action, &mut config, &hub_path, delete)
                    .unwrap_err()
                    .to_string()
                    .contains("请刷新后重试")
            );
        }
        assert!(
            restore_at(&path, &stale_action.revision, &mut config, &hub_path)
                .unwrap_err()
                .to_string()
                .contains("请刷新后重试")
        );
        assert_eq!(fingerprint(&read_snapshot(&path)).unwrap(), db_before);
        assert_eq!(fs::read(&hub_path).unwrap(), hub_before);
        assert_eq!(fs::read(metadata_path(&path)).unwrap(), backup_before);
    }

    #[test]
    fn managed_rows_and_default_changes_invalidate_revision_but_unrelated_edits_do_not() {
        let (_directory, path, hub_path, mut config) = fixture();
        let saved = save_at(
            &path,
            &request(&path, &config, None),
            &mut config,
            &hub_path,
        )
        .unwrap();
        let entry_id = saved.selected_entry_id.unwrap();
        let pending = request(&path, &config, Some(&entry_id));
        config.ai_gateway.providers[0].api_key = "updated-source-only".into();
        config.ai_gateway.providers.insert(
            0,
            ProviderConfig {
                name: "unrelated".into(),
                ..Default::default()
            },
        );
        config.save(&hub_path).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE model_configs SET model_name='external-edit' WHERE model_id='existing'",
                [],
            )
            .unwrap();
        assert_eq!(
            load_at(&path, &config, None).unwrap().revision,
            pending.revision
        );
        save_at(&path, &pending, &mut config, &hub_path).unwrap();
        assert_eq!(config.ai_gateway.providers[0].name, "unrelated");
        let before = load_at(&path, &config, None).unwrap().revision;
        connection
            .execute(
                "UPDATE model_configs SET future_field='user-edit' WHERE model_id=?1",
                [model_id(&entry_id)],
            )
            .unwrap();
        assert_ne!(load_at(&path, &config, None).unwrap().revision, before);
        let before = load_at(&path, &config, None).unwrap().revision;
        connection.execute_batch("UPDATE model_configs SET is_active=0 WHERE is_active=1; UPDATE model_configs SET is_active=1 WHERE model_id='existing';").unwrap();
        assert_ne!(load_at(&path, &config, None).unwrap().revision, before);
    }

    #[test]
    fn old_ui_tokens_require_refresh_while_backup_remains_database_only() {
        let (_directory, path, hub_path, mut config) = fixture();
        let saved = save_at(
            &path,
            &request(&path, &config, None),
            &mut config,
            &hub_path,
        )
        .unwrap();
        let entry_id = saved.selected_entry_id.unwrap();
        let (_, metadata) = read_metadata(&path).unwrap();
        let legacy_token = metadata.unwrap().backup.unwrap().after_revision;
        assert_eq!(
            legacy_token,
            fingerprint(&read_snapshot(&path).entry(&entry_id)).unwrap()
        );
        for old_token in [legacy_token, "gmclaw-v2:old-token".into()] {
            let mut pending = request(&path, &config, Some(&entry_id));
            pending.revision = old_token.clone();
            assert!(
                save_at(&path, &pending, &mut config, &hub_path)
                    .unwrap_err()
                    .to_string()
                    .contains("请刷新后重试")
            );
            assert!(
                restore_at(&path, &old_token, &mut config, &hub_path)
                    .unwrap_err()
                    .to_string()
                    .contains("请刷新后重试")
            );
        }
    }

    #[test]
    fn hub_revision_failure_rolls_back_save_delete_and_activation_with_backup() {
        let (_directory, path, hub_path, mut stale) = fixture();
        let saved = save_at(&path, &request(&path, &stale, None), &mut stale, &hub_path).unwrap();
        let entry_id = saved.selected_entry_id.unwrap();
        let db_before = fingerprint(&read_snapshot(&path)).unwrap();
        let backup_before = fs::read(metadata_path(&path)).unwrap();
        let mut newer = stale.clone();
        newer.language = Some("en-US".into());
        newer.save(&hub_path).unwrap();
        let hub_before = fs::read(&hub_path).unwrap();
        assert!(save_at(&path, &request(&path, &stale, None), &mut stale, &hub_path).is_err());
        for delete in [false, true] {
            assert!(
                mutate_entry_at(
                    &path,
                    &entry_request(&path, &stale, &entry_id),
                    &mut stale,
                    &hub_path,
                    delete
                )
                .is_err()
            );
        }
        assert_eq!(fingerprint(&read_snapshot(&path)).unwrap(), db_before);
        assert_eq!(fs::read(metadata_path(&path)).unwrap(), backup_before);
        assert_eq!(fs::read(&hub_path).unwrap(), hub_before);
    }

    #[test]
    fn restore_preserves_unrelated_managed_edits_but_rejects_target_changes() {
        let (_directory, path, hub_path, mut config) = fixture();
        let first = save_at(
            &path,
            &request(&path, &config, None),
            &mut config,
            &hub_path,
        )
        .unwrap();
        let first_id = first.selected_entry_id.unwrap();
        let mut second = request(&path, &config, None);
        second.make_active = false;
        let second = save_at(&path, &second, &mut config, &hub_path).unwrap();
        let second_id = second.selected_entry_id.unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE model_configs SET future_field='unrelated-edit' WHERE model_id=?1",
                [model_id(&first_id)],
            )
            .unwrap();
        let current = load_at(&path, &config, None).unwrap();
        let restored = restore_at(&path, &current.revision, &mut config, &hub_path).unwrap();
        assert_eq!(restored.entries.len(), 1);
        assert_eq!(
            text(
                read_snapshot(&path).rows.get(&first_id).unwrap(),
                "future_field"
            ),
            Some("unrelated-edit")
        );
        assert!(provider_position(&config, &second_id).unwrap().is_none());
        let saved = save_at(
            &path,
            &request(&path, &config, Some(&first_id)),
            &mut config,
            &hub_path,
        )
        .unwrap();
        connection
            .execute(
                "UPDATE model_configs SET future_field='target-edit' WHERE model_id=?1",
                [model_id(&first_id)],
            )
            .unwrap();
        let current = load_at(&path, &config, None).unwrap();
        assert_ne!(saved.revision, current.revision);
        assert!(restore_at(&path, &current.revision, &mut config, &hub_path).is_err());
    }

    #[test]
    fn parameters_remain_in_hub_and_restore_previous_selection() {
        let (_directory, path, hub_path, mut config) = fixture();
        let mut first = request(&path, &config, None);
        first.parameters = GmClawParameters {
            reasoning_effort: Some("high".into()),
            temperature_mode: TemperatureMode::Omit,
        };
        let saved = save_at(&path, &first, &mut config, &hub_path).unwrap();
        let entry_id = saved.selected_entry_id.unwrap();
        let row = read_snapshot(&path).rows.remove(&entry_id).unwrap();
        assert_eq!(
            row_extra(Some(&row)).unwrap(),
            serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                serde_json::json!({"context_window":128000})
            )
            .unwrap()
        );
        let loaded = AppConfig::load_or_default(&hub_path).unwrap();
        assert!(loaded.ai_gateway.providers[0].gmclaw_parameters.is_none());
        let mut second = request(&path, &config, Some(&entry_id));
        second.parameters = GmClawParameters {
            reasoning_effort: Some("max".into()),
            temperature_mode: TemperatureMode::Preserve,
        };
        let saved = save_at(&path, &second, &mut config, &hub_path).unwrap();
        let restored = restore_at(&path, &saved.revision, &mut config, &hub_path).unwrap();
        assert_eq!(restored.parameters, first.parameters);
    }

    #[test]
    fn missing_database_invalid_entry_and_source_do_not_write() {
        let (_directory, path, hub_path, mut config) = fixture();
        let missing = path.with_file_name("not-installed.db");
        assert!(!load_at(&missing, &config, None).unwrap().exists);
        assert!(
            save_at(
                &missing,
                &request(&path, &config, None),
                &mut config,
                &hub_path
            )
            .is_err()
        );
        assert!(!missing.exists());
        for invalid in ["../unsafe", "not-existing", "legacy"] {
            assert!(
                save_at(
                    &path,
                    &request(&path, &config, Some(invalid)),
                    &mut config,
                    &hub_path
                )
                .is_err()
            );
        }
        config.ai_gateway.providers[0].base_url =
            local_url(&config, GMCLAW_LEGACY_ENTRY_ID).unwrap();
        assert!(
            save_at(
                &path,
                &request(&path, &config, None),
                &mut config,
                &hub_path
            )
            .is_err()
        );
        assert!(!metadata_path(&path).exists());
        assert!(read_snapshot(&path).rows.is_empty());
    }

    #[test]
    fn invalid_parameters_and_native_claude_none_fail_before_writes() {
        let (_directory, path, hub_path, mut config) = fixture();
        let mut pending = request(&path, &config, None);
        pending.parameters.reasoning_effort = Some("unsupported".into());
        assert!(save_at(&path, &pending, &mut config, &hub_path).is_err());
        config.ai_gateway.providers[0].provider_type = ProviderType::AnthropicMessages;
        config.save(&hub_path).unwrap();
        let hub_before = fs::read(&hub_path).unwrap();
        pending.parameters.reasoning_effort = Some("none".into());
        assert!(
            save_at(&path, &pending, &mut config, &hub_path)
                .unwrap_err()
                .to_string()
                .contains("Claude")
        );
        assert_eq!(fs::read(&hub_path).unwrap(), hub_before);
        assert!(!metadata_path(&path).exists());
        assert!(read_snapshot(&path).rows.is_empty());
        config.ai_gateway.providers[0].compatibility = Some("glm_anthropic".into());
        config.save(&hub_path).unwrap();
        let saved = save_at(&path, &pending, &mut config, &hub_path).unwrap();
        assert_eq!(saved.parameters.reasoning_effort.as_deref(), Some("none"));
    }

    #[test]
    fn old_save_payload_defaults_to_explicit_new_entry_and_automatic_parameters() {
        let old: GmClawSaveRequest = serde_json::from_value(serde_json::json!({
            "sourceProvider":"source", "model":"alias", "maxTokens":8192,
            "temperature":0.7, "revision":"old-page"
        }))
        .unwrap();
        assert!(old.entry_id.is_none() && old.make_active);
        assert_eq!(old.parameters, GmClawParameters::default());
    }
}
