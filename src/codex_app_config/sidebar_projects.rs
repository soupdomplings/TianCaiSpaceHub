//! Recover the active desktop's project cards and the native conversation memberships.
use super::{
    LocalProjectCandidate, ThreadAssignment, local_project_owner, repair_thread_memberships,
    undo_thread_memberships, write_file_atomically,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json, value::RawValue};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

const STATE_FILE: &str = ".codex-global-state.json";
const ACCOUNT_SCOPE: &str = "electron-account-renderer-state-v1";
const CARDS: &str = "local-projects";
const MEMBERS: &str = "thread-project-assignments";
const SERVER_IDS: &str = "app-server-project-id-by-legacy-project-id-by-host";
const DELETIONS: &str = "app-server-pending-project-deletions-by-host";
type State = BTreeMap<String, Box<RawValue>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarAction {
    Check,
    Repair,
    Undo,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SidebarReport {
    pub(crate) active_project_count: usize,
    pub(crate) changed_count: usize,
    pub(crate) grouped_thread_count: usize,
    pub(crate) can_repair: bool,
    pub(crate) can_undo: bool,
    pub(crate) skipped: bool,
    pub(crate) detail: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    version: u32,
    owner: String,
    db_dir: PathBuf,
    cards_before: Option<Box<RawValue>>,
    cards_after: Option<Value>,
    cards_added: usize,
    assignments: Vec<ThreadAssignment>,
    members_present: bool,
    member_changes: BTreeMap<String, (Option<Value>, Value)>,
    previous: Option<Box<RawValue>>,
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
fn verify_owner(state: &State, owner: &str) -> Result<()> {
    // Electron stores only the active globals at the top level. Inactive slots
    // are saved separately and must never be read as an alternative source.
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
    let current_home = home.canonicalize()?;
    let mut current = None;
    for (host, section) in all {
        let Some(path) = host.strip_prefix("local:") else {
            continue;
        };
        let path = Path::new(path);
        if path.is_absolute()
            && (path == home || path.canonicalize().is_ok_and(|p| p == current_home))
        {
            ensure!(current.is_none(), "本机项目身份记录不唯一。");
            current = Some(serde_json::from_str(section.get())?);
        }
    }
    Ok(current.unwrap_or_default())
}
fn write_state(home: &Path, state: &State) -> Result<()> {
    write_file_atomically(&home.join(STATE_FILE), &serde_json::to_vec(state)?)
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

fn inspect_or_repair(home: &Path, apply: bool) -> Result<SidebarReport> {
    let path = receipt_path(home);
    let mut report = SidebarReport {
        can_undo: path.is_file(),
        ..Default::default()
    };
    let state_bytes = match fs::read(home.join(STATE_FILE)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            report.detail = "尚未找到 Codex 的桌面项目设置。".into();
            return Ok(report);
        }
        Err(e) => return Err(e.into()),
    };
    let mut state: State = serde_json::from_slice(&state_bytes)?;
    let Some(owner) = local_project_owner(home)? else {
        report.detail = "当前不是 Hub 本机账户，未处理项目。".into();
        return Ok(report);
    };
    verify_owner(&state, &owner)?;
    let cards = current_cards(&state)?;
    let mapping: BTreeMap<String, String> = local_map(&state, SERVER_IDS, home)?;
    let deletions: BTreeMap<String, Value> = local_map(&state, DELETIONS, home)?;
    let visible: Vec<_> = cards
        .keys()
        .map(|id| mapping.get(id).unwrap_or(id).clone())
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
    let projectless: HashSet<String> = read_key(&state, "projectless-thread-ids")?;
    let mut anchors = std::collections::HashMap::new();
    for (thread, entry) in explicit {
        if let Some(id) = entry["projectId"].as_str()
            && entry["projectKind"] == "local"
            && cards.contains_key(id)
        {
            anchors.insert(
                thread,
                mapping.get(id).cloned().unwrap_or_else(|| id.to_owned()),
            );
        }
    }
    for id in projectless.into_iter().chain(
        hosts
            .into_iter()
            .filter_map(|(id, h)| (h != "local").then_some(id)),
    ) {
        anchors.insert(id, String::new());
    }
    let mut candidates =
        super::project_membership::local_project_candidates_with_anchors(home, &visible, &anchors)?;
    candidates.retain(|p| {
        !deleting.contains(p.id.as_str()) && (cards.is_empty() || visible.contains(&p.id))
    });
    let member_changes = desktop_plan(&state, &mut candidates, &mapping, &cards)?;
    let assignments: Vec<_> = candidates
        .iter()
        .flat_map(|p| p.pending_assignments.iter().cloned())
        .collect();
    let mut rebuilt = BTreeMap::new();
    if cards.is_empty() {
        for p in &candidates {
            let mut legacy = mapping
                .iter()
                .filter(|(_, native)| *native == &p.id)
                .map(|(id, _)| id);
            let id = legacy.next().unwrap_or(&p.id);
            ensure!(
                legacy.next().is_none() && !rebuilt.contains_key(id),
                "相同目录存在多个项目标识，未更改归属。"
            );
            rebuilt.insert(id.clone(),json!({"id":id,"name":p.name,"rootPaths":[p.root],"createdAt":p.created_at_ms,"updatedAt":p.updated_at_ms}));
        }
    }
    report.active_project_count = cards.len();
    report.can_repair = !rebuilt.is_empty() || !assignments.is_empty();
    report.detail = if report.can_repair {
        format!(
            "可补回 {} 个项目、将 {} 个会话归入对应项目。",
            rebuilt.len(),
            assignments.len()
        )
    } else {
        "没有找到需要补回且能确定归属的项目或会话。".into()
    };
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
    edit_members(&mut state, &member_changes, false, members_present)?;
    let desktop_changed = !rebuilt.is_empty() || !member_changes.is_empty();
    if !rebuilt.is_empty() {
        state.insert(CARDS.into(), serde_json::value::to_raw_value(&rebuilt)?);
    }
    let receipt = Receipt {
        version: 3,
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
    };
    fs::create_dir_all(sidebar_receipt_dir(home))?;
    write_file_atomically(&path, &serde_json::to_vec(&receipt)?)?;
    if desktop_changed {
        if let Err(e) = write_state(home, &state) {
            store_previous(&path, receipt.previous.as_deref())?;
            return Err(e);
        }
    }
    if let Err(e) = repair_thread_memberships(home, &candidates) {
        if desktop_changed {
            write_file_atomically(&home.join(STATE_FILE), &state_bytes)?;
        }
        store_previous(&path, receipt.previous.as_deref())?;
        return Err(e);
    }
    report.active_project_count += rebuilt.len();
    report.changed_count = rebuilt.len();
    report.grouped_thread_count = receipt.assignments.len();
    report.can_repair = false;
    report.can_undo = true;
    report.detail = format!(
        "已补回 {} 个项目，{} 个会话已归入对应项目。",
        report.changed_count, report.grouped_thread_count
    );
    Ok(report)
}
fn undo(home: &Path) -> Result<SidebarReport> {
    let path = receipt_path(home);
    let receipt: Receipt = serde_json::from_slice(&fs::read(&path)?)?;
    ensure!(
        receipt.version == 3
            && Some(&receipt.owner) == local_project_owner(home)?.as_ref()
            && receipt.db_dir == crate::codex_project_status::sqlite_dir(home)?.canonicalize()?,
        "恢复记录不属于当前账户或历史库。"
    );
    let current = fs::read(home.join(STATE_FILE))?;
    let mut state: State = serde_json::from_slice(&current)?;
    verify_owner(&state, &receipt.owner)?;
    let current_cards: Option<Value> = state
        .get(CARDS)
        .map(|v| serde_json::from_str(v.get()))
        .transpose()?;
    ensure!(
        current_cards == receipt.cards_after,
        "项目清单已被修改，未撤销本次修复。"
    );
    edit_members(
        &mut state,
        &receipt.member_changes,
        true,
        receipt.members_present,
    )?;
    let desktop_changed = receipt.cards_added != 0 || !receipt.member_changes.is_empty();
    if receipt.cards_added != 0 {
        if let Some(old) = receipt.cards_before {
            state.insert(CARDS.into(), old);
        } else {
            state.remove(CARDS);
        }
    }
    if desktop_changed {
        write_state(home, &state)?;
    }
    if let Err(e) = undo_thread_memberships(home, &receipt.assignments) {
        if desktop_changed {
            write_file_atomically(&home.join(STATE_FILE), &current)?;
        }
        return Err(e);
    }
    store_previous(&path, receipt.previous.as_deref())?;
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
    match action {
        SidebarAction::Check => inspect_or_repair(home, false),
        SidebarAction::Repair => inspect_or_repair(home, true),
        SidebarAction::Undo => undo(home),
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
        let entry = json!({"projectKind":"local","projectId":"my-card"});
        let state = json!({(CARDS):{"my-card":{"id":"my-card","name":"重建项目","rootPaths":[root]}},
            (MEMBERS):{"one":entry,"two":entry,"stale":{"projectKind":"local","projectId":"old-card"},"stale-desktop":{"projectKind":"local","projectId":"old-card"}},
            "projectless-thread-ids":["excluded"],(SERVER_IDS):{(format!("local:{}",home.display())):{"my-card":new,"old-card":old}},
            (ACCOUNT_SCOPE):{"owner":local_project_owner(home).unwrap().unwrap(),"inactive":{"other":{"globals":{"local-projects":{"foreign":{}}},"atoms":{}}}}});
        fs::write(home.join(STATE_FILE), state.to_string()).unwrap();
        assert!(
            sidebar_projects(home, SidebarAction::Check)
                .unwrap()
                .can_repair
        );
        let result = sidebar_projects(home, SidebarAction::Repair).unwrap();
        assert_eq!((result.grouped_thread_count, result.changed_count), (3, 0));
        let after: Value =
            serde_json::from_slice(&fs::read(home.join(STATE_FILE)).unwrap()).unwrap();
        assert_eq!(after[MEMBERS]["stale"], entry);
        assert_eq!(after[MEMBERS]["stale-desktop"], entry);
        assert_eq!(after[MEMBERS]["missing"], entry);
        assert_eq!(after[ACCOUNT_SCOPE], state[ACCOUNT_SCOPE]);
        assert!(
            !sidebar_projects(home, SidebarAction::Check)
                .unwrap()
                .can_repair
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
