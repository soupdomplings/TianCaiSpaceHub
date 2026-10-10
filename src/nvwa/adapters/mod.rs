//! Target-only client MCP edits. Public status never contains local credentials.
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::gmclaw_desktop::DesktopClient;

use super::{config::ConfigStore, secrets::SecretStore, types::ClientKind};

mod codex;
mod tiangong;
mod workbuddy;

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 1024 * 1024;

#[derive(Clone)]
pub(crate) struct AdapterContext {
    pub config_path: PathBuf,
    pub secrets: SecretStore,
    /// Supplied only after the runtime verified the current official instance.
    pub desktop: Option<DesktopClient>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterTarget {
    pub profile_id: String,
    pub client: ClientKind,
    pub server_name: String,
    #[serde(default)]
    pub override_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AdapterOperation {
    Apply,
    Remove,
    Restore,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterStatus {
    pub client: ClientKind,
    pub server_name: String,
    pub target_path: Option<String>,
    pub available: bool,
    pub present: bool,
    pub configured: bool,
    pub owned: bool,
    pub modified: bool,
    pub enabled: Option<bool>,
    pub target_fingerprint: String,
    pub backup_ref: Option<String>,
    pub load_state: String,
    pub connection_state: String,
    pub native_connected: Option<bool>,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterPreview {
    pub operation: AdapterOperation,
    pub status: AdapterStatus,
    pub can_apply: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterOutcome {
    pub status: AdapterStatus,
    pub mutation_done: bool,
    pub backup_ref: Option<String>,
    pub warnings: Vec<String>,
}

// This type can contain credentials. It deliberately has no Debug implementation.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "format", content = "entry")]
enum Snapshot {
    Toml(String),
    Json(Value),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagedEntry {
    profile_id: String,
    client: ClientKind,
    server_name: String,
    target_path: Option<PathBuf>,
    fingerprint: String,
    backup_ref: String,
    #[serde(default)]
    pending: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ledger {
    version: u32,
    entries: BTreeMap<String, ManagedEntry>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            version: 1,
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtectedBackup {
    version: u32,
    target_key: String,
    before: Option<Snapshot>,
    after_fingerprint: String,
    previous_managed: Option<ManagedEntry>,
}

#[derive(Serialize, Deserialize)]
struct BackupManifest {
    version: u32,
    chunked: bool,
    parts: usize,
    length: usize,
    sha256: String,
}

struct Location {
    path: Option<PathBuf>,
    key: String,
}
struct TargetRead {
    snapshot: Option<Snapshot>,
    file_bytes: Option<Vec<u8>>,
}

pub(crate) fn managed_server_name(profile_id: &str) -> Result<String> {
    ensure!(
        !profile_id.is_empty()
            && profile_id.len() <= 128
            && profile_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "NVWA 环境标识无效"
    );
    Ok(format!(
        "nvwa-{}",
        &hex::encode(Sha256::digest(profile_id.as_bytes()))[..16]
    ))
}

fn location(target: &AdapterTarget) -> Result<Location> {
    ensure!(
        target.server_name == managed_server_name(&target.profile_id)?,
        "NVWA MCP 受管条目名无效"
    );
    let path = match target.client {
        ClientKind::Codex => Some(
            target
                .override_path
                .clone()
                .unwrap_or_else(codex::default_path),
        ),
        ClientKind::Workbuddy => Some(match &target.override_path {
            Some(path) => path.clone(),
            None => workbuddy::default_path()?,
        }),
        ClientKind::Tiangong => {
            ensure!(
                target.override_path.is_none(),
                "天工 MCP 使用官方本机接口，不能指定数据库或文件路径"
            );
            None
        }
    };
    if let Some(path) = &path {
        ensure!(path.is_absolute(), "MCP 配置路径必须是绝对路径");
        let expected = if target.client == ClientKind::Codex {
            "config.toml"
        } else {
            "mcp.json"
        };
        ensure!(
            path.file_name().is_some_and(|name| name == expected),
            "MCP 配置路径文件名不匹配"
        );
        validate_path(path)?;
    }
    let identity = serde_json::to_vec(&(
        target.profile_id.as_str(),
        target.client,
        target.server_name.as_str(),
        &path,
    ))?;
    Ok(Location {
        path,
        key: hex::encode(Sha256::digest(identity)),
    })
}

fn metadata_root(ctx: &AdapterContext) -> Result<PathBuf> {
    Ok(ConfigStore::new(&ctx.config_path)?.root().to_path_buf())
}

async fn ledger_lock(root: &Path) -> Result<File> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        validate_path(&root)?;
        fs::create_dir_all(&root).map_err(|_| anyhow::anyhow!("无法创建 NVWA 接入元数据目录"))?;
        let path = root.join("managed.lock");
        validate_path(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| anyhow::anyhow!("无法打开 NVWA 接入操作锁"))?;
        file.lock_exclusive()
            .map_err(|_| anyhow::anyhow!("无法锁定 NVWA 接入操作"))?;
        Ok(file)
    })
    .await
    .map_err(|_| anyhow::anyhow!("NVWA 接入操作锁中断"))?
}

fn load_ledger(root: &Path) -> Result<Ledger> {
    let path = root.join("managed.json");
    validate_path(&path)?;
    let Some(bytes) = read_bounded(&path, MAX_METADATA_BYTES)? else {
        return Ok(Ledger::default());
    };
    let ledger: Ledger =
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("NVWA 接入元数据格式无效"))?;
    ensure!(ledger.version == 1, "不支持此 NVWA 接入元数据版本");
    Ok(ledger)
}

fn save_ledger(root: &Path, ledger: &Ledger) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(ledger)
        .map_err(|_| anyhow::anyhow!("无法编码 NVWA 接入元数据"))?;
    ensure!(
        bytes.len() as u64 <= MAX_METADATA_BYTES,
        "NVWA 接入元数据超过限制"
    );
    atomic_write(&root.join("managed.json"), &bytes)
}

pub(crate) async fn inspect(ctx: &AdapterContext, target: &AdapterTarget) -> Result<AdapterStatus> {
    let location = location(target)?;
    let root = metadata_root(ctx)?;
    let _lock = ledger_lock(&root).await?;
    let mut ledger = load_ledger(&root)?;
    if target.client == ClientKind::Tiangong && ctx.desktop.is_none() {
        return Ok(unavailable(target, ledger.entries.get(&location.key)));
    }
    let read = read_target(ctx, target, &location).await?;
    reconcile_journal(ctx, target, &location, &root, &read, &mut ledger)?;
    status(target, &location, &read, ledger.entries.get(&location.key))
}

/// Check all recorded explicit paths, not only the three default targets.
/// Confirmed removals keep a missing-target tombstone solely for restore.
pub(crate) async fn has_managed_profile(ctx: &AdapterContext, profile_id: &str) -> Result<bool> {
    managed_server_name(profile_id)?;
    let root = metadata_root(ctx)?;
    let _lock = ledger_lock(&root).await?;
    let ledger = load_ledger(&root)?;
    Ok(ledger.entries.values().any(|entry| {
        entry.profile_id == profile_id && (entry.pending || entry.fingerprint != "missing")
    }))
}

pub(crate) async fn preview(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    operation: AdapterOperation,
) -> Result<AdapterPreview> {
    let status = inspect(ctx, target).await?;
    let can_apply = status.available
        && !status.modified
        && match operation {
            AdapterOperation::Apply => !status.present || status.owned,
            AdapterOperation::Remove => status.owned,
            AdapterOperation::Restore => status.owned && status.backup_ref.is_some(),
        };
    let mut warnings = client_warnings(target.client);
    if status.present && !status.owned {
        warnings.push(
            "客户端已有同名连接，但不是由 Hub 添加的。请先确认它的用途，Hub 不会覆盖它。".into(),
        );
    }
    if status.modified {
        warnings.push(
            "这条连接已被手动或其他程序修改。请先核对客户端中的设置，再回 Hub 重新检查。".into(),
        );
    }
    Ok(AdapterPreview {
        operation,
        status,
        can_apply,
        warnings,
    })
}

pub(crate) async fn apply(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    expected: &str,
    endpoint: &str,
    bearer: &str,
) -> Result<AdapterOutcome> {
    let url = Url::parse(endpoint).map_err(|_| anyhow::anyhow!("NVWA 本地 MCP 地址无效"))?;
    ensure!(
        url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "NVWA MCP 仅允许本机回环桥地址"
    );
    ensure!(
        url.path() == format!("/mcp/{}/{}", target.profile_id, target.client.as_str()),
        "NVWA MCP 桥地址与环境或客户端不匹配"
    );
    ensure!(
        bearer.len() >= 32 && bearer.len() <= 1024 && bearer.bytes().all(|b| b.is_ascii_graphic()),
        "NVWA 本地连接凭据无效"
    );
    let desired = match target.client {
        ClientKind::Codex => codex::desired(&target.server_name, endpoint, bearer),
        ClientKind::Workbuddy => workbuddy::desired(endpoint, bearer),
        ClientKind::Tiangong => tiangong::desired(&target.server_name, endpoint, bearer),
    };
    mutate(
        ctx,
        target,
        expected,
        AdapterOperation::Apply,
        Some(desired),
        None,
    )
    .await
}

pub(crate) async fn remove(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    expected: &str,
) -> Result<AdapterOutcome> {
    mutate(ctx, target, expected, AdapterOperation::Remove, None, None).await
}

pub(crate) async fn restore(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    expected: &str,
    backup_ref: &str,
) -> Result<AdapterOutcome> {
    mutate(
        ctx,
        target,
        expected,
        AdapterOperation::Restore,
        None,
        Some(backup_ref),
    )
    .await
}

async fn mutate(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    expected: &str,
    operation: AdapterOperation,
    mut desired: Option<Snapshot>,
    restore_ref: Option<&str>,
) -> Result<AdapterOutcome> {
    ensure!(!expected.is_empty(), "接入操作缺少目标指纹，请先预览");
    let location = location(target)?;
    let root = metadata_root(ctx)?;
    let _lock = ledger_lock(&root).await?;
    let mut ledger = load_ledger(&root)?;
    let before = read_target(ctx, target, &location).await?;
    reconcile_journal(ctx, target, &location, &root, &before, &mut ledger)?;
    let current = status(
        target,
        &location,
        &before,
        ledger.entries.get(&location.key),
    )?;
    ensure!(
        current.target_fingerprint == expected,
        "MCP 目标在预览后已变化，本次未覆盖"
    );
    ensure!(!current.modified, "Hub 受管 MCP 条目已被修改，本次未覆盖");
    ensure!(
        !current.present || current.owned,
        "同名 MCP 条目不是 Hub 管理，本次未接管"
    );
    let previous_managed = ledger.entries.get(&location.key).cloned();
    let next_managed;
    match operation {
        AdapterOperation::Restore => {
            let reference = restore_ref.ok_or_else(|| anyhow::anyhow!("还原缺少受保护备份引用"))?;
            ensure!(
                previous_managed
                    .as_ref()
                    .is_some_and(|entry| entry.backup_ref == reference),
                "备份不属于当前受管目标"
            );
            let bytes = load_backup(&ctx.secrets, reference)?;
            let backup: ProtectedBackup = serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("受保护 MCP 备份格式无效"))?;
            ensure!(
                backup.version == 1
                    && backup.target_key == location.key
                    && backup.after_fingerprint
                        == fingerprint(before.snapshot.as_ref(), true, target.client)?,
                "MCP 目标已变化，不能将备份还原到当前条目"
            );
            desired = backup.before;
            next_managed = backup.previous_managed;
        }
        AdapterOperation::Apply | AdapterOperation::Remove => {
            if matches!(operation, AdapterOperation::Remove) {
                ensure!(current.owned, "没有可移除的 Hub 受管 MCP 条目");
            }
            let after = planned_snapshot(target, &before, desired.as_ref())?;
            desired = after;
            let after_fingerprint = fingerprint(desired.as_ref(), true, target.client)?;
            let reference = format!("adapter-backup:{}", Uuid::new_v4());
            let backup = ProtectedBackup {
                version: 1,
                target_key: location.key.clone(),
                before: before.snapshot.clone(),
                after_fingerprint,
                previous_managed: previous_managed.clone(),
            };
            let bytes = serde_json::to_vec(&backup)
                .map_err(|_| anyhow::anyhow!("无法编码 MCP 受保护备份"))?;
            save_backup(&ctx.secrets, &reference, &bytes)?;
            next_managed = Some(ManagedEntry {
                profile_id: target.profile_id.clone(),
                client: target.client,
                server_name: target.server_name.clone(),
                target_path: location.path.clone(),
                fingerprint: fingerprint(desired.as_ref(), true, target.client)?,
                backup_ref: reference.clone(),
                pending: false,
            });
        }
    }
    // Journal ownership before external mutation. A crash leaves a detectable
    // mismatch and protected target-only recovery evidence, never silent takeover.
    let desired_ownership = fingerprint(desired.as_ref(), true, target.client)?;
    let mut journal = next_managed.clone().or_else(|| {
        previous_managed.clone().map(|mut entry| {
            entry.fingerprint = desired_ownership.clone();
            entry
        })
    });
    if let Some(entry) = &mut journal {
        entry.pending = true;
    }
    set_record(&mut ledger, &location.key, journal);
    save_ledger(&root, &ledger)?;
    let mutation = replace_target(ctx, target, &location, &before, desired.as_ref()).await;
    let observed = read_target(ctx, target, &location).await;
    let after_fingerprint = fingerprint(desired.as_ref(), false, target.client)?;
    match observed {
        Ok(after)
            if fingerprint(after.snapshot.as_ref(), false, target.client)? == after_fingerprint =>
        {
            set_record(&mut ledger, &location.key, next_managed);
            if save_ledger(&root, &ledger).is_err() {
                // Compensate only while the exact written target is still ours.
                // Re-read the whole file so unrelated edits remain intact.
                if let Ok(latest) = read_target(ctx, target, &location).await {
                    if fingerprint(latest.snapshot.as_ref(), false, target.client)?
                        == after_fingerprint
                    {
                        let _ = replace_target(
                            ctx,
                            target,
                            &location,
                            &latest,
                            before.snapshot.as_ref(),
                        )
                        .await;
                        if let Ok(restored) = read_target(ctx, target, &location).await {
                            if fingerprint(restored.snapshot.as_ref(), false, target.client)?
                                == expected
                            {
                                set_record(&mut ledger, &location.key, previous_managed);
                                let _ = save_ledger(&root, &ledger);
                                bail!("接入元数据保存失败；已核对目标未变并补偿还原，请刷新后重试")
                            }
                        }
                    }
                }
                bail!("接入元数据提交失败，目标结果待核对；没有覆盖并发修改，受保护恢复记录仍保留")
            }
            let mut warnings = client_warnings(target.client);
            if mutation.is_err() {
                warnings.push("操作时未收到明确回复，随后已检查并确认本次修改已完成。".into());
            }
            let updated_status =
                status(target, &location, &after, ledger.entries.get(&location.key))?;
            Ok(AdapterOutcome {
                backup_ref: updated_status.backup_ref.clone(),
                status: updated_status,
                mutation_done: true,
                warnings,
            })
        }
        Ok(after) => {
            if fingerprint(after.snapshot.as_ref(), false, target.client)? == expected {
                set_record(&mut ledger, &location.key, previous_managed);
                save_ledger(&root, &ledger)?;
            }
            bail!("MCP 保存结果未确认或目标被并发修改；没有重放或覆盖，请重新读取目标状态")
        }
        Err(_) => bail!("MCP 写入后无法核对目标；已保留受保护恢复记录，请重新读取状态后处理"),
    }
}

fn set_record(ledger: &mut Ledger, key: &str, record: Option<ManagedEntry>) {
    match record {
        Some(record) => {
            ledger.entries.insert(key.into(), record);
        }
        None => {
            ledger.entries.remove(key);
        }
    }
}

fn reconcile_journal(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    location: &Location,
    root: &Path,
    read: &TargetRead,
    ledger: &mut Ledger,
) -> Result<()> {
    let Some(mut entry) = ledger
        .entries
        .get(&location.key)
        .filter(|entry| entry.pending)
        .cloned()
    else {
        return Ok(());
    };
    let actual = fingerprint(read.snapshot.as_ref(), true, target.client)?;
    if actual == entry.fingerprint {
        // Read-only evidence confirms the pending external mutation completed.
        entry.pending = false;
        set_record(ledger, &location.key, Some(entry));
        return save_ledger(root, ledger);
    }
    if let Ok(bytes) = load_backup(&ctx.secrets, &entry.backup_ref) {
        if let Ok(backup) = serde_json::from_slice::<ProtectedBackup>(&bytes) {
            if backup.version == 1
                && backup.target_key == location.key
                && fingerprint(backup.before.as_ref(), true, target.client)? == actual
            {
                // The exact before-image remains: undo only our journal metadata.
                set_record(ledger, &location.key, backup.previous_managed);
                return save_ledger(root, ledger);
            }
        }
    }
    Ok(())
}

fn save_backup(secrets: &SecretStore, reference: &str, bytes: &[u8]) -> Result<()> {
    const CHUNK_BYTES: usize = 32 * 1024;
    ensure!(
        bytes.len() <= MAX_FILE_BYTES as usize,
        "目标 MCP 受保护备份超过大小限制"
    );
    let mut written: Vec<String> = Vec::new();
    for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
        let key = format!("{reference}:part:{index}");
        if let Err(error) = secrets.set(&key, chunk) {
            for stored in written {
                let _ = secrets.delete(&stored);
            }
            return Err(error);
        }
        written.push(key);
    }
    let manifest = BackupManifest {
        version: 1,
        chunked: true,
        parts: written.len(),
        length: bytes.len(),
        sha256: hex::encode(Sha256::digest(bytes)),
    };
    let result = secrets.set(reference, &serde_json::to_vec(&manifest)?);
    if result.is_err() {
        for stored in written {
            let _ = secrets.delete(&stored);
        }
    }
    result
}

fn load_backup(secrets: &SecretStore, reference: &str) -> Result<Vec<u8>> {
    let bytes = secrets
        .get(reference)?
        .ok_or_else(|| anyhow::anyhow!("受保护 MCP 备份不存在"))?;
    let manifest: BackupManifest =
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("受保护 MCP 备份索引无效"))?;
    ensure!(
        manifest.version == 1
            && manifest.chunked
            && manifest.length <= MAX_FILE_BYTES as usize
            && manifest.parts > 0
            && manifest.parts <= (MAX_FILE_BYTES as usize).div_ceil(32 * 1024),
        "受保护 MCP 备份索引超出范围"
    );
    let mut assembled = Vec::with_capacity(manifest.length);
    for index in 0..manifest.parts {
        let chunk = secrets
            .get(&format!("{reference}:part:{index}"))?
            .ok_or_else(|| anyhow::anyhow!("受保护 MCP 备份不完整"))?;
        ensure!(
            chunk.len() <= 32 * 1024
                && chunk.len() <= manifest.length.saturating_sub(assembled.len()),
            "受保护 MCP 备份块无效"
        );
        assembled.extend_from_slice(&chunk);
    }
    ensure!(
        assembled.len() == manifest.length
            && hex::encode(Sha256::digest(&assembled)) == manifest.sha256,
        "受保护 MCP 备份校验失败"
    );
    Ok(assembled)
}

