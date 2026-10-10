//! Codex name changes are one file transaction with target-only recovery data.
use std::{collections::BTreeSet, path::Component};

use super::*;

const MAX_SLOTS: usize = 4;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexSlot {
    server_name: String,
    before: Option<Snapshot>,
    after_fingerprint: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexBackup {
    version: u32,
    scope: String,
    slots: Vec<CodexSlot>,
    previous_records: BTreeMap<String, ManagedEntry>,
    next_records: BTreeMap<String, ManagedEntry>,
}

struct FileState {
    raw: Option<Vec<u8>>,
    slots: BTreeMap<String, Option<Snapshot>>,
}

struct Plan {
    actual: AdapterTarget,
    actual_location: Location,
    state: FileState,
    record: Option<ManagedEntry>,
    public: AdapterStatus,
}

fn scope(target: &AdapterTarget, location: &Location) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
        &target.profile_id,
        target.client,
        path_identity(location.path.as_deref())?,
    ))?)))
}

fn legacy_scope(target: &AdapterTarget, location: &Location) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
        &target.profile_id,
        target.client,
        &location.path,
    ))?)))
}

// Normalize only file identity. Managed keys and v1 backups keep the exact
// original path spelling, so an alias never reinterprets an existing key.
fn path_identity(path: Option<&Path>) -> Result<String> {
    let path = path.ok_or_else(|| anyhow::anyhow!("Codex 配置路径缺失"))?;
    ensure!(path.is_absolute(), "Codex 配置路径必须是绝对路径");
    validate_path(path)?;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    validate_path(&normalized)?;
    let mut ancestor = normalized.as_path();
    let mut suffix = Vec::new();
    let mut resolved = loop {
        match fs::canonicalize(ancestor) {
            Ok(resolved) => break resolved,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                suffix.push(
                    ancestor
                        .file_name()
                        .ok_or_else(|| anyhow::anyhow!("无法核对 Codex 配置路径身份"))?
                        .to_os_string(),
                );
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("无法核对 Codex 配置路径身份"))?;
            }
            Err(_) => bail!("无法核对 Codex 配置路径身份"),
        }
    };
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    let identity = resolved
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Codex 配置路径编码无效"))?;
    #[cfg(windows)]
    {
        let identity = identity.replace('/', "\\").to_lowercase();
        if let Some(unc) = identity.strip_prefix("\\\\?\\unc\\") {
            return Ok(format!("\\\\{unc}"));
        }
        if let Some(drive) = identity.strip_prefix("\\\\?\\")
            && drive.as_bytes().get(1) == Some(&b':')
            && drive.as_bytes().get(2) == Some(&b'\\')
        {
            return Ok(drive.into());
        }
        Ok(identity)
    }
    #[cfg(not(windows))]
    {
        Ok(identity.into())
    }
}

fn same_path(left: Option<&Path>, right: Option<&Path>) -> Result<bool> {
    Ok(path_identity(left)? == path_identity(right)?)
}

fn named(target: &AdapterTarget, name: &str) -> AdapterTarget {
    let mut target = target.clone();
    target.server_name = name.into();
    target
}

fn record_target(target: &AdapterTarget, record: &ManagedEntry) -> AdapterTarget {
    let mut actual = named(target, &record.server_name);
    actual.override_path = record.target_path.clone();
    actual
}

fn scoped_records(
    target: &AdapterTarget,
    location: &Location,
    ledger: &Ledger,
) -> Result<BTreeMap<String, ManagedEntry>> {
    let mut result = BTreeMap::new();
    for (key, record) in &ledger.entries {
        if record.profile_id == target.profile_id
            && record.client == ClientKind::Codex
            && same_path(record.target_path.as_deref(), location.path.as_deref())?
        {
            validate_codex_server_name(&record.server_name)?;
            ensure!(
                super::location(&record_target(target, record))?.key == *key,
                "Codex 受管名称元数据不一致，请先核对接入记录"
            );
            result.insert(key.clone(), record.clone());
        }
    }
    Ok(result)
}

async fn read_state(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    location: &Location,
    names: &BTreeSet<String>,
) -> Result<FileState> {
    let read = read_target(ctx, target, location).await?;
    let mut slots = BTreeMap::new();
    for name in names {
        slots.insert(
            name.clone(),
            codex::read(read.file_bytes.as_deref().unwrap_or_default(), name)?,
        );
    }
    Ok(FileState {
        raw: read.file_bytes,
        slots,
    })
}

