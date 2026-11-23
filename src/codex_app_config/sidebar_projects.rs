//! Recover the active desktop's project cards and the native conversation memberships.
use super::{
    LocalProjectCandidate, ThreadAssignment, local_project_owner, repair_thread_memberships,
    undo_thread_memberships, write_file_atomically,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json, value::RawValue};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

const STATE_FILE: &str = ".codex-global-state.json";
const ACCOUNT_SCOPE: &str = "electron-account-renderer-state-v1";
const CARDS: &str = "local-projects";
const MEMBERS: &str = "thread-project-assignments";
const PROJECTLESS: &str = "projectless-thread-ids";
const SERVER_IDS: &str = "app-server-project-id-by-legacy-project-id-by-host";
const MIGRATION: &str = "app-server-projects-migration-by-host";
const ORDER: &str = "project-order";
const DELETIONS: &str = "app-server-pending-project-deletions-by-host";
type State = BTreeMap<String, Box<RawValue>>;
#[path = "sidebar_store.rs"]
mod sidebar_store;
use sidebar_store::Store;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarAction {
    Check,
    Repair,
    #[cfg(test)]
    Undo,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SidebarReport {
    pub(crate) active_project_count: usize,
    pub(crate) planned_projects: usize,
    pub(crate) planned_threads: usize,
    pub(crate) changed_count: usize,
    pub(crate) grouped_thread_count: usize,
    pub(crate) can_repair: bool,
    pub(crate) can_undo: bool,
    pub(crate) skipped: bool,
    pub(crate) detail: String,
    #[serde(skip)] // Titles are for the local repair dialog, not reports or logs.
    pub(crate) projectless_choices: Vec<ProjectlessChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectlessChoice {
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
    pub(crate) title: String,
    pub(crate) project_name: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    version: u32,
    #[serde(default)]
    store: Store,
    owner: String,
    db_dir: PathBuf,
    cards_before: Option<Box<RawValue>>,
    cards_after: Option<Value>,
    cards_added: usize,
    assignments: Vec<ThreadAssignment>,
    members_present: bool,
    member_changes: BTreeMap<String, (Option<Value>, Value)>,
    previous: Option<Box<RawValue>>,
    #[serde(default)]
    order_change: Option<(Option<Vec<String>>, Vec<String>)>,
    #[serde(default)]
    map_change: Option<MapChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    removed_markers: Vec<(usize, String)>,
}

#[derive(Serialize, Deserialize)]
struct MapChange {
    host: String,
    had_map: bool,
    had_host: bool,
    added: BTreeMap<String, String>,
}

pub(crate) fn sidebar_receipt_dir(home: &Path) -> PathBuf {
    home.join("hub-project-sidebar-repair")
}
fn receipt_path(home: &Path) -> PathBuf {
    sidebar_receipt_dir(home).join("receipt.json")
}
fn read_key<T: DeserializeOwned + Default>(state: &State, key: &str) -> Result<T> {
    state
        .get(key)
        .map(|v| serde_json::from_str(v.get()))
        .transpose()
        .with_context(|| format!("无法读取桌面项目设置 {key}。"))
        .map(|v| v.unwrap_or_default())
}
fn current_cards(state: &State) -> Result<BTreeMap<String, Value>> {
    read_key(state, CARDS)
}
fn has_cloud_link(card: &Value) -> bool {
    [
        "chatGptBacking",
        "retiredChatGptBacking",
        "chatGptBackingCreation",
    ]
    .iter()
    .any(|key| !card[*key].is_null())
}
// Only the active same-owner slice of the sibling backup is admissible.
// It can corroborate an existing native project binding, never supply rows for import.
fn matching_backup_cards(home: &Path, owner: &str) -> Option<HashMap<String, String>> {
    let path = home.join(".codex-global-state.json.bak");
    if !fs::symlink_metadata(&path).ok()?.is_file() {
        return None;
    }
    let state: State = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    if !state.contains_key(ACCOUNT_SCOPE) && !state.contains_key(sidebar_store::OWNER) {
        return None;
    }
    verify_owner(&state, owner).ok()?;
    let mapping: BTreeMap<String, String> = local_map(&state, SERVER_IDS, home).ok()?;
    let deletions: BTreeMap<String, Value> = local_map(&state, DELETIONS, home).ok()?;
    let deleted: HashSet<_> = deletions
        .iter()
        .flat_map(|(id, row)| {
            [
                Some(mapping.get(id).unwrap_or(id).as_str()),
                row["projectId"].as_str(),
            ]
            .into_iter()
            .flatten()
        })
        .collect();
    let mut result = HashMap::new();
    let mut duplicates = HashSet::new();
    for (id, card) in current_cards(&state).ok()? {
        let native = mapping.get(&id).unwrap_or(&id);
        if card["id"].as_str() != Some(id.as_str())
            || deleted.contains(native.as_str())
            || has_cloud_link(&card)
        {
            continue;
        }
        let Some([root]) = card["rootPaths"].as_array().map(Vec::as_slice) else {
            continue;
        };
        if let Some(path) = root.as_str() {
            if result.insert(native.clone(), path.to_owned()).is_some() {
                duplicates.insert(native.clone());
            }
        }
    }
    result.retain(|id, _| !duplicates.contains(id));
    Some(result)
}
fn verify_owner(state: &State, owner: &str) -> Result<()> {
    // Electron stores only the active globals at the top level. Inactive slots
    // are saved separately and must never be read as an alternative source.
    if let Some(raw) = state.get(sidebar_store::OWNER) {
        ensure!(
            serde_json::from_str::<String>(raw.get()).ok().as_deref() == Some(owner),
            "Codex 当前侧栏属于其他账户或尚未确定账户。请用原账户打开一次再退出。"
        );
    }
    if let Some(raw) = state.get(ACCOUNT_SCOPE) {
        #[derive(Deserialize)]
        struct Scope {
            owner: Option<String>,
        }
        let scope: Scope = serde_json::from_str(raw.get())?;
        ensure!(
            scope.owner.as_deref() == Some(owner),
            "Codex 当前侧栏属于其他账户或尚未确定账户。请用原账户打开一次再退出。"
        );
    }
    Ok(())
}
fn local_map<T: DeserializeOwned>(
    state: &State,
    key: &str,
    home: &Path,
) -> Result<BTreeMap<String, T>> {
    let all: BTreeMap<String, Box<RawValue>> = read_key(state, key)?;
    Ok(local_host(all.keys(), home)?
        .as_ref()
        .and_then(|key| all.get(key))
        .map(|raw| serde_json::from_str(raw.get()))
        .transpose()?
        .unwrap_or_default())
}
fn local_host<'a>(hosts: impl Iterator<Item = &'a String>, home: &Path) -> Result<Option<String>> {
    let current_home = home.canonicalize()?;
    let mut current = None;
    for host in hosts {
        let Some(path) = host.strip_prefix("local:") else {
            continue;
        };
        let path = Path::new(path);
        if path.is_absolute()
            && (path == home || path.canonicalize().is_ok_and(|p| p == current_home))
        {
            ensure!(current.is_none(), "本机项目身份记录不唯一。");
            current = Some(host.clone());
        }
    }
    Ok(current)
}
fn map_host(state: &State, home: &Path) -> Result<String> {
    for key in [SERVER_IDS, MIGRATION, DELETIONS] {
        let hosts: BTreeMap<String, Box<RawValue>> = read_key(state, key)?;
        if let Some(host) = local_host(hosts.keys(), home)? {
            return Ok(host);
        }
    }
    let path = home.display().to_string();
    Ok(format!(
        "local:{}",
        path.strip_prefix(r"\\?\").unwrap_or(&path)
    ))
}
fn edit_map(state: &mut State, change: &MapChange, undo: bool) -> Result<()> {
    let mut groups: BTreeMap<String, Box<RawValue>> = read_key(state, SERVER_IDS)?;
    let mut ids: BTreeMap<String, String> = groups
        .get(&change.host)
        .map(|v| serde_json::from_str(v.get()))
        .transpose()?
        .unwrap_or_default();
    for (legacy, native) in &change.added {
        if undo {
            ensure!(
                ids.get(legacy) == Some(native),
                "侧栏项目关联已被修改，未撤销本次修复。"
            );
            ids.remove(legacy);
        } else {
            ids.insert(legacy.clone(), native.clone());
        }
    }
    if undo && !change.had_host && ids.is_empty() {
        groups.remove(&change.host);
    } else {
        groups.insert(change.host.clone(), serde_json::value::to_raw_value(&ids)?);
    }
    if undo && !change.had_map && groups.is_empty() {
        state.remove(SERVER_IDS);
    } else {
        state.insert(SERVER_IDS.into(), serde_json::value::to_raw_value(&groups)?);
    }
    Ok(())
}
fn store_previous(path: &Path, previous: Option<&RawValue>) -> Result<()> {
    if let Some(raw) = previous {
        write_file_atomically(path, raw.get().as_bytes())
    } else {
        fs::remove_file(path).context("无法移除本次恢复记录。")
    }
}
fn desktop_plan(
    state: &State,
    candidates: &mut [LocalProjectCandidate],
    mapping: &BTreeMap<String, String>,
    cards: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, (Option<Value>, Value)>> {
    let mut changes = BTreeMap::new();
    let excluded: HashSet<String> = read_key(state, "projectless-thread-ids")?;
    let assignments: BTreeMap<String, Value> = read_key(state, MEMBERS)?;
    let hosts: BTreeMap<String, String> = read_key(state, "thread-project-membership-host-ids")?;
    for p in candidates {
        let mut legacy = mapping.iter().filter(|(_, v)| *v == &p.id).map(|(k, _)| k);
        let target = legacy.next().unwrap_or(&p.id);
        ensure!(legacy.next().is_none(), "项目标识对应多个侧栏位置。");
        let next = json!({"projectKind":"local","projectId":target});
        p.pending_assignments.retain(|a| {
            if excluded.contains(&a.thread_id)
                || hosts.get(&a.thread_id).is_some_and(|h| h != "local")
            {
                return false;
            }
            let before = assignments.get(&a.thread_id);
            if let Some(before) = before {
                if before == &next {
                    return true;
                }
                let Some(old) = before["projectId"].as_str() else {
                    return false;
                };
                if before["projectKind"] != "local"
                    || cards.contains_key(old)
                    || a.previous_project_id
                        .as_deref()
                        .or(p.predecessor_id.as_deref())
                        != Some(mapping.get(old).map(String::as_str).unwrap_or(old))
                {
                    return false;
                }
            }
            changes.insert(a.thread_id.clone(), (before.cloned(), next.clone()));
            true
        });
    }
    Ok(changes)
}
fn edit_members(
    state: &mut State,
    changes: &BTreeMap<String, (Option<Value>, Value)>,
    undo: bool,
    present: bool,
) -> Result<()> {
    if changes.is_empty() {
        return Ok(());
    }
    let mut entries: BTreeMap<String, Value> = read_key(state, MEMBERS)?;
    let excluded: HashSet<String> = read_key(state, "projectless-thread-ids")?;
    let hosts: BTreeMap<String, String> = read_key(state, "thread-project-membership-host-ids")?;
    for (thread, (before, after)) in changes {
        if undo {
            ensure!(
                entries.get(thread) == Some(after)
                    && !excluded.contains(thread)
                    && hosts.get(thread).is_none_or(|h| h == "local"),
                "会话的桌面分组已修改，未执行撤销。"
            );
            if let Some(before) = before {
                entries.insert(thread.clone(), before.clone());
            } else {
                entries.remove(thread);
            }
        } else {
            entries.insert(thread.clone(), after.clone());
        }
    }
    if undo && entries.is_empty() && !present {
        state.remove(MEMBERS);
    } else {
        state.insert(MEMBERS.into(), serde_json::value::to_raw_value(&entries)?);
    }
    Ok(())
}

fn inspect_or_repair(
    home: &Path,
    apply: bool,
    include_legacy: bool,
    selected: &BTreeMap<String, String>,
) -> Result<SidebarReport> {
    let path = receipt_path(home);
    let mut report = SidebarReport {
        can_undo: path.is_file(),
        ..Default::default()
    };
    let Some(owner) = local_project_owner(home)? else {
        ensure!(selected.is_empty(), "当前身份已改变，请重新检查。");
        report.detail = "当前身份无法核实为 Hub 本机账户。请在原先使用的 Codex 账号中查看项目；如果原来使用 Hub 本机身份，请切回后重新检查。".into();
        return Ok(report);
    };
    let Some((store, before_state)) = Store::read(home, &owner)? else {
        ensure!(selected.is_empty(), "侧栏项目已改变，请重新检查。");
        report.detail =
            "尚未找到 Codex 的桌面项目设置。请用原账号打开一次 Codex，再完全退出并重新检查。"
                .into();
        return Ok(report);
    };
    let mut state = before_state.clone();
    verify_owner(&state, &owner)?;
    let cards = current_cards(&state)?;
    let mapping: BTreeMap<String, String> = local_map(&state, SERVER_IDS, home)?;
    let host = map_host(&state, home)?;
    let mut rooted: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for (id, card) in &cards {
        for root in card["rootPaths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(key) = super::project_membership::root_match_key(root) {
                rooted.entry(key).or_default().push(id);
            }
        }
    }
    let remote: Vec<Value> = read_key(&state, "remote-projects")?;
    let remote: HashSet<_> = remote.iter().filter_map(|p| p["id"].as_str()).collect();
    let deletions: BTreeMap<String, Value> = local_map(&state, DELETIONS, home)?;
    let visible: Vec<_> = cards
        .iter()
        .filter(|(_, card)| !has_cloud_link(card))
        .map(|(id, _)| mapping.get(id).unwrap_or(id).clone())
        .collect();
    let deleting: HashSet<_> = deletions
        .iter()
        .flat_map(|(k, v)| {
            [
                Some(mapping.get(k).unwrap_or(k).as_str()),
                v["projectId"].as_str(),
            ]
            .into_iter()
            .flatten()
        })
        .collect();
    let explicit: BTreeMap<String, Value> = read_key(&state, MEMBERS)?;
    let hosts: BTreeMap<String, String> = read_key(&state, "thread-project-membership-host-ids")?;
    let markers: Vec<String> = read_key(&state, PROJECTLESS)?;
    let projectless: HashSet<_> = markers.iter().cloned().collect();
    let projectless_count = projectless.len();
    let standalone: HashSet<_> = projectless
        .iter()
        .filter(|id| !explicit.contains_key(*id) && hosts.get(*id).is_none_or(|h| h == "local"))
        .cloned()
        .collect();
    let mut anchors = std::collections::HashMap::new();
    for (thread, entry) in explicit {
        if let Some(id) = entry["projectId"].as_str()
            && entry["projectKind"] == "local"
            && cards.get(id).is_some_and(|card| !has_cloud_link(card))
        {
            anchors.insert(
                thread,
                mapping.get(id).cloned().unwrap_or_else(|| id.to_owned()),
            );
        } else if entry["projectKind"] != "local"
            || entry["projectId"]
                .as_str()
                .and_then(|id| cards.get(id))
                .is_some_and(has_cloud_link)
        {
            anchors.insert(thread, String::new());
        }
    }
    for id in projectless.into_iter().chain(
        hosts
            .into_iter()
            .filter_map(|(id, h)| (h != "local").then_some(id)),
    ) {
        anchors.insert(id, String::new());
    }
    // Inspect both sets read-only, then narrow by desktop membership before
    // presenting the exact opt-in count or passing any updates to SQLite.
    let backup_cards = if cards.is_empty() {
        matching_backup_cards(home, &owner).unwrap_or_default()
    } else {
        HashMap::new()
    };
    let (mut candidates, mut evidence) = super::project_membership::inspect_with_standalone(
        home,
        &visible,
        &anchors,
        true,
        &standalone,
        &backup_cards,
    )?;
    // An existing desktop card can have an unmapped legacy ID. Never synthesize
    // a second one at that root on the assumption that its ID is a native ID.
    let mut ignored = 0usize;
    let mut ids = BTreeMap::new();
    candidates.retain(|p| {
        let mut legacy = mapping
            .iter()
            .filter(|(_, native)| *native == &p.id)
            .map(|(id, _)| id);
        let id = legacy.next().unwrap_or(&p.id);
        let same_root =
            super::project_membership::root_match_key(&p.root).and_then(|key| rooted.get(&key));
        let accept = !deleting.contains(p.id.as_str())
            && !remote.contains(id.as_str())
            && legacy.next().is_none()
            && !cards.get(id).is_some_and(has_cloud_link)
            && mapping.get(id).is_none_or(|native| native == &p.id)
            && same_root.is_none_or(|owners| owners.len() == 1 && owners[0] == id)
            && (!cards.contains_key(id) || same_root.is_some());
        if accept {
            ids.insert(p.id.clone(), id.clone());
        } else {
            ignored += 1
        }
        accept
    });
    let optional_cards = candidates
        .iter()
        .filter(|p| evidence.legacy_cards.contains(&p.id) && !cards.contains_key(&ids[&p.id]))
        .count();
    if !include_legacy {
        candidates.retain(|p| !evidence.legacy_cards.contains(&p.id));
    }
    for candidate in &mut candidates {
        candidate.pending_assignments.retain(|a| {
            !a.previous_project_id
                .as_deref()
                .is_some_and(|id| deleting.contains(id))
        });
    }
    let mut member_changes = desktop_plan(&state, &mut candidates, &mapping, &cards)?;
    let mut selected_count = 0;
    for p in &mut candidates {
        let desktop_id = &ids[&p.id];
        let Some((old, rows)) = evidence.standalone.remove(&p.id) else {
            continue;
        };
        let Some(card) = cards.get(desktop_id) else {
            continue;
        };
        if deleting.contains(old.as_str())
            || card["rootPaths"].as_array().is_none_or(|roots| {
                roots.len() != 1
                    || !roots[0]
                        .as_str()
                        .is_some_and(|root| super::project_membership::same_root(root, &p.root))
            })
        {
            continue;
        }
        let name = card["name"].as_str().unwrap_or(&p.name);
        for (id, title) in rows {
            if selected.get(&id) == Some(&p.id) {
                selected_count += 1;
                p.pending_assignments.push(ThreadAssignment {
                    thread_id: id.clone(),
                    project_id: p.id.clone(),
                    previous_project_id: None,
                    legacy: false,
                });
                member_changes.insert(
                    id.clone(),
                    (None, json!({"projectKind":"local","projectId":desktop_id})),
                );
            }
            report.projectless_choices.push(ProjectlessChoice {
                thread_id: id,
                project_id: p.id.clone(),
                title,
                project_name: name.to_string(),
            });
        }
    }
    ensure!(
        selected_count == selected.len(),
        "所选会话或目标项目已经改变，未执行修复。请重新检查并选择。"
    );
    let eligible = candidates
        .iter()
        .flat_map(|p| p.pending_assignments.iter())
        .filter(|a| a.legacy)
        .count();
    if !include_legacy {
        for p in &mut candidates {
            p.pending_assignments.retain(|a| {
                if a.legacy {
                    member_changes.remove(&a.thread_id);
                }
                !a.legacy
            });
        }
    }
    let assignments: Vec<_> = candidates
        .iter()
        .flat_map(|p| p.pending_assignments.iter().cloned())
        .collect();
    let mut rebuilt = BTreeMap::new();
    let mut map_added = BTreeMap::new();
    let mut order: Vec<String> = read_key(&state, ORDER)?;
    let old_order = state.contains_key(ORDER).then(|| order.clone());
    let mut added_order = false;
    for p in &candidates {
        let id = &ids[&p.id];
        if !cards.contains_key(id) {
            rebuilt.insert(id.clone(),json!({"id":id,"name":p.name,"rootPaths":[p.root],"createdAt":p.created_at_ms,"updatedAt":p.updated_at_ms}));
            if !mapping.contains_key(id) {
                map_added.insert(id.clone(), p.id.clone());
            }
            if !order.contains(id) {
                order.push(id.clone());
                added_order = true;
            }
        }
    }
    report.active_project_count = cards.len();
    report.planned_projects = rebuilt.len();
    report.planned_threads = assignments.len();
    report.can_repair = report.planned_projects != 0 || report.planned_threads != 0;
    let detail_tail = format!(
        "{}{}客户端共标记 {projectless_count} 个“无项目”会话。{}",
        if !include_legacy && eligible != 0 {
            format!("另有 {eligible} 个缺少旧身份记录的会话，可勾选后重新检查。")
        } else {
            String::new()
        },
        if ignored != 0 {
            format!("跳过 {ignored} 个待删除或无法确认的项目。")
        } else {
            String::new()
        },
        if report.projectless_choices.is_empty() {
            "本次保留这些标记。".to_owned()
        } else {
            format!(
                "其中 {} 个与重建项目的目录相同，已选择 {} 个归类，其余保持原状。",
                report.projectless_choices.len(),
                selected_count
            )
        },
    );
    report.detail = format!(
        "当前侧栏有 {} 个项目。可补回 {} 个项目，将 {} 个会话归入对应项目。{}",
        cards.len(),
        rebuilt.len(),
        assignments.len(),
        detail_tail
    );
    if !apply {
        if !include_legacy && optional_cards > 0 {
            report.detail.push_str(&format!("\n有 {optional_cards} 个项目已有原分组和匹配目录，但旧会话未记录账号。确认这些旧会话属于自己后，勾选上方旧会话选项并重新检查，可补回项目卡片。"));
        }
        if let Some(reason) = evidence.unavailable {
            report.detail.push_str(&format!("\n{reason}"));
        }
        if evidence.conflicting_projects > 0 {
            report.detail.push_str(&format!("\n{} 个项目包含其他账号、身份不完整或来源无法核实的记录。请在原账号的 Codex 中打开这些项目。", evidence.conflicting_projects));
        }
        if evidence.backed_projects > 0 {
            report.detail.push_str(&format!(
                "\n{} 条项目目录记录带有 ChatGPT 云端账号关联。请在原 ChatGPT 账号中查看对应项目。",
                evidence.backed_projects
            ));
        }
        if evidence.invalid_projects > 0
            || evidence.ambiguous_roots > 0
            || evidence.mismatched_grouped_paths > 0
        {
            report.detail.push_str(&format!("\n未能核对的记录：项目字段或本机目录不符合要求 {} 个，同目录目标不唯一 {} 条，已分组会话的目录不匹配 {} 条。可在 Codex 打开原项目目录并手动将确认属于它的会话移入，再退出后检查。", evidence.invalid_projects, evidence.ambiguous_roots, evidence.mismatched_grouped_paths));
        }
        if evidence.unverified_projects > 0 && !report.can_repair {
            report.detail.push_str(&format!("\n{} 个项目缺少对应的本机用户会话证据。请先用原账号在 Codex 核对项目和“分项目显示”；若使用过另一模型来源，可到 Hub「Codex 会话管理」核对。", evidence.unverified_projects));
        }
    }
    if !apply || !report.can_repair {
        return Ok(report);
    }
    let previous = match fs::read_to_string(&path) {
        Ok(raw) => Some(RawValue::from_string(raw)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let cards_before = state.get(CARDS).cloned();
    let members_present = state.contains_key(MEMBERS);
    let mut removed_markers = Vec::new();
    if !selected.is_empty() {
        let mut retained = Vec::new();
        for (idx, id) in markers.into_iter().enumerate() {
            if selected.contains_key(&id) {
                removed_markers.push((idx, id));
            } else {
                retained.push(id)
            }
        }
        state.insert(
            PROJECTLESS.into(),
            serde_json::value::to_raw_value(&retained)?,
        );
    }
    let order_change = added_order.then_some((old_order, order));
    if let Some((_, after)) = &order_change {
        state.insert(ORDER.into(), serde_json::value::to_raw_value(after)?);
    }
    let mut map_change = None;
    if !map_added.is_empty() {
        let hosts: BTreeMap<String, Box<RawValue>> = read_key(&state, SERVER_IDS)?;
        map_change = Some(MapChange {
            had_map: state.contains_key(SERVER_IDS),
            had_host: hosts.contains_key(&host),
            host,
            added: map_added,
        });
        edit_map(&mut state, map_change.as_ref().unwrap(), false)?;
    }
    edit_members(&mut state, &member_changes, false, members_present)?;
    let desktop_changed = !rebuilt.is_empty() || !member_changes.is_empty();
    if !rebuilt.is_empty() {
        let mut all = cards;
        all.extend(rebuilt.clone());
        state.insert(CARDS.into(), serde_json::value::to_raw_value(&all)?);
    }
    let receipt = Receipt {
        version: if store == Store::Json { 5 } else { 6 },
        store,
        owner,
        db_dir: crate::codex_project_status::sqlite_dir(home)?.canonicalize()?,
        cards_before,
        cards_after: state
            .get(CARDS)
            .map(|v| serde_json::from_str(v.get()))
            .transpose()?,
        cards_added: rebuilt.len(),
        assignments,
        previous,
        members_present,
        member_changes,
        map_change,
        order_change,
        removed_markers,
    };
    fs::create_dir_all(sidebar_receipt_dir(home))?;
    write_file_atomically(&path, &serde_json::to_vec(&receipt)?)?;
    if desktop_changed {
        if let Err(e) = store.replace(home, &receipt.owner, &before_state, &state) {
            store_previous(&path, receipt.previous.as_deref())?;
            return Err(e);
        }
    }
    if let Err(e) = repair_thread_memberships(home, &candidates) {
        if desktop_changed {
            store.replace(home, &receipt.owner, &state, &before_state)?;
        }
        store_previous(&path, receipt.previous.as_deref())?;
        return Err(e);
    }
    report.active_project_count += rebuilt.len();
    report.changed_count = rebuilt.len();
    report.grouped_thread_count = receipt.assignments.len();
    report.can_repair = false;
    report.can_undo = true;
    report.projectless_choices.clear();
    report.detail = format!(
        "已补回 {} 个项目，当前侧栏共 {} 个项目；{} 个会话已归入对应项目。{}",
        report.changed_count, report.active_project_count, report.grouped_thread_count, detail_tail
    );
    #[cfg(not(test))]
    crate::chain_log::write_line(format!(
        "[project_sidebar] action=apply cards={} conversations={}",
        report.changed_count, report.grouped_thread_count
    ));
    Ok(report)
}

#[derive(Clone, Debug)]
pub(crate) struct SidebarUndoPreview {
    fingerprint: [u8; 32],
    pub(crate) cards: usize,
    pub(crate) assignments: usize,
}

fn receipt_for_undo(home: &Path) -> Result<(Receipt, SidebarUndoPreview)> {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(receipt_path(home)).context("项目修复记录已不存在，请刷新备份列表。")?;
    let receipt: Receipt = serde_json::from_slice(&bytes)?;
    ensure!(
        matches!(receipt.version, 3..=6)
            && Some(&receipt.owner) == local_project_owner(home)?.as_ref()
            && receipt.db_dir == crate::codex_project_status::sqlite_dir(home)?.canonicalize()?,
        "恢复记录不属于当前账户或历史库。"
    );
    let summary = SidebarUndoPreview {
        // Shared receipt.json always points to the newest batch, including v3-v5
        // backups without a batch ID. Confirm the receipt, not just its directory.
        fingerprint: Sha256::digest(&bytes).into(),
        cards: receipt.cards_added,
        assignments: receipt.assignments.len(),
    };
    Ok((receipt, summary))
}

pub(crate) fn preview_sidebar_undo(home: &Path) -> Result<SidebarUndoPreview> {
    let (receipt, summary) = receipt_for_undo(home)?;
    let (store, state) =
        Store::read(home, &receipt.owner)?.context("当前侧栏项目设置已不存在。")?;
    ensure!(
        store == receipt.store,
        "侧栏存储位置已变化，未撤销。请检查当前项目。"
    );
    verify_owner(&state, &receipt.owner)?;
    Ok(summary)
}

pub(crate) fn apply_sidebar_undo(
    home: &Path,
    confirmed: &SidebarUndoPreview,
) -> Result<SidebarReport> {
    let path = receipt_path(home);
    let (receipt, current_batch) = receipt_for_undo(home)?;
    ensure!(
        current_batch.fingerprint == confirmed.fingerprint,
        "项目修复记录已变化，未撤销。请重新查看要撤销的批次。"
    );
    let (store, current) =
        Store::read(home, &receipt.owner)?.context("当前侧栏项目设置已不存在。")?;
    ensure!(
        store == receipt.store,
        "侧栏存储位置已变化，未撤销。请检查当前项目。"
    );
    let mut state = current.clone();
    verify_owner(&state, &receipt.owner)?;
    let current_cards: Option<Value> = state
        .get(CARDS)
        .map(|v| serde_json::from_str(v.get()))
        .transpose()?;
    ensure!(
        current_cards == receipt.cards_after,
        "项目清单已被修改，未撤销本次修复。"
    );
    if let Some((before, after)) = receipt.order_change {
        ensure!(
            read_key::<Vec<String>>(&state, ORDER)? == after,
            "项目顺序已被修改，未执行撤销。"
        );
        match before {
            Some(list) => {
                state.insert(ORDER.into(), serde_json::value::to_raw_value(&list)?);
            }
            None => {
                state.remove(ORDER);
            }
        }
    }
    if let Some(change) = receipt.map_change {
        edit_map(&mut state, &change, true)?;
    }
    edit_members(
        &mut state,
        &receipt.member_changes,
        true,
        receipt.members_present,
    )?;
    if !receipt.removed_markers.is_empty() {
        // Members above verifies the selected chats still have the assigned
        // destination and were not marked standalone again after the repair.
        let mut markers: Vec<String> = read_key(&state, PROJECTLESS)?;
        for (idx, id) in receipt.removed_markers {
            markers.insert(idx.min(markers.len()), id);
        }
        state.insert(
            PROJECTLESS.into(),
            serde_json::value::to_raw_value(&markers)?,
        );
    }
    let desktop_changed = receipt.cards_added != 0 || !receipt.member_changes.is_empty();
    if receipt.cards_added != 0 {
        if let Some(old) = receipt.cards_before {
            state.insert(CARDS.into(), old);
        } else {
            state.remove(CARDS);
        }
    }
    if desktop_changed {
        store.replace(home, &receipt.owner, &current, &state)?;
    }
    if let Err(e) = undo_thread_memberships(home, &receipt.assignments) {
        if desktop_changed {
            store.replace(home, &receipt.owner, &state, &current)?;
        }
        return Err(e);
    }
    store_previous(&path, receipt.previous.as_deref())?;
    #[cfg(not(test))]
    crate::chain_log::write_line(format!(
        "[project_sidebar] action=undo cards={} conversations={}",
        receipt.cards_added,
        receipt.assignments.len()
    ));
    Ok(SidebarReport {
        changed_count: receipt.cards_added,
        grouped_thread_count: receipt.assignments.len(),
        can_undo: receipt.previous.is_some(),
        detail: format!(
            "已撤销 {} 个项目和 {} 个会话的本次修复。",
            receipt.cards_added,
            receipt.assignments.len()
        ),
        ..Default::default()
    })
}
pub(crate) fn sidebar_projects(home: &Path, action: SidebarAction) -> Result<SidebarReport> {
    sidebar_projects_with_options(home, action, false)
}
pub(crate) fn sidebar_projects_with_options(
    home: &Path,
    action: SidebarAction,
    include_legacy: bool,
) -> Result<SidebarReport> {
    sidebar_projects_with_selection(home, action, include_legacy, &BTreeMap::new())
}

pub(crate) fn sidebar_projects_with_selection(
    home: &Path,
    action: SidebarAction,
    include_legacy: bool,
    selected: &BTreeMap<String, String>,
) -> Result<SidebarReport> {
    match action {
        SidebarAction::Check => inspect_or_repair(home, false, include_legacy, selected),
        SidebarAction::Repair => inspect_or_repair(home, true, include_legacy, selected),
        #[cfg(test)]
        SidebarAction::Undo => apply_sidebar_undo(home, &preview_sidebar_undo(home)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_only_active_globals_and_rejects_switched_accounts() {
        let s:State=serde_json::from_value(json!({
            "electron-account-renderer-state-v1":{"owner":"current","inactive":{"other":{"globals":{"local-projects":{"foreign":{}}}}}},
            "electron-persisted-atom-state":{"local-projects":{"wrong":{}}},
            "local-projects":{"active":{}}
        })).unwrap();
        verify_owner(&s, "current").unwrap();
        assert!(verify_owner(&s, "other").is_err());
        assert_eq!(
            current_cards(&s).unwrap().into_keys().collect::<Vec<_>>(),
            ["active"]
        );
    }
    #[test]
    fn resolves_only_the_local_home_project_mapping() {
        let t = tempfile::tempdir().unwrap();
        let key = format!("local:{}", t.path().display());
        let s:State=serde_json::from_value(json!({(SERVER_IDS):{(key):{"legacy":"native"},"remote:elsewhere":{"legacy":"foreign"}}})).unwrap();
        assert_eq!(
            local_map::<String>(&s, SERVER_IDS, t.path()).unwrap()["legacy"],
            "native"
        );
    }
    #[test]
    fn rebuild_after_two_desktop_moves_survives_recheck_and_undo() {
        use rusqlite::{Connection, params};
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let root = home.join("example").display().to_string();
        let old = "00000000-0000-4000-8000-000000000001";
        let new = "00000000-0000-4000-8000-000000000002";
        let user = "user_codexhub_local";
        let acct = "acct_codexhub_local";
        let claims = json!({"sub":"local|user_codexhub_local","https://api.openai.com/auth":{"localhost":true,"chatgpt_user_id":user,"chatgpt_account_id":acct}});
        let token = format!("e30.{}.sig", super::super::b64url_json(&claims).unwrap());
        fs::write(home.join("auth.json"),json!({"auth_mode":"chatgptAuthTokens","tokens":{"account_id":acct,"id_token":token,"access_token":token}}).to_string()).unwrap();
        let mut config = toml_edit::DocumentMut::new();
        config["model_provider"] = toml_edit::value("ai-gateway");
        config["sqlite_home"] = toml_edit::value(home.to_str().unwrap());
        fs::write(home.join("config.toml"), config.to_string()).unwrap();
        let db = Connection::open(home.join("state_5.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY,name TEXT,metadata TEXT,position INTEGER,created_at_ms INTEGER,updated_at_ms INTEGER);
            CREATE TABLE project_roots(project_id TEXT,position INTEGER,path TEXT);
            CREATE TABLE threads(id TEXT PRIMARY KEY,creator_user_id TEXT,creator_account_id TEXT,source TEXT,thread_source TEXT,archived INTEGER,attached_thread_parent_id TEXT,thread_section_id TEXT,model_provider TEXT,cwd TEXT,project_id TEXT);").unwrap();
        for (id, stamp) in [(old, 1), (new, 2)] {
            db.execute(
                "INSERT INTO projects VALUES (?1,'example','{}',0,?2,?2)",
                params![id, stamp],
            )
            .unwrap();
            db.execute(
                "INSERT INTO project_roots VALUES (?1,0,?2)",
                params![id, root],
            )
            .unwrap();
        }
        for (id, prior) in [
            ("one", Some(new)),
            ("two", None),
            ("missing", None),
            ("stale", Some(old)),
            ("stale-desktop", None),
            ("excluded", None),
        ] {
            db.execute("INSERT INTO threads VALUES (?1,?2,?3,'vscode','user',0,NULL,NULL,'ai-gateway',?4,?5)",params![id,user,acct,root,prior]).unwrap();
        }
        let mut native_ids = Vec::new();
        for n in 3..=8 {
            let id = format!("00000000-0000-4000-8000-{n:012}");
            let path = home.join(format!("proj{n}")).display().to_string();
            db.execute(
                "INSERT INTO projects VALUES (?1,'同名项目','{}',?2,1,1)",
                params![id, n],
            )
            .unwrap();
            db.execute(
                "INSERT INTO project_roots VALUES (?1,0,?2)",
                params![id, path],
            )
            .unwrap();
            db.execute("INSERT INTO threads VALUES (?1,?2,?3,'vscode','user',0,NULL,NULL,'ai-gateway',?4,?5)",params![format!("owned-{n}"),user,acct,path,id]).unwrap();
            native_ids.push(id);
        }
        db.execute("INSERT INTO threads VALUES ('legacy-no-owner',NULL,NULL,'vscode','user',0,NULL,NULL,'ai-gateway',?1,NULL)",[&root]).unwrap();
        db.execute("INSERT INTO threads VALUES ('legacy-independent',NULL,NULL,'vscode','user',0,NULL,NULL,'ai-gateway',?1,NULL)",[&root]).unwrap();
        let entry = json!({"projectKind":"local","projectId":"my-card"});
        let host = format!("local:{}", home.display());
        let state = json!({(CARDS):{"my-card":{"id":"my-card","name":"重建项目","rootPaths":[root]}},
            (ORDER):["my-card"], "remote-projects":[{"hostId":"remoteHost","id":native_ids[5]}],
            (DELETIONS):{(&host):{"pending":{"projectId":native_ids[4]}}},
            (MIGRATION):{(&host):{"projectsMigrated":true,"version":1}},
            "project-appearances":{"my-card":{"icon":"original"}},
            (MEMBERS):{"one":entry,"two":entry,"stale":{"projectKind":"local","projectId":"old-card"},"stale-desktop":{"projectKind":"local","projectId":"old-card"}},
            "projectless-thread-ids":["excluded","legacy-independent"],(SERVER_IDS):{(&host):{"my-card":new,"old-card":old,"saved-id":native_ids[0]},"remote:test":{"other":"untouched"}},
            (ACCOUNT_SCOPE):{"owner":local_project_owner(home).unwrap().unwrap(),"inactive":{"other":{"globals":{"local-projects":{"foreign":{}}},"atoms":{}}}}});
        fs::write(home.join(STATE_FILE), state.to_string()).unwrap();
        let planned = sidebar_projects(home, SidebarAction::Check).unwrap();
        assert!(
            planned.can_repair
                && planned.detail.contains("可补回 4 个项目")
                && planned.detail.contains("另有 1 个")
                && planned.detail.contains("客户端共标记 2 个“无项目”会话")
        );
        let result = sidebar_projects(home, SidebarAction::Repair).unwrap();
        assert_eq!(
            (
                result.grouped_thread_count,
                result.changed_count,
                result.active_project_count
            ),
            (3, 4, 5)
        );
        let after: Value =
            serde_json::from_slice(&fs::read(home.join(STATE_FILE)).unwrap()).unwrap();
        assert_eq!(after[MEMBERS]["stale"], entry);
        assert_eq!(after[MEMBERS]["stale-desktop"], entry);
        assert_eq!(after[MEMBERS]["missing"], entry);
        assert_eq!(after[ACCOUNT_SCOPE], state[ACCOUNT_SCOPE]);
        assert_eq!(after[CARDS]["my-card"], state[CARDS]["my-card"]);
        assert_eq!(
            after[ORDER],
            json!([
                "my-card",
                "saved-id",
                native_ids[1],
                native_ids[2],
                native_ids[3]
            ])
        );
        for id in after[ORDER]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
        {
            // Migration first indexes native projects by their stored legacy
            // ID, then imports only cards missing from that index.
            let native = after[SERVER_IDS][&host][id].as_str().unwrap();
            assert!(
                db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
                    [native],
                    |r| r.get::<_, bool>(0)
                )
                .unwrap()
            );
        }
        assert_eq!(after[MIGRATION], state[MIGRATION]);
        assert_eq!(after["project-appearances"], state["project-appearances"]);
        assert_eq!(
            after[SERVER_IDS]["remote:test"],
            state[SERVER_IDS]["remote:test"]
        );
        assert!(
            !sidebar_projects(home, SidebarAction::Check)
                .unwrap()
                .can_repair
        );
        // A = rebuild missing project cards. B = explicitly return one independent
        // conversation to the recreated project. They share a receipt path, not
        // an undo target: a confirmation for A may never operate on B or vice versa.
        let batch_a = preview_sidebar_undo(home).unwrap();
        assert_eq!((batch_a.cards, batch_a.assignments), (4, 3));
        let selected = BTreeMap::from([("excluded".to_owned(), new.to_owned())]);
        let applied =
            sidebar_projects_with_selection(home, SidebarAction::Repair, false, &selected).unwrap();
        assert_eq!(
            (applied.changed_count, applied.grouped_thread_count),
            (0, 1)
        );
        let after_b: Value =
            serde_json::from_slice(&fs::read(home.join(STATE_FILE)).unwrap()).unwrap();
        assert_eq!(after_b[CARDS], after[CARDS]);
        for (id, placement) in after[MEMBERS].as_object().unwrap() {
            assert_eq!(&after_b[MEMBERS][id], placement);
        }
        assert_eq!(after_b[MEMBERS]["excluded"], entry);
        assert!(apply_sidebar_undo(home, &batch_a).is_err());
        let batch_b = preview_sidebar_undo(home).unwrap();
        assert_eq!((batch_b.cards, batch_b.assignments), (0, 1));
        assert!(apply_sidebar_undo(home, &batch_b).unwrap().can_undo);
        // Double submission after B was popped must not undo A.
        assert!(apply_sidebar_undo(home, &batch_b).is_err());
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(home.join(STATE_FILE)).unwrap()).unwrap(),
            after
        );
        assert!(
            db.query_row(
                "SELECT project_id IS NULL FROM threads WHERE id='excluded'",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
        );
        assert_eq!(
            db.query_row("SELECT project_id FROM threads WHERE id='stale'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            new
        );
        let extra = sidebar_projects_with_options(home, SidebarAction::Repair, true).unwrap();
        assert_eq!((extra.changed_count, extra.grouped_thread_count), (0, 1));
        assert!(
            !sidebar_projects_with_options(home, SidebarAction::Check, true)
                .unwrap()
                .can_repair
        );
        let undo_extra = sidebar_projects(home, SidebarAction::Undo).unwrap();
        assert!(undo_extra.can_undo);
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(home.join(STATE_FILE)).unwrap()).unwrap(),
            after
        );
        let mut changed = after.clone();
        changed[MEMBERS]["missing"] = json!({"projectKind":"local","projectId":"elsewhere"});
        fs::write(home.join(STATE_FILE), changed.to_string()).unwrap();
        assert!(sidebar_projects(home, SidebarAction::Undo).is_err());
        fs::write(home.join(STATE_FILE), after.to_string()).unwrap();
        sidebar_projects(home, SidebarAction::Undo).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(home.join(STATE_FILE)).unwrap()).unwrap(),
            state
        );
        assert_eq!(
            db.query_row("SELECT project_id FROM threads WHERE id='stale'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            old
        );
        assert!(!receipt_path(home).exists());
    }
}