fn planned_snapshot(
    target: &AdapterTarget,
    before: &TargetRead,
    desired: Option<&Snapshot>,
) -> Result<Option<Snapshot>> {
    match target.client {
        ClientKind::Codex | ClientKind::Workbuddy => {
            let bytes = replace_file(
                target,
                before.file_bytes.as_deref().unwrap_or_default(),
                desired,
            )?;
            read_file(target, &bytes)
        }
        ClientKind::Tiangong => Ok(desired.cloned()),
    }
}

async fn read_target(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    location: &Location,
) -> Result<TargetRead> {
    if let Some(path) = &location.path {
        let path = path.clone();
        let target = target.clone();
        return tokio::task::spawn_blocking(move || {
            validate_path(&path)?;
            let raw = read_bounded(&path, MAX_FILE_BYTES)?;
            let snapshot = read_file(&target, raw.as_deref().unwrap_or_default())?;
            Ok(TargetRead {
                snapshot,
                file_bytes: raw,
            })
        })
        .await
        .map_err(|_| anyhow::anyhow!("MCP 配置读取中断"))?;
    }
    let desktop = ctx
        .desktop
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("天工未取得已验证的运行授权，请先显式连接桌面"))?;
    Ok(TargetRead {
        snapshot: tiangong::read(desktop, &target.server_name).await?,
        file_bytes: None,
    })
}