fn slot_fingerprint(state: &FileState, name: &str) -> Result<String> {
    fingerprint(
        state.slots.get(name).and_then(Option::as_ref),
        false,
        ClientKind::Codex,
    )
}

fn validate_backup(
    backup: &CodexBackup,
    target: &AdapterTarget,
    location: &Location,
) -> Result<()> {
    ensure!(
        backup.version == 2 && !backup.slots.is_empty() && backup.slots.len() <= MAX_SLOTS,
        "Codex 名称迁移备份不属于当前环境或保存位置"
    );
    let mut names = BTreeSet::new();
    for slot in &backup.slots {
        validate_codex_server_name(&slot.server_name)?;
        ensure!(
            names.insert(slot.server_name.clone()),
            "Codex 名称迁移备份包含重复目标"
        );
    }
    for (records, after) in [
        (&backup.previous_records, false),
        (&backup.next_records, true),
    ] {
        for (key, record) in records {
            ensure!(
                !record.pending
                    && record.profile_id == target.profile_id
                    && record.client == ClientKind::Codex
                    && same_path(record.target_path.as_deref(), location.path.as_deref())?
                    && names.contains(&record.server_name)
                    && super::location(&record_target(target, record))?.key == *key,
                "Codex 名称迁移备份归属无效"
            );
            let slot = backup
                .slots
                .iter()
                .find(|slot| slot.server_name == record.server_name)
                .ok_or_else(|| anyhow::anyhow!("Codex 名称迁移备份目标缺失"))?;
            let expected = if after {
                slot.after_fingerprint.clone()
            } else {
                fingerprint(slot.before.as_ref(), false, ClientKind::Codex)?
            };
            // Missing entries are historical recovery markers. Normalizing old
            // aliases can place one beside the active owner of the same slot;
            // Restore preserves that metadata without claiming the live entry.
            ensure!(
                record.fingerprint == expected || record.fingerprint == "missing",
                "Codex 名称迁移备份指纹与归属记录不一致"
            );
        }
    }
    let mut valid_scope =
        backup.scope == scope(target, location)? || backup.scope == legacy_scope(target, location)?;
    // Accept earlier v2 scopes only through an independently validated record's
    // original path. v1 keys are never recomputed from the request's alias.
    for record in backup
        .previous_records
        .values()
        .chain(backup.next_records.values())
    {
        valid_scope |= backup.scope
            == legacy_scope(target, &super::location(&record_target(target, record))?)?;
    }
    ensure!(valid_scope, "Codex 名称迁移备份不属于当前环境或保存位置");
    Ok(())
}

fn backup_names(backup: &CodexBackup) -> BTreeSet<String> {
    backup
        .slots
        .iter()
        .map(|slot| slot.server_name.clone())
        .collect()
}

fn matches_after(backup: &CodexBackup, state: &FileState) -> Result<bool> {
    for slot in &backup.slots {
        if slot_fingerprint(state, &slot.server_name)? != slot.after_fingerprint {
            return Ok(false);
        }
    }
    Ok(true)
}

