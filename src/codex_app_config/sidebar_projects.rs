//! Rebuild an empty legacy desktop project list from verified local evidence.
//!
//! The desktop global state is deliberately treated as an opaque JSON object.  We
//! only decode `local-projects`; account scoped renderer state, thread membership,
//! ordering, selection, and trust settings are left untouched.

use super::{
    LocalProjectCandidate, ThreadAssignment, local_project_candidates_for_ids, local_project_owner,
    repair_thread_memberships, undo_thread_memberships, write_file_atomically,
};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value, value::RawValue};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

const STATE_FILE: &str = ".codex-global-state.json";
const RECEIPT_FILE: &str = "receipt.json";
const PERSISTED_ATOMS: &str = "electron-persisted-atom-state";
const ACCOUNT_SCOPED_STATE: &str = "electron-account-renderer-state-v1";
const LOCAL_PROJECTS: &str = "local-projects";
const RECEIPT_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarAction {
    Check,
    Repair,
    Undo,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SidebarReport {
    pub(crate) active_project_count: usize,
    pub(crate) candidate_count: usize,
    pub(crate) changed_count: usize,
    pub(crate) grouped_thread_count: usize,
    pub(crate) can_repair: bool,
    pub(crate) can_undo: bool,
    pub(crate) detail: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    version: u32,
    owner_hash: String,
    before_present: bool,
    before_projects: Option<String>,
    after_hash: String,
    after_present: bool,
    sidebar_changed: bool,
    project_ids: Vec<String>,
    thread_assignments: Vec<ThreadAssignment>,
}

/// The receipt is intentionally kept beside the Codex home.  It does not enter
/// global renderer state and can only be consumed by an explicit Undo action.
pub(crate) fn sidebar_receipt_dir(home: &Path) -> PathBuf {
    home.join("hub-project-sidebar-repair")
}

fn state_path(home: &Path) -> PathBuf {
    home.join(STATE_FILE)
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn parse_state(raw: &[u8]) -> Result<BTreeMap<String, Box<RawValue>>> {
    serde_json::from_slice(raw).context("Codex 桌面状态不是 JSON 对象。")
}

fn raw_value(value: &Value) -> Result<Box<RawValue>> {
    Ok(serde_json::value::to_raw_value(value)?)
}

fn persisted_atoms(
    state: &BTreeMap<String, Box<RawValue>>,
) -> Result<BTreeMap<String, Box<RawValue>>> {
    let raw = state
        .get(PERSISTED_ATOMS)
        .ok_or_else(|| anyhow!("Codex 桌面持久状态缺少 electron-persisted-atom-state。"))?;
    serde_json::from_str(raw.get()).context("Codex 桌面持久状态不是 JSON 对象。")
}

fn set_persisted_atoms(
    state: &mut BTreeMap<String, Box<RawValue>>,
    atoms: &BTreeMap<String, Box<RawValue>>,
) -> Result<()> {
    state.insert(
        PERSISTED_ATOMS.into(),
        serde_json::value::to_raw_value(atoms)?,
    );
    Ok(())
}

fn projects_from_state(state: &BTreeMap<String, Box<RawValue>>) -> Result<(bool, Vec<String>)> {
    let atoms = persisted_atoms(state)?;
    let Some(raw) = atoms.get(LOCAL_PROJECTS) else {
        return Ok((false, Vec::new()));
    };
    let value: Value = serde_json::from_str(raw.get())?;
    let Some(projects) = value.as_object() else {
        return Err(anyhow!("桌面 local-projects 不是对象，未执行修复。"));
    };
    Ok((true, projects.keys().cloned().collect()))
}

fn projects_json(candidates: &[LocalProjectCandidate]) -> Result<Value> {
    let mut projects = JsonMap::new();
    for candidate in candidates {
        let mut item = JsonMap::new();
        item.insert("id".into(), Value::String(candidate.id.clone()));
        item.insert("name".into(), Value::String(candidate.name.clone()));
        item.insert(
            "rootPaths".into(),
            Value::Array(vec![Value::String(candidate.root.clone())]),
        );
        item.insert(
            "createdAt".into(),
            Value::Number(candidate.created_at_ms.into()),
        );
        item.insert(
            "updatedAt".into(),
            Value::Number(candidate.updated_at_ms.into()),
        );
        projects.insert(candidate.id.clone(), Value::Object(item));
    }
    Ok(Value::Object(projects))
}

fn empty_or_missing(state: &BTreeMap<String, Box<RawValue>>) -> Result<(bool, Vec<String>)> {
    let (present, ids) = projects_from_state(state)?;
    if !ids.is_empty() {
        return Err(anyhow!("当前桌面项目清单已有内容，保留现状。"));
    }
    Ok((present, ids))
}

fn detail(candidates: &[LocalProjectCandidate], suffix: &str) -> String {
    let names = candidates
        .iter()
        .take(8)
        .map(|candidate| candidate.name.as_str())
        .collect::<Vec<_>>();
    let names = if candidates.len() > names.len() {
        format!("{} 等 {} 项", names.join("、"), candidates.len())
    } else {
        names.join("、")
    };
    if names.is_empty() {
        suffix.to_owned()
    } else {
        format!("{suffix}：{names}")
    }
}

fn load_state(home: &Path) -> Result<Vec<u8>> {
    fs::read(state_path(home)).with_context(|| "找不到 Codex 桌面状态，未执行侧栏修复。")
}

fn owner(home: &Path) -> Result<String> {
    local_project_owner(home)?.ok_or_else(|| anyhow!("当前 Codex 身份不是已核实的 Hub 本机身份。"))
}

fn check(home: &Path) -> Result<SidebarReport> {
    let state_raw = match fs::read(state_path(home)) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(SidebarReport {
                active_project_count: 0,
                candidate_count: 0,
                changed_count: 0,
                grouped_thread_count: 0,
                can_repair: false,
                can_undo: receipt_path(home).is_file(),
                detail: "未找到 Codex 桌面状态，未提供侧栏恢复操作。".into(),
            });
        }
        Err(error) => return Err(error).context("无法读取 Codex 桌面状态。"),
    };
    let state = parse_state(&state_raw)?;
    let _ = persisted_atoms(&state)?;
    if state.contains_key(ACCOUNT_SCOPED_STATE) {
        return Ok(SidebarReport {
            active_project_count: projects_from_state(&state)?.1.len(),
            candidate_count: 0,
            changed_count: 0,
            grouped_thread_count: 0,
            can_repair: false,
            can_undo: receipt_path(home).is_file(),
            detail: "当前桌面状态存在账户作用域记录，无法证明恢复属于当前槽。".into(),
        });
    }
    let (_, ids) = projects_from_state(&state)?;
    let candidates = local_project_candidates_for_ids(home, &ids)?;
    let can_repair = !candidates.is_empty() && owner(home).is_ok();
    Ok(SidebarReport {
        active_project_count: ids.len(),
        candidate_count: candidates.len(),
        changed_count: 0,
        grouped_thread_count: 0,
        can_repair,
        can_undo: receipt_path(home).is_file(),
        detail: if can_repair && ids.is_empty() {
            detail(&candidates, "可按已登记目录重建项目清单并恢复未归属会话")
        } else if can_repair {
            detail(&candidates, "可按已登记目录恢复未归属会话")
        } else if candidates.is_empty() {
            "没有同时满足本机 Hub 身份、ai-gateway 和会话来源校验的项目候选。".into()
        } else {
            "当前项目清单非空或身份作用域无法确认，未提供恢复操作。".into()
        },
    })
}