async fn replace_target(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    location: &Location,
    before: &TargetRead,
    desired: Option<&Snapshot>,
) -> Result<()> {
    if let Some(path) = &location.path {
        let path = path.clone();
        let target = target.clone();
        let raw = before.file_bytes.clone();
        let desired = desired.cloned();
        return tokio::task::spawn_blocking(move || {
            validate_path(&path)?;
            let parent = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("MCP 配置路径无效"))?;
            fs::create_dir_all(parent).map_err(|_| anyhow::anyhow!("无法创建 MCP 配置目录"))?;
            validate_path(&path)?;
            let lock_path = path.with_extension(format!(
                "{}.nvwa.lock",
                path.extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("config")
            ));
            validate_path(&lock_path)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(lock_path)
                .map_err(|_| anyhow::anyhow!("无法打开 MCP 配置操作锁"))?;
            lock.lock_exclusive()
                .map_err(|_| anyhow::anyhow!("无法锁定 MCP 配置"))?;
            let current = read_bounded(&path, MAX_FILE_BYTES)?;
            ensure!(current == raw, "MCP 配置在保存前已变化，本次未覆盖");
            let bytes = replace_file(
                &target,
                current.as_deref().unwrap_or_default(),
                desired.as_ref(),
            )?;
            ensure!(
                read_bounded(&path, MAX_FILE_BYTES)? == current,
                "MCP 配置被其他操作修改，本次未覆盖"
            );
            atomic_write(&path, &bytes)
        })
        .await
        .map_err(|_| anyhow::anyhow!("MCP 配置写入中断"))?;
    }
    let desktop = ctx
        .desktop
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("天工运行授权不可用"))?;
    let current = tiangong::read(desktop, &target.server_name).await?;
    ensure!(
        fingerprint(current.as_ref(), false, target.client)?
            == fingerprint(before.snapshot.as_ref(), false, target.client)?,
        "天工 MCP 目标在保存前已变化，本次未覆盖"
    );
    tiangong::replace(desktop, &target.server_name, desired, current.is_some()).await
}