fn matches_before(backup: &CodexBackup, state: &FileState) -> Result<bool> {
    for slot in &backup.slots {
        if slot_fingerprint(state, &slot.server_name)?
            != fingerprint(slot.before.as_ref(), false, ClientKind::Codex)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn install_records(
    ledger: &mut Ledger,
    target: &AdapterTarget,
    location: &Location,
    backup: &CodexBackup,
    records: &BTreeMap<String, ManagedEntry>,
) -> Result<()> {
    let names = backup_names(backup);
    for (key, record) in scoped_records(target, location, ledger)? {
        if names.contains(&record.server_name) {
            ledger.entries.remove(&key);
        }
    }
    ledger.entries.extend(records.clone());
    Ok(())
}

async fn reconcile(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    location: &Location,
    root: &Path,
    ledger: &mut Ledger,
) -> Result<()> {
    let pending = scoped_records(target, location, ledger)?;
    let mut handled = BTreeSet::new();
    for (key, record) in pending.into_iter().filter(|(_, record)| record.pending) {
        if !handled.insert(record.backup_ref.clone()) {
            continue;
        }
        let bytes = match load_backup(&ctx.secrets, &record.backup_ref) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if let Ok(backup) = serde_json::from_slice::<CodexBackup>(&bytes) {
            validate_backup(&backup, target, location)?;
            let scoped = scoped_records(target, location, ledger)?;
            ensure!(
                backup
                    .slots
                    .iter()
                    .any(|slot| slot.server_name == record.server_name),
                "Codex 名称迁移日志与保护备份不一致"
            );
            for slot in &backup.slots {
                let entries: Vec<_> = scoped
                    .values()
                    .filter(|entry| entry.server_name == slot.server_name)
                    .collect();
                ensure!(
                    entries.len() == 1
                        && entries.iter().all(|entry| {
                            entry.pending
                                && entry.backup_ref == record.backup_ref
                                && entry.fingerprint == slot.after_fingerprint
                                && entry.profile_id == target.profile_id
                                && entry.client == ClientKind::Codex
                                && entry.server_name == slot.server_name
                        }),
                    "Codex 名称迁移日志尚不完整，请核对接入记录"
                );
            }
            let state = read_state(ctx, target, location, &backup_names(&backup)).await?;
            let records = if matches_after(&backup, &state)? {
                Some(&backup.next_records)
            } else if matches_before(&backup, &state)? {
                Some(&backup.previous_records)
            } else {
                None
            };
            if let Some(records) = records {
                install_records(ledger, target, location, &backup, records)?;
                save_ledger(root, ledger)?;
            }
        } else {
            // Existing v1 journals and protected single-target backups retain
            // their original name, key and fingerprint normalization.
            let actual = record_target(target, &record);
            let actual_location = super::location(&actual)?;
            ensure!(actual_location.key == key, "Codex 受管归属键无效");
            let read = read_target(ctx, &actual, &actual_location).await?;
            reconcile_journal(ctx, &actual, &actual_location, root, &read, ledger)?;
        }
    }
    Ok(())
}

async fn plan(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    location: &Location,
    ledger: &Ledger,
) -> Result<Plan> {
    let records = scoped_records(target, location, ledger)?;
    let pending = records.values().any(|record| record.pending);
    let active: Vec<_> = records
        .values()
        .filter(|record| record.fingerprint != "missing")
        .collect();
    ensure!(
        active.len() <= 1,
        "同一环境在此 Codex 配置中有多个受管连接，请先核对旧连接，Hub 未自动删除"
    );
    let preferred: Vec<_> = records
        .values()
        .filter(|record| record.server_name == target.server_name)
        .collect();
    ensure!(
        !active.is_empty() || preferred.len() <= 1,
        "此 Codex 名称有多个路径别名恢复记录，请先核对接入记录"
    );
    let record = active
        .first()
        .copied()
        .or(preferred.first().copied())
        .or_else(|| {
            (records.len() == 1)
                .then(|| records.values().next())
                .flatten()
        })
        .cloned();
    ensure!(
        record.is_some() || records.is_empty() || pending,
        "此环境有多个可恢复的旧 Codex 名称，请先将环境名改为要管理的名称"
    );
    let valid_desired = validate_codex_server_name(&target.server_name).is_ok();
    let actual_name = record
        .as_ref()
        .map(|record| record.server_name.clone())
        .unwrap_or(if valid_desired {
            target.server_name.clone()
        } else {
            managed_server_name(&target.profile_id)?
        });
    let actual = record
        .as_ref()
        .map(|record| record_target(target, record))
        .unwrap_or_else(|| named(target, &actual_name));
    let actual_location = super::location(&actual)?;
    let mut names = BTreeSet::from([actual_name.clone()]);
    if valid_desired {
        names.insert(target.server_name.clone());
    }
    // Include the latest migration's old name in preview CAS, so Restore
    // cannot remove/recreate a target changed since the confirmation preview.
    if let Some(record) = &record
        && let Ok(bytes) = load_backup(&ctx.secrets, &record.backup_ref)
        && let Ok(backup) = serde_json::from_slice::<CodexBackup>(&bytes)
    {
        validate_backup(&backup, target, location)?;
        names.extend(backup_names(&backup));
    }
    ensure!(names.len() <= MAX_SLOTS, "Codex 名称迁移目标超出限制");
    let state = read_state(ctx, &actual, &actual_location, &names).await?;
    let read = TargetRead {
        snapshot: state.slots.get(&actual_name).cloned().flatten(),
        file_bytes: None,
    };
    let mut public = status(&actual, &actual_location, &read, record.as_ref())?;
    public.desired_server_name = Some(target.server_name.clone());
    public.previous_server_name =
        (actual_name != target.server_name && record.is_some()).then_some(actual_name.clone());
    if actual_name != target.server_name
        && valid_desired
        && state
            .slots
            .get(&target.server_name)
            .is_some_and(Option::is_some)
    {
        public.name_conflict = true;
        public.detail =
            "Codex 已有与环境名称相同的其他连接，Hub 不会覆盖它；请更换环境名称。".into();
    }
    if pending {
        public.modified = true;
        public.detail =
            "上次 Codex 名称迁移的结果还未确认，请核对新旧连接后重新检查；保护备份已保留。".into();
    }
    let fingerprints: BTreeMap<_, _> = names
        .iter()
        .map(|name| Ok((name.clone(), slot_fingerprint(&state, name)?)))
        .collect::<Result<_>>()?;
    public.target_fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&(
        scope(target, location)?,
        &target.server_name,
        &actual_name,
        fingerprints,
        &records,
    ))?));
    Ok(Plan {
        actual,
        actual_location,
        state,
        record,
        public,
    })
}

