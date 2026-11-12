//! Ownership changes for the verified TianGong Desktop 1.0.4 local schema.
//!
//! The caller holds the desktop-stop and IM/link guards. This module neither
//! starts the desktop nor edits its login, models, task IDs, or message bodies.
use anyhow::{Context, Result, bail, ensure};
use rusqlite::TransactionBehavior;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

mod database;
mod links;
mod memory;

#[cfg(test)]
mod tests;

pub(crate) const GATEWAY_USER_ID: &str = "ai-gateway";
const LEDGER_VERSION: u32 = 1;
const MAX_SELECTIONS: usize = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryEntry {
    pub session_id: String,
    pub title: String,
    pub user_id: String,
    pub agent_id: String,
    pub project_id: String,
    pub work_dir: String,
    pub updated_at: String,
    pub message_count: u64,
    pub status: String,
    pub is_gateway: bool,
    pub can_move: bool,
    pub can_restore: bool,
    pub migration_id: Option<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryList {
    pub entries: Vec<HistoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSelection {
    pub session_id: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveRequest {
    pub sessions: Vec<SessionSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RestoreRequest {
    pub sessions: Vec<SessionSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveResult {
    pub changed: usize,
    pub backup_path: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum Phase {
    Preparing,
    Moved,
    Restoring,
    Restored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnershipMove {
    id: String,
    session_id: String,
    original_user_id: String,
    agent_id: String,
    project_id: String,
    work_dir: String,
    phase: Phase,
    files: Vec<memory::FileRecord>,
    // Only identities are needed; never snapshot a whole DB or link file.
    links: Vec<links::LinkOwner>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ledger {
    version: u32,
    database: PathBuf,
    moves: Vec<OwnershipMove>,
}

impl Ledger {
    fn load(database: &Path, hub_config: &Path) -> Result<Self> {
        let database = database.canonicalize().context("无法定位天工历史数据库")?;
        let Some(bytes) = memory::read(&ledger_path(hub_config))? else {
            return Ok(Self {
                version: LEDGER_VERSION,
                database,
                moves: Vec::new(),
            });
        };
        let ledger: Self = serde_json::from_slice(&bytes)
            .context("天工历史撤销记录格式不兼容，请保留记录后再处理")?;
        ensure!(
            ledger.version == LEDGER_VERSION && ledger.database == database,
            "天工历史撤销记录的版本或数据库位置不一致，未修改历史"
        );
        ensure!(ledger.moves.len() <= 20_000, "天工历史撤销记录数量过多");
        Ok(ledger)
    }

    fn latest(&self, session_id: &str) -> Option<&OwnershipMove> {
        self.moves
            .iter()
            .rev()
            .find(|item| item.session_id == session_id && item.phase != Phase::Restored)
    }
}

fn ledger_path(hub_config: &Path) -> PathBuf {
    crate::desktop::state_root(hub_config).join("tiangong-history/ownership-moves.json")
}

fn validate_selections(sessions: &[SessionSelection]) -> Result<()> {
    ensure!(
        !sessions.is_empty() && sessions.len() <= MAX_SELECTIONS,
        "请选择 1 至 1000 条天工会话"
    );
    let mut ids = HashSet::new();
    for item in sessions {
        memory::validate_component(&item.session_id)?;
        memory::validate_component(&item.user_id)?;
        ensure!(ids.insert(&item.session_id), "同一会话不能重复选择");
    }
    Ok(())
}

pub(crate) fn list_at(database: &Path, hub_config: &Path) -> Result<HistoryList> {
    let connection = database::open(database, false)?;
    database::check_schema(&connection)?;
    let ledger = Ledger::load(database, hub_config)?;
    let link_file = links::LinkFile::load(hub_config)?;
    let mut states = database::load(&connection, database, None)?;
    for state in &mut states {
        let entry = &mut state.entry;
        let migration = ledger.latest(&entry.session_id);
        let recovery_owner = migration
            .filter(|item| matches!(item.phase, Phase::Preparing | Phase::Restoring))
            .map(|item| {
                if entry.user_id == GATEWAY_USER_ID {
                    item.original_user_id.as_str()
                } else {
                    GATEWAY_USER_ID
                }
            });
        if entry.unavailable_reason.is_none() {
            entry.unavailable_reason =
                links::check(&link_file, entry, &state.task_ids, recovery_owner)
                    .err()
                    .map(|error| error.to_string());
        }
        if let Some(item) = migration {
            entry.migration_id = Some(item.id.clone());
            let expected_owner = entry.user_id == GATEWAY_USER_ID
                || (matches!(item.phase, Phase::Preparing | Phase::Restoring)
                    && entry.user_id == item.original_user_id);
            entry.can_restore =
                expected_owner && entry.unavailable_reason.is_none() && same_identity(entry, item);
            entry.can_move = false;
            if !expected_owner || !same_identity(entry, item) {
                entry.unavailable_reason = Some("历史归属或项目已被修改，不能覆盖现有改动".into());
            }
        } else {
            entry.can_move = !entry.is_gateway && entry.unavailable_reason.is_none();
        }
    }
    let mut entries: Vec<_> = states.into_iter().map(|state| state.entry).collect();
    entries.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then(a.session_id.cmp(&b.session_id))
    });
    Ok(HistoryList { entries })
}

fn same_identity(entry: &HistoryEntry, item: &OwnershipMove) -> bool {
    entry.agent_id == item.agent_id
        && entry.project_id == item.project_id
        && memory::path_key(Path::new(&entry.work_dir))
            == memory::path_key(Path::new(&item.work_dir))
}

pub(crate) fn move_at(
    database: &Path,
    hub_config: &Path,
    request: &MoveRequest,
) -> Result<MoveResult> {
    validate_selections(&request.sessions)?;
    let mut connection = database::open(database, true)?;
    database::check_schema(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .context("天工历史正在被其他程序使用，请退出天工后重试")?;
    let mut ledger = Ledger::load(database, hub_config)?;
    let before_ledger = serde_json::to_vec_pretty(&ledger)?;
    let mut link_file = links::LinkFile::load(hub_config)?;
    let mut edits = Vec::new();
    let mut warnings = Vec::new();
    let first_record = ledger.moves.len();
    for selected in &request.sessions {
        let state = database::selected(&transaction, database, selected)?;
        ensure!(
            ledger.latest(&selected.session_id).is_none(),
            "此会话已有迁移记录，请先撤销或处理上次操作"
        );
        ensure!(
            !state.entry.is_gateway,
            "此会话已属于 AI Gateway，无需重复迁移"
        );
        links::check(&link_file, &state.entry, &state.task_ids, None)?;
        let keys = database::memory_keys(&transaction, &state.entry)?;
        let (files, mut planned) = memory::plan_move(&state.entry, &keys)?;
        ensure!(
            state.entry.message_count == 0
                || files
                    .first()
                    .is_some_and(|file| file.source_digest.is_some()),
            "此会话缺少原始 JSONL 上下文，数据库消息索引不能重建正文，未迁移归属"
        );
        edits.append(&mut planned);
        let original_links = link_file.change(
            &state.entry.session_id,
            &state.entry.user_id,
            GATEWAY_USER_ID,
        )?;
        database::change_owner(
            &transaction,
            &state.entry,
            &state.entry.user_id,
            GATEWAY_USER_ID,
            &keys,
        )?;
        ledger.moves.push(OwnershipMove {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: state.entry.session_id,
            original_user_id: state.entry.user_id,
            agent_id: state.entry.agent_id,
            project_id: state.entry.project_id,
            work_dir: state.entry.work_dir,
            phase: Phase::Preparing,
            files,
            links: original_links,
        });
    }
    ensure!(ledger.moves.len() <= 20_000, "天工历史撤销记录数量已达上限");
    if let Some(edit) = link_file.edit()? {
        edits.push(edit);
    }
    let path = ledger_path(hub_config);
    commit_files(transaction, &path, &before_ledger, &ledger, edits)?;
    for item in &mut ledger.moves[first_record..] {
        item.phase = Phase::Moved;
    }
    if memory::write(&path, Some(&serde_json::to_vec_pretty(&ledger)?)).is_err() {
        warnings.push("归属已保存；撤销记录保留为可恢复状态".into());
    }
    Ok(MoveResult {
        changed: request.sessions.len(),
        backup_path: Some(path.to_string_lossy().into_owned()),
        warnings,
    })
}

pub(crate) fn restore_at(
    database: &Path,
    hub_config: &Path,
    request: &RestoreRequest,
) -> Result<MoveResult> {
    validate_selections(&request.sessions)?;
    let mut connection = database::open(database, true)?;
    database::check_schema(&connection)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .context("天工历史正在被其他程序使用，请退出天工后重试")?;
    let mut ledger = Ledger::load(database, hub_config)?;
    let before_ledger = serde_json::to_vec_pretty(&ledger)?;
    let mut link_file = links::LinkFile::load(hub_config)?;
    let mut edits = Vec::new();
    let mut warnings = Vec::new();
    let mut restored = Vec::new();
    for selected in &request.sessions {
        let state = database::selected(&transaction, database, selected)?;
        let index = ledger
            .moves
            .iter()
            .rposition(|item| {
                item.session_id == selected.session_id && item.phase != Phase::Restored
            })
            .context("此会话没有可撤销的归属迁移")?;
        let item = &ledger.moves[index];
        memory::validate_component(&item.original_user_id)?;
        ensure!(
            item.original_user_id != GATEWAY_USER_ID,
            "天工历史撤销记录的原始归属无效"
        );
        ensure!(
            same_identity(&state.entry, item),
            "会话的智能体、项目或目录已变化，未撤销归属"
        );
        ensure!(
            state.entry.user_id == GATEWAY_USER_ID
                || (matches!(item.phase, Phase::Preparing | Phase::Restoring)
                    && state.entry.user_id == item.original_user_id),
            "会话归属已被其他操作修改，未撤销"
        );
        let recovery_owner = matches!(item.phase, Phase::Preparing | Phase::Restoring).then_some(
            if state.entry.user_id == GATEWAY_USER_ID {
                item.original_user_id.as_str()
            } else {
                GATEWAY_USER_ID
            },
        );
        links::check(&link_file, &state.entry, &state.task_ids, recovery_owner)?;
        let keys = database::memory_keys_for(&transaction, &state.entry, GATEWAY_USER_ID)?;
        let (mut planned, mut messages) = memory::plan_restore(&state.entry, item, &keys)?;
        edits.append(&mut planned);
        warnings.append(&mut messages);
        link_file.restore(&state.entry.session_id, &item.original_user_id)?;
        // Query current session-scoped rows so memory created since the move is
        // restored as well. Messages, task records, and compaction stay in place.
        database::restore_owner(&transaction, &state.entry, &item.original_user_id, &keys)?;
        ledger.moves[index].phase = Phase::Restoring;
        restored.push(index);
    }
    if let Some(edit) = link_file.edit()? {
        edits.push(edit);
    }
    let path = ledger_path(hub_config);
    commit_files(transaction, &path, &before_ledger, &ledger, edits)?;
    for index in restored {
        ledger.moves[index].phase = Phase::Restored;
    }
    if memory::write(&path, Some(&serde_json::to_vec_pretty(&ledger)?)).is_err() {
        warnings.push("归属已撤销；可再次执行撤销以完成记录整理".into());
    }
    Ok(MoveResult {
        changed: request.sessions.len(),
        backup_path: Some(path.to_string_lossy().into_owned()),
        warnings,
    })
}

fn commit_files(
    transaction: rusqlite::Transaction<'_>,
    ledger_path: &Path,
    before_ledger: &[u8],
    ledger: &Ledger,
    edits: Vec<memory::Edit>,
) -> Result<()> {
    memory::check_edits(&edits)?;
    memory::write(ledger_path, Some(&serde_json::to_vec_pretty(ledger)?))
        .context("无法保存天工历史撤销记录，历史未修改")?;
    let mut applied = 0;
    let result = (|| -> Result<()> {
        for edit in &edits {
            applied += 1;
            memory::write(&edit.path, edit.after.as_deref())?;
        }
        transaction.commit().context("无法提交天工历史归属变更")?;
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_ok = true;
        for edit in edits[..applied].iter().rev() {
            rollback_ok &= memory::rollback(edit).is_ok();
        }
        if rollback_ok {
            rollback_ok = memory::write(ledger_path, Some(before_ledger)).is_ok();
        }
        if !rollback_ok {
            bail!("天工历史归属操作未完成，撤销记录已保留，请先使用撤销恢复；原因：{error}");
        }
        return Err(error.context("天工历史归属未修改，已回滚文件更改"));
    }
    Ok(())
}
