use super::*;
use crate::desktop::{permissions::owner_only, transaction::reject_links};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Component,
};

const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FileRecord {
    relative_path: String,
    pub source_digest: Option<String>,
    target_digest: Option<String>,
    created: bool,
}

pub(super) struct Edit {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

pub(super) fn path_key(path: &Path) -> String {
    let value = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_owned();
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

pub(super) fn validate_component(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 256
            && value != "."
            && value != ".."
            && !value.ends_with(['.', ' '])
            && !value
                .chars()
                .any(|character| character.is_control() || "<>:\"/\\|?*".contains(character)),
        "会话、账号、智能体或项目标识无法安全用于本地路径"
    );
    let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    ensure!(
        !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            && !(stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit()),
        "本地历史标识使用了系统保留名称"
    );
    Ok(())
}

pub(super) fn validate_directory(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute()
            && !path
                .components()
                .any(|part| matches!(part, Component::ParentDir)),
        "天工工作目录必须为不含上级跳转的完整路径"
    );
    reject_links(path)
}

pub(super) fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    reject_links(path)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => bail!("无法读取天工历史文件，请检查文件权限"),
    };
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "天工历史文件路径被目录占用");
    ensure!(
        metadata.len() <= MAX_FILE_BYTES,
        "单个天工历史文件超过 128 MiB，请先在天工整理历史"
    );
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_FILE_BYTES, "天工历史文件过大");
    Ok(Some(bytes))
}

pub(super) fn write(path: &Path, bytes: Option<&[u8]>) -> Result<()> {
    reject_links(path)?;
    let Some(bytes) = bytes else {
        match fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("无法整理本次创建的天工历史副本"),
        }
    };
    let directory = path.parent().context("天工历史文件缺少所在目录")?;
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    owner_only(temporary.path())?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .context("无法保存天工历史文件")?
        .sync_all()?;
    Ok(())
}

pub(super) fn check_edits(edits: &[Edit]) -> Result<()> {
    let mut paths = HashSet::new();
    let mut size = 0usize;
    for edit in edits {
        ensure!(
            paths.insert(path_key(&edit.path)),
            "选中会话引用了同一记忆文件，请分开处理"
        );
        size = size
            .saturating_add(edit.before.as_ref().map_or(0, Vec::len))
            .saturating_add(edit.after.as_ref().map_or(0, Vec::len));
        ensure!(
            size <= 512 * 1024 * 1024,
            "本批历史文件超过 512 MiB，请分批选择"
        );
        ensure!(
            read(&edit.path)? == edit.before,
            "天工历史文件刚被其他程序修改，请刷新后重试"
        );
    }
    Ok(())
}

pub(super) fn rollback(edit: &Edit) -> Result<()> {
    let current = read(&edit.path)?;
    if current == edit.before {
        return Ok(());
    }
    ensure!(
        current == edit.after,
        "历史文件在操作期间被外部修改，已保留现状"
    );
    write(&edit.path, edit.before.as_deref())
}

fn digest(bytes: Option<&[u8]>) -> Option<String> {
    bytes.map(|bytes| hex::encode(Sha256::digest(bytes)))
}

fn scope(entry: &HistoryEntry, user: &str) -> Result<PathBuf> {
    validate_component(user)?;
    validate_component(&entry.project_id)?;
    validate_directory(Path::new(&entry.work_dir))?;
    Ok(Path::new(&entry.work_dir)
        .join(".tiangong/memory")
        .join(user)
        .join("memory")
        .join(&entry.project_id))
}

fn session_file(session_id: &str) -> String {
    format!(
        "session-{}.jsonl",
        session_id.strip_prefix("session-").unwrap_or(session_id)
    )
}

fn legacy_file(session_id: &str) -> String {
    format!(
        "runtime/session-{}.json",
        session_id.strip_prefix("session-").unwrap_or(session_id)
    )
}

fn relative_paths(entry: &HistoryEntry, keys: &[database::MemoryKey]) -> Result<Vec<String>> {
    let mut files = vec![
        session_file(&entry.session_id),
        legacy_file(&entry.session_id),
    ];
    for key in keys.iter().filter(|key| key.layer == 5) {
        validate_component(&key.slug)?;
        files.push(format!("feedback-{}.md", key.slug));
        files.push(format!("reference-{}.md", key.slug));
    }
    let mut seen = HashSet::new();
    files.retain(|file| seen.insert(file.clone()));
    Ok(files)
}

fn check_relative(entry: &HistoryEntry, relative: &str) -> Result<()> {
    if relative == session_file(&entry.session_id) || relative == legacy_file(&entry.session_id) {
        return Ok(());
    }
    ensure!(
        !relative.contains(['/', '\\'])
            && relative.ends_with(".md")
            && (relative.starts_with("feedback-") || relative.starts_with("reference-")),
        "天工历史撤销清单包含不受支持的记忆路径"
    );
    validate_component(relative)
}