fn read_file(target: &AdapterTarget, raw: &[u8]) -> Result<Option<Snapshot>> {
    match target.client {
        ClientKind::Codex => codex::read(raw, &target.server_name),
        ClientKind::Workbuddy => workbuddy::read(raw, &target.server_name),
        ClientKind::Tiangong => bail!("天工 MCP 不使用配置文件"),
    }
}

fn replace_file(target: &AdapterTarget, raw: &[u8], desired: Option<&Snapshot>) -> Result<Vec<u8>> {
    match target.client {
        ClientKind::Codex => codex::replace(raw, &target.server_name, desired),
        ClientKind::Workbuddy => workbuddy::replace(raw, &target.server_name, desired),
        ClientKind::Tiangong => bail!("天工 MCP 不使用配置文件"),
    }
}

/// Reuse the exact inspection normalization when checking a fresh native entry
/// before writing TianGong connection discovery fields.
pub(crate) fn tiangong_target_fingerprint(entry: &Value) -> Result<String> {
    let snapshot = tiangong::snapshot(entry)?;
    fingerprint(Some(&snapshot), false, ClientKind::Tiangong)
}

fn fingerprint(snapshot: Option<&Snapshot>, ownership: bool, client: ClientKind) -> Result<String> {
    let Some(snapshot) = snapshot else {
        return Ok("missing".into());
    };
    let snapshot = if ownership && client == ClientKind::Tiangong {
        tiangong::static_snapshot(snapshot)
    } else {
        snapshot.clone()
    };
    let bytes = match snapshot {
        Snapshot::Toml(text) => text.into_bytes(),
        Snapshot::Json(value) => serde_json::to_vec(&value)?,
    };
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn status(
    target: &AdapterTarget,
    location: &Location,
    read: &TargetRead,
    managed: Option<&ManagedEntry>,
) -> Result<AdapterStatus> {
    let owned = managed.is_some();
    let modified = managed.is_some_and(|entry| {
        fingerprint(read.snapshot.as_ref(), true, target.client)
            .ok()
            .as_deref()
            != Some(entry.fingerprint.as_str())
    });
    let present = read.snapshot.is_some();
    let enabled = read
        .snapshot
        .as_ref()
        .and_then(|snapshot| match target.client {
            ClientKind::Codex => codex::enabled(snapshot, &target.server_name),
            ClientKind::Workbuddy => workbuddy::enabled(snapshot),
            ClientKind::Tiangong => tiangong::enabled(snapshot),
        });
    let native_connected = if target.client == ClientKind::Tiangong {
        read.snapshot.as_ref().and_then(tiangong::connected)
    } else {
        None
    };
    let configured = present && owned && !modified && !managed.is_some_and(|entry| entry.pending);
    let detail = if managed.is_some_and(|entry| entry.pending) {
        "上次修改的结果还未确认。请查看客户端中的这条连接，再回 Hub 重新检查；修改前的备份已保留。"
    } else if modified {
        "这条连接与 Hub 上次保存的设置不同。请先核对客户端中的设置，Hub 会保留这些改动。"
    } else if present && !owned {
        "客户端已有同名连接，但不是由 Hub 添加的。请先确认它的用途，Hub 不会覆盖它。"
    } else if configured {
        "连接设置已保存。请刷新客户端，并按客户端提示确认使用这条连接。"
    } else {
        "尚未给此客户端添加 NVWA 连接。"
    };
    Ok(AdapterStatus {
        client: target.client,
        server_name: target.server_name.clone(),
        target_path: location
            .path
            .as_ref()
            .map(|path| path.display().to_string()),
        available: true,
        present,
        configured,
        owned,
        modified,
        enabled,
        target_fingerprint: fingerprint(read.snapshot.as_ref(), false, target.client)?,
        backup_ref: managed.map(|entry| entry.backup_ref.clone()),
        load_state: if configured {
            if target.client == ClientKind::Tiangong {
                "next_turn"
            } else {
                "pending_client_refresh"
            }
        } else {
            "not_configured"
        }
        .into(),
        connection_state: "unknown".into(),
        native_connected,
        detail: detail.into(),
    })
}

fn unavailable(target: &AdapterTarget, managed: Option<&ManagedEntry>) -> AdapterStatus {
    AdapterStatus {
        client: target.client,
        server_name: target.server_name.clone(),
        target_path: None,
        available: false,
        present: false,
        configured: false,
        owned: managed.is_some(),
        modified: false,
        enabled: None,
        target_fingerprint: String::new(),
        backup_ref: managed.map(|entry| entry.backup_ref.clone()),
        load_state: "unavailable".into(),
        connection_state: "unknown".into(),
        native_connected: None,
        detail: "请先打开天工 Claw，并在 Hub 中完成天工连接，再检查 NVWA 接入。".into(),
    }
}

fn client_warnings(client: ClientKind) -> Vec<String> {
    match client {
        ClientKind::Codex => {
            vec!["保存后请重新打开或刷新 Codex 会话，并按 Codex 的提示确认使用这条连接。".into()]
        }
        ClientKind::Workbuddy => vec![
            "保存后请刷新或重新打开 WorkBuddy，并按 WorkBuddy 的提示确认使用这条连接。".into(),
            "使用品牌版或专享版 WorkBuddy 时，请确认这里的配置文件路径属于你正在使用的版本。"
                .into(),
        ],
        ClientKind::Tiangong => vec![
            "保存后请在 Hub 检测这条天工连接。检测成功并更新能力列表后，再到天工开始下一轮对话。"
                .into(),
            "操作期间请勿同时在天工中修改同一条连接。设置发生变化时，请重新检查后再操作。".into(),
        ],
    }
}

fn validate_path(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                ensure!(
                    !metadata.file_type().is_symlink(),
                    "MCP 配置或元数据路径包含符号链接，请选择真实路径"
                );
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        metadata.file_attributes() & 0x400 == 0,
                        "MCP 配置或元数据路径包含重解析点，请选择真实路径"
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => bail!("无法核对 MCP 配置路径"),
        }
    }
    Ok(())
}

fn read_bounded(path: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>> {
    validate_path(path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => bail!("无法读取 MCP 配置或接入元数据"),
    };
    ensure!(
        file.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= max_bytes),
        "MCP 配置文件类型或大小无效"
    );
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("无法读取 MCP 配置"))?;
    ensure!(bytes.len() as u64 <= max_bytes, "MCP 配置超过读取限制");
    Ok(Some(bytes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    validate_path(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("MCP 保存路径无效"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| anyhow::anyhow!("无法准备 MCP 临时文件"))?;
    if let Ok(metadata) = fs::metadata(path) {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())
            .map_err(|_| anyhow::anyhow!("无法保留 MCP 文件权限"))?;
    }
    temporary
        .write_all(bytes)
        .map_err(|_| anyhow::anyhow!("无法写入 MCP 临时文件"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| anyhow::anyhow!("无法同步 MCP 临时文件"))?;
    validate_path(path)?;
    temporary
        .persist(path)
        .map_err(|_| anyhow::anyhow!("无法原子替换 MCP 配置"))?;
    Ok(())
}