fn receipt_path(home: &Path) -> PathBuf {
    sidebar_receipt_dir(home).join(RECEIPT_FILE)
}

fn write_state(home: &Path, state: &BTreeMap<String, Box<RawValue>>) -> Result<()> {
    let bytes = serde_json::to_vec(state)?;
    write_file_atomically(&state_path(home), &bytes)
}

fn repair(home: &Path) -> Result<SidebarReport> {
    let state_raw = match fs::read(state_path(home)) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(SidebarReport {
                active_project_count: 0,
                candidate_count: 0,
                changed_count: 0,
                grouped_thread_count: 0,
                can_repair: false,
                can_undo: receipt_path(home).is_file(),
                detail: "未找到 Codex 桌面状态，未执行侧栏恢复。".into(),
            });
        }
        Err(error) => return Err(error).context("无法读取 Codex 桌面状态。"),
    };
    let mut state = parse_state(&state_raw)?;
    let mut atoms = persisted_atoms(&state)?;
    if state.contains_key(ACCOUNT_SCOPED_STATE) {
        return Err(anyhow!("当前桌面状态存在账户作用域记录，停止恢复。"));
    }
    let (before_present, before_ids) = projects_from_state(&state)?;
    let owner_hash = owner(home)?;
    let candidates = local_project_candidates_for_ids(home, &before_ids)?;
    if candidates.is_empty() {
        return Err(anyhow!("没有可核实的本机 Hub 项目候选，停止恢复。"));
    }
    let thread_assignments = repair_thread_memberships(home, &candidates)?;
    let before_projects = atoms
        .get(LOCAL_PROJECTS)
        .map(|value| value.get().to_owned());
    let sidebar_changed = before_ids.is_empty();
    if sidebar_changed {
        let projects = projects_json(&candidates)?;
        atoms.insert(LOCAL_PROJECTS.into(), raw_value(&projects)?);
        set_persisted_atoms(&mut state, &atoms)?;
        write_state(home, &state)?;
    }
    if !sidebar_changed && thread_assignments.is_empty() {
        return Err(anyhow!("当前项目清单和会话归属均无需恢复。"));
    }
    let (after_present, after_hash) = atoms
        .get(LOCAL_PROJECTS)
        .map(|value| (true, digest(value.get().as_bytes())))
        .unwrap_or_else(|| (false, digest(b"missing")));
    let receipt = Receipt {
        version: RECEIPT_VERSION,
        owner_hash,
        before_present,
        before_projects,
        after_hash,
        after_present,
        sidebar_changed,
        project_ids: candidates
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect(),
        thread_assignments: thread_assignments.clone(),
    };
    let receipt_bytes = serde_json::to_vec_pretty(&receipt)?;
    fs::create_dir_all(sidebar_receipt_dir(home))?;
    write_file_atomically(&receipt_path(home), &receipt_bytes)?;
    Ok(SidebarReport {
        active_project_count: if sidebar_changed {
            candidates.len()
        } else {
            before_ids.len()
        },
        candidate_count: candidates.len(),
        changed_count: if sidebar_changed { candidates.len() } else { 0 },
        grouped_thread_count: thread_assignments.len(),
        can_repair: false,
        can_undo: true,
        detail: if sidebar_changed && thread_assignments.is_empty() {
            detail(&candidates, "已按已登记目录重建项目清单")
        } else if sidebar_changed {
            format!(
                "{}，并恢复 {} 个未归属会话",
                detail(&candidates, "已按已登记目录重建项目清单"),
                thread_assignments.len()
            )
        } else {
            format!("已恢复 {} 个未归属会话", thread_assignments.len())
        },
    })
}