pub(super) fn plan_move(
    entry: &HistoryEntry,
    keys: &[database::MemoryKey],
) -> Result<(Vec<FileRecord>, Vec<Edit>)> {
    let source = scope(entry, &entry.user_id)?;
    let target = scope(entry, GATEWAY_USER_ID)?;
    let mut records = Vec::new();
    let mut edits = Vec::new();
    for relative in relative_paths(entry, keys)? {
        let before = read(&source.join(&relative))?;
        let existing = read(&target.join(&relative))?;
        // Keep the absent primary in the manifest: a later turn may create it,
        // and undo must bring that new context back to the original owner.
        if before.is_none() && existing.is_none() && relative != session_file(&entry.session_id) {
            continue;
        }
        ensure!(
            existing.is_none() || existing == before,
            "AI Gateway 下已有不同的同名会话或记忆文件，未覆盖历史"
        );
        check_state(&relative, before.as_deref())?;
        let created = before.is_some() && existing.is_none();
        let source_digest = digest(before.as_deref());
        records.push(FileRecord {
            relative_path: relative.clone(),
            target_digest: source_digest.clone(),
            source_digest,
            created,
        });
        if created {
            edits.push(Edit {
                path: target.join(relative),
                before: existing,
                after: before,
            });
        }
    }
    Ok((records, edits))
}

pub(super) fn plan_restore(
    entry: &HistoryEntry,
    movement: &OwnershipMove,
    keys: &[database::MemoryKey],
) -> Result<(Vec<Edit>, Vec<String>)> {
    let source = scope(entry, &movement.original_user_id)?;
    let target = scope(entry, GATEWAY_USER_ID)?;
    let mut edits = Vec::new();
    let mut warnings = Vec::new();
    ensure!(movement.files.len() <= 2_002, "天工历史记忆撤销清单过大");
    let mut files = movement.files.clone();
    for relative_path in relative_paths(entry, keys)? {
        if !files.iter().any(|file| file.relative_path == relative_path) {
            files.push(FileRecord {
                relative_path,
                source_digest: None,
                target_digest: None,
                created: false,
            });
        }
    }
    for record in &files {
        check_relative(entry, &record.relative_path)?;
        let original = source.join(&record.relative_path);
        let migrated = target.join(&record.relative_path);
        let old = read(&original)?;
        let current = read(&migrated)?;
        if record.relative_path == session_file(&entry.session_id)
            && entry.user_id == GATEWAY_USER_ID
            && entry.message_count > 0
        {
            ensure!(
                current.is_some(),
                "AI Gateway 的会话上下文文件已缺失，无法保证完整恢复，未撤销归属"
            );
        }
        check_state(&record.relative_path, old.as_deref())?;
        check_state(&record.relative_path, current.as_deref())?;
        let source_changed = digest(old.as_deref()) != record.source_digest;
        let target_changed = digest(current.as_deref()) != record.target_digest;
        if current.is_some() && old != current {
            ensure!(
                !source_changed || !target_changed,
                "原账号和 AI Gateway 的同一会话记忆都已更改，无法安全合并，未撤销归属"
            );
            if !source_changed {
                // New messages, compact checkpoints and runtime events travel
                // back together; never revert the whole DB to an old snapshot.
                edits.push(Edit {
                    path: original,
                    before: old,
                    after: current.clone(),
                });
            }
        }
        if record.created
            && current.is_some()
            && !target_changed
            && (record.relative_path == session_file(&entry.session_id)
                || record.relative_path == legacy_file(&entry.session_id))
        {
            edits.push(Edit {
                path: migrated,
                before: current,
                after: None,
            });
        } else if current.is_some() && target_changed {
            warnings.push(
                "迁移后有更新的 AI Gateway 记忆副本已保留，当前上下文同时恢复到原账号".into(),
            );
        }
    }
    warnings.sort();
    warnings.dedup();
    Ok((edits, warnings))
}

fn check_state(relative: &str, bytes: Option<&[u8]>) -> Result<()> {
    let Some(bytes) = bytes else {
        return Ok(());
    };
    if relative.ends_with(".jsonl") {
        let text = std::str::from_utf8(bytes).context("天工会话上下文编码不兼容，未修改历史")?;
        let mut runtime = None;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let event: serde_json::Value = serde_json::from_str(line)
                .context("天工会话上下文存在无法识别的记录，请先在天工核对")?;
            if event.get("subtype").and_then(serde_json::Value::as_str) == Some("runtime_state") {
                if event
                    .get("runtime_type")
                    .and_then(serde_json::Value::as_str)
                    == Some("cleared")
                {
                    runtime = None;
                } else {
                    runtime = Some(
                        event
                            .get("runtime_data")
                            .cloned()
                            .context("天工会话运行状态不完整，未修改历史")?,
                    );
                }
            }
        }
        if let Some(runtime) = runtime {
            check_runtime(&runtime)?;
        }
    } else if relative.starts_with("runtime/") {
        let value = serde_json::from_slice(bytes).context("天工旧版运行状态格式不兼容")?;
        check_runtime(&value)?;
    }
    Ok(())
}

fn check_runtime(runtime: &serde_json::Value) -> Result<()> {
    ensure!(
        matches!(
            runtime
                .get("session_state")
                .and_then(serde_json::Value::as_str),
            Some("idle" | "paused")
        ) && runtime
            .get("awaiting_state")
            .is_some_and(serde_json::Value::is_null),
        "此会话上下文仍有运行中、待审批或未知状态，请先在天工处理"
    );
    if let Some(processes) = runtime.get("process_sessions") {
        let processes = processes.as_object().context("天工后台命令状态不兼容")?;
        ensure!(
            processes.values().all(|process| matches!(
                process.get("status").and_then(serde_json::Value::as_str),
                Some("completed" | "exited" | "stopped" | "failed" | "cancelled")
            )),
            "此会话还有未确认结束的后台命令，请先在天工处理"
        );
    }
    Ok(())
}