pub(super) async fn inspect(ctx: &AdapterContext, target: &AdapterTarget) -> Result<AdapterStatus> {
    let location = super::location(target)?;
    let root = metadata_root(ctx)?;
    let _lock = ledger_lock(&root).await?;
    let mut ledger = load_ledger(&root)?;
    reconcile(ctx, target, &location, &root, &mut ledger).await?;
    Ok(plan(ctx, target, &location, &ledger).await?.public)
}

async fn write_slots(
    location: &Location,
    before: &FileState,
    desired: &BTreeMap<String, Option<Snapshot>>,
) -> Result<()> {
    let path = location
        .path
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Codex 配置路径缺失"))?;
    let raw = before.raw.clone();
    let desired = desired.clone();
    tokio::task::spawn_blocking(move || {
        validate_path(&path)?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Codex 配置路径无效"))?;
        fs::create_dir_all(parent).map_err(|_| anyhow::anyhow!("无法创建 Codex 配置目录"))?;
        validate_path(&path)?;
        let lock_path = path.with_extension("toml.nvwa.lock");
        validate_path(&lock_path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|_| anyhow::anyhow!("无法打开 Codex 配置操作锁"))?;
        lock.lock_exclusive()
            .map_err(|_| anyhow::anyhow!("无法锁定 Codex 配置"))?;
        let current = read_bounded(&path, MAX_FILE_BYTES)?;
        ensure!(current == raw, "Codex 配置在保存前已变化，本次未覆盖");
        let bytes = codex::replace_many(current.as_deref().unwrap_or_default(), &desired)?;
        ensure!(
            bytes.len() <= MAX_FILE_BYTES as usize,
            "Codex MCP 配置超出大小限制"
        );
        ensure!(
            read_bounded(&path, MAX_FILE_BYTES)? == current,
            "Codex 配置被其他操作修改，本次未覆盖"
        );
        atomic_write(&path, &bytes)
    })
    .await
    .map_err(|_| anyhow::anyhow!("Codex 配置写入中断"))?
}