fn undo(home: &Path) -> Result<SidebarReport> {
    let receipt_raw = fs::read(receipt_path(home)).context("没有本次侧栏修复记录，未执行撤销。")?;
    let receipt: Receipt =
        serde_json::from_slice(&receipt_raw).context("侧栏修复记录无法解析，未执行撤销。")?;
    if receipt.version != RECEIPT_VERSION || owner(home)? != receipt.owner_hash {
        return Err(anyhow!("侧栏修复记录不属于当前已核实身份，未执行撤销。"));
    }
    let state_raw = load_state(home)?;
    let mut state = parse_state(&state_raw)?;
    let mut atoms = persisted_atoms(&state)?;
    if state.contains_key(ACCOUNT_SCOPED_STATE) {
        return Err(anyhow!("当前桌面状态已进入账户作用域，未执行撤销。"));
    }
    let current = atoms.get(LOCAL_PROJECTS);
    if receipt.after_present != (current.is_some())
        || current.is_some_and(|value| digest(value.get().as_bytes()) != receipt.after_hash)
    {
        return Err(anyhow!("当前项目清单已被修改，未执行撤销。"));
    }
    if receipt.sidebar_changed {
        match (receipt.before_present, receipt.before_projects) {
            (true, Some(raw)) => {
                atoms.insert(LOCAL_PROJECTS.into(), RawValue::from_string(raw)?);
            }
            (false, None) => {
                atoms.remove(LOCAL_PROJECTS);
            }
            _ => return Err(anyhow!("侧栏修复记录不完整，未执行撤销。")),
        }
    }
    undo_thread_memberships(home, &receipt.thread_assignments)?;
    if receipt.sidebar_changed {
        set_persisted_atoms(&mut state, &atoms)?;
        write_state(home, &state)?;
    }
    Ok(SidebarReport {
        active_project_count: 0,
        candidate_count: receipt.project_ids.len(),
        changed_count: if receipt.sidebar_changed {
            receipt.project_ids.len()
        } else {
            0
        },
        grouped_thread_count: receipt.thread_assignments.len(),
        can_repair: false,
        can_undo: false,
        detail: "已撤销本次恢复的项目卡片和会话归属；其他桌面状态未改动。".into(),
    })
}

pub(crate) fn sidebar_projects(home: &Path, action: SidebarAction) -> Result<SidebarReport> {
    match action {
        SidebarAction::Check => check(home),
        SidebarAction::Repair => repair(home),
        SidebarAction::Undo => undo(home),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_cards_are_read_from_the_persisted_atom_scope() {
        let mut atoms: BTreeMap<String, Box<RawValue>> = BTreeMap::new();
        atoms.insert(
            LOCAL_PROJECTS.into(),
            RawValue::from_string("{\"project-1\":{}}".into()).unwrap(),
        );
        let mut state = BTreeMap::new();
        state.insert(
            PERSISTED_ATOMS.into(),
            serde_json::value::to_raw_value(&atoms).unwrap(),
        );
        state.insert(
            ACCOUNT_SCOPED_STATE.into(),
            RawValue::from_string("{}".into()).unwrap(),
        );
        assert_eq!(
            projects_from_state(&state).unwrap(),
            (true, vec!["project-1".to_string()])
        );
        let persisted = persisted_atoms(&state).unwrap();
        assert!(persisted.contains_key(LOCAL_PROJECTS));
        assert!(state.contains_key(ACCOUNT_SCOPED_STATE));
    }

    #[test]
    fn empty_project_cards_are_the_only_repair_input() {
        let mut atoms: BTreeMap<String, Box<RawValue>> = BTreeMap::new();
        atoms.insert(
            LOCAL_PROJECTS.into(),
            RawValue::from_string("{}".into()).unwrap(),
        );
        let mut state = BTreeMap::new();
        state.insert(
            PERSISTED_ATOMS.into(),
            serde_json::value::to_raw_value(&atoms).unwrap(),
        );
        assert_eq!(empty_or_missing(&state).unwrap(), (true, Vec::new()));
    }
}