pub(super) async fn mutate(
    ctx: &AdapterContext,
    target: &AdapterTarget,
    expected: &str,
    operation: AdapterOperation,
    desired: Option<Snapshot>,
    restore_ref: Option<&str>,
) -> Result<AdapterOutcome> {
    ensure!(!expected.is_empty(), "接入操作缺少目标指纹，请先预览");
    if matches!(operation, AdapterOperation::Apply) {
        validate_codex_server_name(&target.server_name)?;
    }
    let location = super::location(target)?;
    let root = metadata_root(ctx)?;
    let _lock = ledger_lock(&root).await?;
    let mut ledger = load_ledger(&root)?;
    reconcile(ctx, target, &location, &root, &mut ledger).await?;
    let current = plan(ctx, target, &location, &ledger).await?;
    ensure!(
        current.public.target_fingerprint == expected,
        "MCP 目标在预览后已变化，本次未覆盖"
    );
    ensure!(
        !current.public.modified,
        "Codex 新旧连接存在冲突或外部修改，本次未覆盖"
    );
    ensure!(
        !current.public.present || current.public.owned,
        "同名 MCP 条目不是 Hub 管理，本次未接管"
    );
    // Keep the selected owner's original path for this complete transaction.
    // Request aliases affect discovery, never the interpretation of old keys.
    let location = Location {
        path: current.actual_location.path.clone(),
        key: current.actual_location.key.clone(),
    };
    let reference = format!("adapter-backup:{}", Uuid::new_v4());
    let mut desired_slots = BTreeMap::new();
    let mut next_records = BTreeMap::new();
    match operation {
        AdapterOperation::Apply => {
            ensure!(
                !current.public.name_conflict,
                "Codex 环境名称已被其他连接占用，本次未覆盖"
            );
            if current.actual.server_name != target.server_name {
                ensure!(
                    current
                        .state
                        .slots
                        .get(&target.server_name)
                        .is_none_or(Option::is_none),
                    "Codex 环境名称已被其他连接占用，本次未覆盖"
                );
                if current.record.is_some() {
                    desired_slots.insert(current.actual.server_name.clone(), None);
                }
            }
            desired_slots.insert(target.server_name.clone(), desired);
        }
        AdapterOperation::Remove => {
            ensure!(current.record.is_some(), "没有可移除的 Hub 受管 MCP 条目");
            desired_slots.insert(current.actual.server_name.clone(), None);
        }
        AdapterOperation::Restore => {
            let restore_ref =
                restore_ref.ok_or_else(|| anyhow::anyhow!("还原缺少受保护备份引用"))?;
            ensure!(
                current
                    .record
                    .as_ref()
                    .is_some_and(|record| record.backup_ref == restore_ref),
                "备份不属于当前受管目标"
            );
            let bytes = load_backup(&ctx.secrets, restore_ref)?;
            if let Ok(backup) = serde_json::from_slice::<CodexBackup>(&bytes) {
                validate_backup(&backup, target, &location)?;
                ensure!(
                    backup
                        .next_records
                        .get(&current.actual_location.key)
                        .is_some_and(|record| {
                            record.backup_ref == restore_ref
                                && current.record.as_ref().is_some_and(|actual| {
                                    record.server_name == actual.server_name
                                        && record.fingerprint == actual.fingerprint
                                })
                        }),
                    "Codex 名称迁移备份不属于当前受管连接"
                );
                let state =
                    read_state(ctx, &current.actual, &location, &backup_names(&backup)).await?;
                ensure!(
                    matches_after(&backup, &state)?,
                    "Codex 新旧名称在迁移后已变化，本次未还原"
                );
                for slot in backup.slots {
                    desired_slots.insert(slot.server_name, slot.before);
                }
                next_records = backup.previous_records;
            } else {
                let backup: ProtectedBackup = serde_json::from_slice(&bytes)
                    .map_err(|_| anyhow::anyhow!("受保护 MCP 备份格式无效"))?;
                ensure!(
                    backup.version == 1
                        && backup.target_key == current.actual_location.key
                        && backup.after_fingerprint
                            == slot_fingerprint(&current.state, &current.actual.server_name)?,
                    "MCP 目标已变化，不能将备份还原到当前条目"
                );
                desired_slots.insert(current.actual.server_name.clone(), backup.before);
                if let Some(record) = backup.previous_managed {
                    ensure!(
                        !record.pending
                            && record.profile_id == target.profile_id
                            && record.client == ClientKind::Codex
                            && record.server_name == current.actual.server_name
                            && same_path(record.target_path.as_deref(), location.path.as_deref())?
                            && super::location(&record_target(target, &record))?.key
                                == current.actual_location.key,
                        "旧 Codex 备份归属无效"
                    );
                    next_records.insert(current.actual_location.key.clone(), record);
                }
            }
        }
    }
    // Normalize exactly as Codex will serialize the complete file, including
    // both the removed old name and the newly inserted environment name.
    let planned = codex::replace_many(
        current.state.raw.as_deref().unwrap_or_default(),
        &desired_slots,
    )?;
    for (name, snapshot) in &mut desired_slots {
        *snapshot = codex::read(&planned, name)?;
    }
    if !matches!(operation, AdapterOperation::Restore) {
        let actual_name = if matches!(operation, AdapterOperation::Apply) {
            &target.server_name
        } else {
            &current.actual.server_name
        };
        let actual_target = named(&current.actual, actual_name);
        next_records.insert(
            super::location(&actual_target)?.key,
            ManagedEntry {
                profile_id: target.profile_id.clone(),
                client: ClientKind::Codex,
                server_name: actual_name.clone(),
                target_path: location.path.clone(),
                fingerprint: fingerprint(
                    desired_slots.get(actual_name).and_then(Option::as_ref),
                    true,
                    ClientKind::Codex,
                )?,
                backup_ref: reference.clone(),
                pending: false,
            },
        );
    }
    let names: BTreeSet<_> = desired_slots.keys().cloned().collect();
    let before = read_state(ctx, &current.actual, &location, &names).await?;
    ensure!(
        before.raw == current.state.raw,
        "Codex 配置在预览核对后已变化，本次未覆盖"
    );
    let previous_records = scoped_records(target, &location, &ledger)?
        .into_iter()
        .filter(|(_, record)| names.contains(&record.server_name))
        .collect();
    let mut slots = Vec::new();
    for (name, after) in &desired_slots {
        slots.push(CodexSlot {
            server_name: name.clone(),
            before: before.slots.get(name).cloned().flatten(),
            after_fingerprint: fingerprint(after.as_ref(), false, ClientKind::Codex)?,
        });
    }
    let backup = CodexBackup {
        version: 2,
        scope: scope(target, &location)?,
        slots,
        previous_records,
        next_records,
    };
    validate_backup(&backup, target, &location)?;
    save_backup(
        &ctx.secrets,
        &reference,
        &serde_json::to_vec(&backup).map_err(|_| anyhow::anyhow!("无法编码 Codex 名称迁移备份"))?,
    )?;
    install_records(&mut ledger, target, &location, &backup, &BTreeMap::new())?;
    for slot in &backup.slots {
        let key = super::location(&named(&current.actual, &slot.server_name))?.key;
        ledger.entries.insert(
            key,
            ManagedEntry {
                profile_id: target.profile_id.clone(),
                client: ClientKind::Codex,
                server_name: slot.server_name.clone(),
                target_path: location.path.clone(),
                fingerprint: slot.after_fingerprint.clone(),
                backup_ref: reference.clone(),
                pending: true,
            },
        );
    }
    save_ledger(&root, &ledger)?;
    let mutation = write_slots(&location, &before, &desired_slots).await;
    let observed = read_state(ctx, &current.actual, &location, &names).await;
    match observed {
        Ok(after) if matches_after(&backup, &after)? => {
            install_records(
                &mut ledger,
                target,
                &location,
                &backup,
                &backup.next_records,
            )?;
            if save_ledger(&root, &ledger).is_err() {
                // Re-read and compensate only while both exact written slots
                // remain ours. Unrelated file edits are retained in this raw.
                if let Ok(latest) = read_state(ctx, &current.actual, &location, &names).await
                    && matches_after(&backup, &latest)?
                {
                    let original = backup
                        .slots
                        .iter()
                        .map(|slot| (slot.server_name.clone(), slot.before.clone()))
                        .collect();
                    let _ = write_slots(&location, &latest, &original).await;
                    if let Ok(restored) = read_state(ctx, &current.actual, &location, &names).await
                        && matches_before(&backup, &restored)?
                    {
                        install_records(
                            &mut ledger,
                            target,
                            &location,
                            &backup,
                            &backup.previous_records,
                        )?;
                        let _ = save_ledger(&root, &ledger);
                        bail!("接入元数据保存失败；已核对新旧名称未变并补偿还原，请刷新后重试");
                    }
                }
                bail!("Codex 名称迁移元数据提交失败，结果待核对；未覆盖并发修改，保护记录已保留");
            }
            let mut warnings = client_warnings(ClientKind::Codex);
            if mutation.is_err() {
                warnings
                    .push("操作时未收到明确回复，随后已核对新旧名称并确认本次修改已完成。".into());
            }
            let public = plan(ctx, target, &location, &ledger).await?.public;
            Ok(AdapterOutcome {
                backup_ref: public.backup_ref.clone(),
                status: public,
                mutation_done: true,
                warnings,
            })
        }
        Ok(after) => {
            if matches_before(&backup, &after)? {
                install_records(
                    &mut ledger,
                    target,
                    &location,
                    &backup,
                    &backup.previous_records,
                )?;
                save_ledger(&root, &ledger)?;
            }
            bail!("Codex 保存结果未确认或新旧名称被并发修改；没有重放或覆盖，请重新检查");
        }
        Err(_) => bail!("Codex 写入后无法核对新旧名称；保护恢复记录已保留，请重新检查"),
    }
}
