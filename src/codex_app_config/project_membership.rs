//! Evidence and narrowly scoped repair for missing local desktop project cards.
//! Any database change requires the verified Hub identity and exact project root.
use super::{AI_GATEWAY_PROVIDER_NAME, LOCAL_AUTH_MODE, decode_jwt_payload_value};
use crate::codex_project_status::{database_settings, with_database};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::ErrorKind,
    path::Path,
};

const HUB_USER: &str = "user_codexhub_local";
const HUB_ACCOUNT: &str = "acct_codexhub_local";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocalProjectCandidate {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) root: String,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) predecessor_id: Option<String>,
    pub(crate) pending_assignments: Vec<ThreadAssignment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadAssignment {
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) previous_project_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) legacy: bool,
}

#[derive(Default, Debug)]
pub(crate) struct ProjectEvidence {
    pub(super) unavailable: Option<&'static str>,
    pub(super) invalid_projects: usize,
    pub(super) backed_projects: usize,
    pub(super) conflicting_projects: usize,
    pub(super) ambiguous_roots: usize,
    pub(super) unverified_projects: usize,
    pub(super) mismatched_grouped_paths: usize,
    /// An opt-in permits restoring only the card for an existing native link.
    pub(super) legacy_cards: HashSet<String>,
    pub(crate) eligible_legacy_count: usize,
    pub(crate) eligible_legacy_by_project: HashMap<String, usize>,
    /// Native target -> (older sibling, standalone thread IDs and display titles).
    pub(crate) standalone: HashMap<String, (String, Vec<(String, String)>)>,
}

/// Compare desktop ownership only for the actual active, Hub-managed identity.
/// Never infer the active account from a database row or a backup credential.
pub(crate) fn local_project_owner(home: &Path) -> Result<Option<String>> {
    let raw = match fs::read(home.join("auth.json")) {
        Ok(raw) => raw,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context("无法核实当前 Codex 本地身份。"),
    };
    let auth: Value = serde_json::from_slice(&raw).context("当前 Codex 本地身份文件无法解析。")?;
    if auth["auth_mode"].as_str() != Some(LOCAL_AUTH_MODE)
        || !auth["OPENAI_API_KEY"].is_null()
        || auth["tokens"]["account_id"].as_str() != Some(HUB_ACCOUNT)
    {
        return Ok(None);
    }
    for key in ["id_token", "access_token"] {
        let Some(payload) = auth["tokens"][key]
            .as_str()
            .and_then(decode_jwt_payload_value)
        else {
            return Ok(None);
        };
        let claims = &payload["https://api.openai.com/auth"];
        if payload["sub"].as_str() != Some("local|user_codexhub_local")
            || claims["localhost"].as_bool() != Some(true)
            || claims["chatgpt_account_id"].as_str() != Some(HUB_ACCOUNT)
            || claims["chatgpt_user_id"].as_str() != Some(HUB_USER)
        {
            return Ok(None);
        }
    }
    let pair = serde_json::to_vec(&[HUB_USER, HUB_ACCOUNT])?;
    Ok(Some(hex::encode(Sha256::digest(pair))))
}

pub(crate) fn local_project_candidates(home: &Path) -> Result<Vec<LocalProjectCandidate>> {
    local_project_candidates_for_ids(home, &[])
}

/// Read project candidates using the project IDs currently shown by the
/// desktop sidebar as a disambiguation hint. A manually recreated project can
/// coexist with its old database row while retaining the same exact root.
pub(crate) fn local_project_candidates_for_ids(
    home: &Path,
    preferred_project_ids: &[String],
) -> Result<Vec<LocalProjectCandidate>> {
    local_project_candidates_with_anchors(home, preferred_project_ids, &HashMap::new())
}

/// Desktop-only manual placements take precedence over the SQLite projection.
/// Only a scoped, unarchived native thread at the same root can provide evidence.
pub(crate) fn local_project_candidates_with_anchors(
    home: &Path,
    preferred_project_ids: &[String],
    manual: &HashMap<String, String>,
) -> Result<Vec<LocalProjectCandidate>> {
    Ok(inspect_project_candidates(home, preferred_project_ids, manual, false)?.0)
}

pub(crate) fn inspect_project_candidates(
    home: &Path,
    preferred_project_ids: &[String],
    manual: &HashMap<String, String>,
    include_legacy: bool,
) -> Result<(Vec<LocalProjectCandidate>, ProjectEvidence)> {
    inspect_with_standalone(
        home,
        preferred_project_ids,
        manual,
        include_legacy,
        &HashSet::new(),
        &HashMap::new(),
    )
}

pub(crate) fn inspect_with_standalone(
    home: &Path,
    preferred_project_ids: &[String],
    manual: &HashMap<String, String>,
    include_legacy: bool,
    standalone: &HashSet<String>,
    backup_cards: &HashMap<String, String>,
) -> Result<(Vec<LocalProjectCandidate>, ProjectEvidence)> {
    if local_project_owner(home)?.is_none() {
        return Ok(Default::default());
    }
    let (dir, provider) = database_settings(home)?;
    let unavailable = if provider.as_deref() != Some(AI_GATEWAY_PROVIDER_NAME) {
        Some("当前 Codex 配置未选用可核实的 Hub 本机模型来源。请核对原账号和使用的配置，再检查。")
    } else if !dir.join(super::CODEX_APP_STATE_DB).try_exists()? {
        Some("未找到当前配置指向的原生历史库。请从原配置备份核对历史库位置，再检查。")
    } else {
        None
    };
    if unavailable.is_some() {
        return Ok((
            Vec::new(),
            ProjectEvidence {
                unavailable,
                ..Default::default()
            },
        ));
    }
    let preferred = preferred_project_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    with_database(&dir, |tx| {
        read_candidates(
            tx,
            &preferred,
            manual,
            include_legacy,
            standalone,
            backup_cards,
        )
    })
    .context("无法核实本机原项目和会话来源。")
}

/// Both directions use a compare-and-set in one transaction. A later manual
/// move, archive, or identity change prevents the whole batch from applying.
pub(crate) fn repair_thread_memberships(
    home: &Path,
    candidates: &[LocalProjectCandidate],
) -> Result<Vec<ThreadAssignment>> {
    let assignments: Vec<_> = candidates
        .iter()
        .flat_map(|p| p.pending_assignments.iter().cloned())
        .collect();
    if assignments.is_empty() {
        return Ok(assignments);
    }
    let mut db = open_for_repair(home)?;
    let tx = db.transaction()?;
    {
        let mut update = tx.prepare(&format!(
            "UPDATE threads SET project_id=?4 WHERE id=?5 AND project_id IS ?6 AND {BASE_SCOPE} AND {WRITE_OWNER} RETURNING cwd"
        ))?;
        for project in candidates {
            for assignment in &project.pending_assignments {
                let cwd: String = update
                    .query_row(
                        params![
                            HUB_USER,
                            HUB_ACCOUNT,
                            AI_GATEWAY_PROVIDER_NAME,
                            project.id,
                            assignment.thread_id,
                            assignment.previous_project_id,
                            assignment.legacy
                        ],
                        |r| r.get(0),
                    )
                    .context("会话归属已经改变，请重新检查。")?;
                ensure!(
                    root_key(&cwd) == root_key(&project.root),
                    "会话目录已经改变，请重新检查。"
                );
            }
        }
    }
    tx.commit()?;
    Ok(assignments)
}

fn open_for_repair(home: &Path) -> Result<Connection> {
    let (dir, provider) = database_settings(home)?;
    ensure!(
        provider.as_deref() == Some(AI_GATEWAY_PROVIDER_NAME)
            && local_project_owner(home)?.is_some(),
        "当前身份或模型来源已改变，未修改会话归属。"
    );
    Ok(Connection::open_with_flags(
        dir.join(super::CODEX_APP_STATE_DB),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

const BASE_SCOPE: &str = "model_provider=?3 AND source='vscode' AND archived=0
    AND attached_thread_parent_id IS NULL AND thread_section_id IS NULL";
const CURRENT_OWNER: &str =
    "(creator_user_id=?1 AND creator_account_id=?2 AND thread_source='user')";
const LEGACY_OWNER: &str =
    "(creator_user_id IS NULL AND creator_account_id IS NULL AND thread_source='user')";
const WRITE_OWNER: &str =
    "((?7=0 AND creator_user_id=?1 AND creator_account_id=?2 AND thread_source='user')
    OR (?7=1 AND creator_user_id IS NULL AND creator_account_id IS NULL AND thread_source='user'))";

pub(crate) fn undo_thread_memberships(home: &Path, assignments: &[ThreadAssignment]) -> Result<()> {
    if assignments.is_empty() {
        return Ok(());
    }
    let mut db = open_for_repair(home)?;
    let tx = db.transaction()?;
    {
        let mut update = tx.prepare(&format!(
            "UPDATE threads SET project_id=?4 WHERE id=?5 AND project_id=?6 AND {BASE_SCOPE} AND {WRITE_OWNER}"
        ))?;
        for assignment in assignments {
            ensure!(
                update.execute(params![
                    HUB_USER,
                    HUB_ACCOUNT,
                    AI_GATEWAY_PROVIDER_NAME,
                    assignment.previous_project_id,
                    assignment.thread_id,
                    assignment.project_id,
                    assignment.legacy
                ])? == 1,
                "会话归属已经改变，未执行撤销。"
            );
        }
    }
    tx.commit()?;
    Ok(())
}

fn local_id(id: &str) -> bool {
    id.strip_prefix("local-").is_some_and(|hex| {
        hex.len() == 32
            && hex
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    }) || uuid::Uuid::parse_str(id).is_ok_and(|u| !u.is_nil() && u.to_string() == id)
}

// A scope key is lexically exact. Windows handles may have a drive prefix or
// slash spelling, but case below the drive can matter. Do not resolve aliases,
// links, parent paths, network shares, or relative paths to guess a match.
fn root_key(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return None;
    }
    #[cfg(windows)]
    {
        let path = raw.replace('\\', "/");
        let path = path
            .strip_prefix("//?/")
            .unwrap_or(&path)
            .trim_end_matches('/');
        let bytes = path.as_bytes();
        if bytes.len() <= 3 || !bytes[0].is_ascii_alphabetic() || &bytes[1..3] != b":/" {
            return None;
        }
        if path[3..].split('/').any(|s| {
            s.is_empty()
                || s == "."
                || s == ".."
                || s.ends_with([' ', '.'])
                || s.contains([':', '<', '>', '"', '|', '?', '*'])
        }) {
            return None;
        }
        Some(format!(
            "{}{}",
            char::from(bytes[0].to_ascii_uppercase()),
            &path[1..]
        ))
    }
    #[cfg(not(windows))]
    {
        let path = raw.trim_end_matches('/');
        let tail = path.strip_prefix('/')?;
        if tail
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        {
            return None;
        }
        Some(path.to_owned())
    }
}

fn root_identity(key: &str) -> String {
    #[cfg(windows)]
    {
        key.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        key.to_owned()
    }
}

pub(crate) fn root_match_key(raw: &str) -> Option<String> {
    root_key(raw).map(|k| root_identity(&k))
}

pub(super) fn same_root(a: &str, b: &str) -> bool {
    root_key(a).is_some_and(|a| Some(a) == root_key(b))
}

fn read_candidates(
    tx: &Transaction<'_>,
    preferred_project_ids: &HashSet<String>,
    manual: &HashMap<String, String>,
    include_legacy: bool,
    standalone: &HashSet<String>,
    backup_cards: &HashMap<String, String>,
) -> Result<(Vec<LocalProjectCandidate>, ProjectEvidence)> {
    // An unrecognized project scope/ownership column is a new contract. Do
    // not use default assumptions for it.
    let known_schema: bool = tx.query_row(
        "SELECT (SELECT count(*)=6 AND count(CASE WHEN name IN ('id','name','metadata','position','created_at_ms','updated_at_ms') THEN 1 END)=6 FROM pragma_table_info('projects'))
         AND (SELECT count(*)=3 AND count(CASE WHEN name IN ('project_id','position','path') THEN 1 END)=3 FROM pragma_table_info('project_roots'))
         AND (SELECT count(*)=10 FROM pragma_table_info('threads') WHERE name IN
             ('creator_user_id','creator_account_id','source','thread_source','archived','attached_thread_parent_id','thread_section_id','model_provider','cwd','project_id'))",
        [], |r| r.get(0))?;
    if !known_schema {
        return Ok((
            Vec::new(),
            ProjectEvidence {
                unavailable: Some(
                    "这份库可以统计项目数量，但项目、目录或会话归属字段与已支持的版本不同。请保留原库，在原账号的 Codex 中查看项目。",
                ),
                ..Default::default()
            },
        ));
    }
    let mut info = ProjectEvidence::default();
    let mut projects = tx.prepare(
        "SELECT p.id,p.name,p.metadata,p.created_at_ms,p.updated_at_ms,r.path,
            count(*) OVER (PARTITION BY r.project_id),r.position
         FROM projects p LEFT JOIN project_roots r ON r.project_id=p.id
         ORDER BY p.position,p.id,r.position",
    )?;
    let mut roots = projects.query([])?;
    let mut counts = HashMap::<String, usize>::new();
    let mut visible_counts = HashMap::<String, usize>::new();
    let mut candidates = Vec::<(String, LocalProjectCandidate)>::new();
    let mut invalid_ids = HashSet::new();
    while let Some(row) = roots.next()? {
        let id: String = row.get(0)?;
        let root: Option<String> = row.get(5)?;
        let Some(key) = root.as_deref().and_then(root_key) else {
            invalid_ids.insert(id);
            continue;
        };
        let root = root.expect("normalized a present project root");
        // Treat case-only duplicates as ambiguous even on case-sensitive
        // directories; the caller may disambiguate a manually recreated card
        // by passing the exact project ID currently shown in the sidebar.
        *counts.entry(root_identity(&key)).or_default() += 1;
        if preferred_project_ids.contains(&id) {
            *visible_counts.entry(root_identity(&key)).or_default() += 1;
        }
        let name: String = row.get(1)?;
        let metadata: String = row.get(2)?;
        let metadata: Option<serde_json::Map<String, Value>> = serde_json::from_str(&metadata).ok();
        if metadata.as_ref().is_some_and(|m| {
            m.keys().any(|k| {
                k.starts_with("codex.chatGpt")
                    || k == "codex.retiredChatGptBacking"
                    || k == "codex.localProjectId"
            })
        }) {
            // Native Codex only assigns codex.localProjectId to ChatGPT-backed
            // projects. They must be loaded with the associated ChatGPT account.
            info.backed_projects += 1;
            continue;
        }
        let created_at_ms: i64 = row.get(3)?;
        let updated_at_ms: i64 = row.get(4)?;
        if row.get::<_, i64>(6)? != 1
            || row.get::<_, i64>(7)? != 0
            || !local_id(&id)
            || name.trim().is_empty()
            || name.chars().any(char::is_control)
            || !metadata.is_some_and(|m| {
                m.iter().all(|(key, value)| {
                    matches!(key.as_str(), "appearance.color" | "appearance.marker")
                        && value.is_string()
                })
            })
            || created_at_ms < 0
            || updated_at_ms < created_at_ms
            || updated_at_ms > 9_007_199_254_740_991
        {
            invalid_ids.insert(id);
            continue;
        }
        candidates.push((
            key,
            LocalProjectCandidate {
                id,
                name,
                root,
                created_at_ms,
                updated_at_ms,
                predecessor_id: None,
                pending_assignments: Vec::new(),
            },
        ));
    }
    info.invalid_projects = invalid_ids.len();
    let mut conflicts = tx.prepare(&format!(
        "SELECT DISTINCT project_id FROM threads WHERE project_id IS NOT NULL
         AND (creator_user_id IS NOT ?1 OR creator_account_id IS NOT ?2)
         AND NOT COALESCE(model_provider=?3 AND source='vscode' AND {LEGACY_OWNER},0)"
    ))?;
    let foreign = conflicts
        .query_map([HUB_USER, HUB_ACCOUNT, AI_GATEWAY_PROVIDER_NAME], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    let before = candidates.len();
    candidates.retain(|(_, p)| !foreign.contains(&p.id));
    info.conflicting_projects = before - candidates.len();

    // A predecessor is provable only if there are exactly two project rows
    // for this root and the other one is a valid, older, invisible local row.
    let hidden: HashMap<_, _> = candidates
        .iter()
        .filter(|(_, p)| !preferred_project_ids.contains(&p.id))
        .map(|(key, p)| (key.clone(), (p.id.clone(), p.created_at_ms)))
        .collect();
    let before = candidates.len();
    candidates.retain(|(key, p)| {
        let identity = root_identity(key);
        counts.get(&identity) == Some(&1)
            || (preferred_project_ids.contains(&p.id) && visible_counts.get(&identity) == Some(&1))
    });
    info.ambiguous_roots = before - candidates.len();
    if candidates.is_empty() {
        return Ok((Vec::new(), info));
    }
    for (key, project) in &mut candidates {
        if counts.get(&root_identity(key)) == Some(&2) {
            project.predecessor_id = hidden
                .get(key)
                .filter(|(_, created)| *created < project.created_at_ms)
                .map(|(id, _)| id.clone());
        }
    }

    let index: HashMap<_, _> = candidates
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (k.clone(), i))
        .collect();
    let by_id: HashMap<_, _> = candidates
        .iter()
        .enumerate()
        .map(|(i, (_, p))| (p.id.clone(), i))
        .collect();
    let mut evidence = tx.prepare(&format!(
        "SELECT id,cwd,project_id,creator_user_id IS NULL FROM threads WHERE {BASE_SCOPE}
        AND ({CURRENT_OWNER} OR {LEGACY_OWNER}) ORDER BY id"
    ))?;
    let mut rows = evidence.query(params![HUB_USER, HUB_ACCOUNT, AI_GATEWAY_PROVIDER_NAME])?;
    let mut seeded = HashSet::new();
    let mut grouped_cards = HashSet::new();
    let mut grouped_legacy = HashSet::new();
    let mut legacy_rows = vec![Vec::new(); candidates.len()];
    // Old SQLite fixtures and older clients may not store a title. Fetch only
    // a short display title of an eligible row, never a message or rollout.
    let mut title_query = if standalone.is_empty() {
        None
    } else {
        let cols: (bool,bool) = tx.query_row("SELECT count(*) FILTER (WHERE name='name'),count(*) FILTER (WHERE name='title') FROM pragma_table_info('threads')",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
        match cols {
            (true,true) => Some(tx.prepare("SELECT substr(coalesce(nullif(trim(name),''),title),1,120) FROM threads WHERE id=?1")?),
            (true,false) => Some(tx.prepare("SELECT substr(name,1,120) FROM threads WHERE id=?1")?),
            (false,true) => Some(tx.prepare("SELECT substr(title,1,120) FROM threads WHERE id=?1")?),
            (false,false) => None
        }
    };
    while let Some(row) = rows.next()? {
        let prev: Option<String> = row.get(2)?;
        let linked = prev.as_ref().and_then(|id| by_id.get(id)).copied();
        let Some(key) = row
            .get::<_, Option<String>>(1)?
            .as_deref()
            .and_then(root_key)
        else {
            if linked.is_some() {
                info.mismatched_grouped_paths += 1;
            }
            continue;
        };
        let thread_id: String = row.get(0)?;
        let legacy: bool = row.get(3)?;
        if let Some(i) = linked {
            let (project_root, p) = &candidates[i];
            let in_root = key == *project_root
                || key
                    .strip_prefix(project_root.as_str())
                    .is_some_and(|tail| tail.starts_with('/'));
            if !in_root {
                info.mismatched_grouped_paths += 1;
            } else if manual
                .get(&thread_id)
                .is_none_or(|assigned| assigned == &p.id)
            {
                // A stored project ID plus a matching local root can reconstruct a card.
                // Never repoint this conversation or use a subdirectory to assign ungrouped ones.
                if !legacy
                    || backup_cards
                        .get(&p.id)
                        .is_some_and(|root| same_root(root, &p.root))
                {
                    grouped_cards.insert(i);
                } else {
                    grouped_legacy.insert(i);
                }
            }
        }
        if legacy && prev.is_some() {
            continue;
        }
        let Some(&i) = index.get(&key) else { continue };
        let project = &mut candidates[i].1;
        if let Some(target) = manual.get(&thread_id) {
            if standalone.contains(&thread_id)
                && !legacy
                && row.get::<_, Option<String>>(2)?.is_none()
                && preferred_project_ids.contains(&project.id)
                && let Some(old) = &project.predecessor_id
            {
                let title = match &mut title_query {
                    Some(q) => q.query_row([&thread_id], |r| r.get::<_, Option<String>>(0))?,
                    None => None,
                }
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
                let title = if title.is_empty() {
                    thread_id.clone()
                } else {
                    title
                };
                info.standalone
                    .entry(project.id.clone())
                    .or_insert_with(|| (old.clone(), Vec::new()))
                    .1
                    .push((thread_id, title));
            } else if !legacy && target == &project.id && preferred_project_ids.contains(target) {
                seeded.insert(i);
            }
            // Keep an explicit placement, including manual "no project".
            continue;
        }
        if prev.as_deref() == Some(project.id.as_str()) {
            seeded.insert(i);
            continue;
        }
        if prev.is_some() && prev != project.predecessor_id {
            continue;
        }
        let change = ThreadAssignment {
            thread_id,
            project_id: project.id.clone(),
            previous_project_id: prev,
            legacy,
        };
        if legacy {
            legacy_rows[i].push(change);
        } else {
            project.pending_assignments.push(change);
        }
    }
    let candidates = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(i, (_, mut project))| {
            let has_seed = seeded.contains(&i);
            if !has_seed {
                // A visible card suffices for matching an unassigned chat. A
                // non-null link to its older sibling needs a manual placement sample.
                project.predecessor_id = None;
                project
                    .pending_assignments
                    .retain(|a| a.previous_project_id.is_none());
            }
            let owned = has_seed || !project.pending_assignments.is_empty();
            let verified_target = owned || preferred_project_ids.contains(&project.id);
            let eligible_count = legacy_rows[i].len();
            if verified_target && eligible_count > 0 {
                info.eligible_legacy_by_project
                    .insert(project.id.clone(), eligible_count);
                info.eligible_legacy_count += eligible_count;
                if include_legacy {
                    project.pending_assignments.append(&mut legacy_rows[i]);
                }
            }
            let regular = owned
                || grouped_cards.contains(&i)
                || (verified_target && eligible_count > 0)
                || info.standalone.contains_key(&project.id);
            if !regular && grouped_legacy.contains(&i) {
                info.legacy_cards.insert(project.id.clone());
                return include_legacy.then_some(project);
            }
            if !regular {
                info.unverified_projects += 1;
            }
            regular.then_some(project)
        })
        .collect();
    Ok((candidates, info))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const OLD: &str = "local-0123456789abcdef0123456789abcdef";
    const NEW: &str = "local-fedcba9876543210fedcba9876543210";
    const OTHER: &str = "local-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HIDDEN: &str = "local-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct Fixture {
        tmp: tempfile::TempDir,
        db: Connection,
        root: String,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path();
            let claims = json!({
                "sub":"local|user_codexhub_local",
                "https://api.openai.com/auth": {"localhost":true,"chatgpt_user_id":HUB_USER,"chatgpt_account_id":HUB_ACCOUNT}
            });
            let token = format!("e30.{}.sig", super::super::b64url_json(&claims).unwrap());
            fs::write(dir.join("auth.json"), serde_json::to_vec(&json!({
                "auth_mode":LOCAL_AUTH_MODE, "tokens":{"account_id":HUB_ACCOUNT,"access_token":token,"id_token":token}
            })).unwrap()).unwrap();
            let mut config = toml_edit::DocumentMut::new();
            config["model_provider"] = toml_edit::value(AI_GATEWAY_PROVIDER_NAME);
            config["sqlite_home"] = toml_edit::value(dir.to_str().unwrap());
            fs::write(dir.join("config.toml"), config.to_string()).unwrap();
            let db = Connection::open(dir.join(super::super::CODEX_APP_STATE_DB)).unwrap();
            db.execute_batch("CREATE TABLE projects (id TEXT PRIMARY KEY,name TEXT,metadata TEXT,position INTEGER,created_at_ms INTEGER,updated_at_ms INTEGER);
             CREATE TABLE project_roots (project_id TEXT,position INTEGER,path TEXT);
             CREATE TABLE threads (id TEXT PRIMARY KEY,creator_user_id TEXT,creator_account_id TEXT,source TEXT,thread_source TEXT,archived INTEGER,attached_thread_parent_id TEXT,thread_section_id TEXT,model_provider TEXT,cwd TEXT,project_id TEXT);").unwrap();
            let root = dir.join("work").to_str().unwrap().to_owned();
            Self { tmp, db, root }
        }

        fn project(&self, id: &str, root: &str, created: i64) {
            self.db
                .execute(
                    "INSERT INTO projects VALUES (?1,'Demo','{}',0,?2,?2)",
                    params![id, created],
                )
                .unwrap();
            self.db
                .execute(
                    "INSERT INTO project_roots VALUES (?1,0,?2)",
                    params![id, root],
                )
                .unwrap();
        }

        fn thread(&self, id: &str, root: &str, project: Option<&str>) {
            self.db
                .execute(
                    "INSERT INTO threads VALUES (?1,?4,?5,'vscode','user',0,NULL,NULL,?6,?2,?3)",
                    params![
                        id,
                        root,
                        project,
                        HUB_USER,
                        HUB_ACCOUNT,
                        AI_GATEWAY_PROVIDER_NAME
                    ],
                )
                .unwrap();
        }

        fn state(&self) -> Vec<(String, Option<String>)> {
            self.db
                .prepare("SELECT id,project_id FROM threads ORDER BY id")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        }

        fn preview(&self, cards: &[&str]) -> Vec<LocalProjectCandidate> {
            local_project_candidates_for_ids(
                self.tmp.path(),
                &cards.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            )
            .unwrap()
        }
    }

    #[test]
    fn empty_sqlite_sidebar_recovers_eleven_native_cards_without_rewriting_thirty_nine_links() {
        use super::super::{SidebarAction, sidebar_projects};
        let f = Fixture::new();
        let home = f.tmp.path();
        let owner = local_project_owner(home).unwrap().unwrap();
        let desktop = Connection::open(home.join(".codex-global-state.sqlite")).unwrap();
        desktop.execute_batch("PRAGMA user_version=1; CREATE TABLE settings(scope TEXT NOT NULL,key TEXT NOT NULL,value TEXT NOT NULL,PRIMARY KEY(scope,key)) WITHOUT ROWID;
          INSERT INTO settings VALUES('atom','local-projects','{}'),('global','unrelated','42');").unwrap();
        desktop
            .execute(
                "INSERT INTO settings VALUES('global','electron-renderer-content-owner',?1)",
                [json!(owner).to_string()],
            )
            .unwrap();
        let old_json = br#"{"local-projects":{"stale":{}}}"#;
        fs::write(home.join(".codex-global-state.json"), old_json).unwrap();
        for n in 1..=11 {
            let project = format!("00000000-0000-4000-8000-{n:012}");
            let root = home.join(format!("project-{n}")).display().to_string();
            f.project(&project, &root, n);
            for c in 0..if n <= 6 { 4 } else { 3 } {
                f.thread(
                    &format!("s-{n}-{c}"),
                    &format!("{root}/src"),
                    Some(&project),
                );
            }
        }
        f.db.execute(
            "UPDATE projects SET metadata=?1",
            [json!({"appearance.color":"blue"}).to_string()],
        )
        .unwrap();
        let before = f.state();
        assert_eq!(before.len(), 39);
        let check = sidebar_projects(home, SidebarAction::Check).unwrap();
        assert_eq!(
            (
                check.active_project_count,
                check.planned_projects,
                check.planned_threads
            ),
            (0, 11, 0)
        );
        let action = sidebar_projects(home, SidebarAction::Repair).unwrap();
        assert_eq!((action.changed_count, action.grouped_thread_count), (11, 0));
        assert!(
            !sidebar_projects(home, SidebarAction::Check)
                .unwrap()
                .can_repair
        );
        assert_eq!(
            fs::read(home.join(".codex-global-state.json")).unwrap(),
            old_json
        );
        assert_eq!(f.state(), before);
        sidebar_projects(home, SidebarAction::Undo).unwrap();
        assert_eq!(
            desktop
                .query_row("SELECT count(*) FROM settings", [], |r| r
                    .get::<_, usize>(0))
                .unwrap(),
            3
        );
        desktop
            .execute(
                "UPDATE settings SET value='\"other\"' WHERE key='electron-renderer-content-owner'",
                [],
            )
            .unwrap();
        assert!(
            sidebar_projects(home, SidebarAction::Repair)
                .unwrap_err()
                .to_string()
                .contains("活动账号")
        );
        desktop.execute_batch("PRAGMA user_version=2").unwrap();
        assert!(
            sidebar_projects(home, SidebarAction::Check)
                .unwrap_err()
                .to_string()
                .contains("已提交结构")
        );
    }

    #[test]
    fn grouped_legacy_card_needs_confirmation_or_matching_owner_backup_and_refuses_foreign_projects()
     {
        use super::super::{SidebarAction, sidebar_projects, sidebar_projects_with_options};
        let f = Fixture::new();
        let home = f.tmp.path();
        let owner = local_project_owner(home).unwrap().unwrap();
        fs::write(home.join(".codex-global-state.json"), "{}").unwrap();
        let backup = home.join(".codex-global-state.json.bak");
        let put_backup = |account: &str, path: &str| {
            fs::write(
                &backup,
                json!({
                    "electron-account-renderer-state-v1":{"owner":account},
                    "local-projects":{(OLD):{"id":OLD,"rootPaths":[path]}}
                })
                .to_string(),
            )
            .unwrap()
        };
        f.project(OLD, &f.root, 1);
        f.thread("legacy", &f.root, Some(OLD));
        f.db.execute(
            "UPDATE threads SET creator_user_id=NULL,creator_account_id=NULL",
            [],
        )
        .unwrap();
        let other = format!("{}/separate", f.root);
        f.project(OTHER, &other, 2);
        f.thread("foreign", &other, Some(OTHER));
        f.db.execute(
            "UPDATE threads SET creator_account_id='another' WHERE id='foreign'",
            [],
        )
        .unwrap();
        let wrong = format!("{}-similarly-named", f.root);
        f.project(HIDDEN, &wrong, 3);
        f.thread("wrong-root", &format!("{wrong}123"), Some(HIDDEN));
        f.project(NEW, &format!("{}/cloud", f.root), 4);
        f.db.execute(
            "UPDATE projects SET metadata=?1 WHERE id=?2",
            params![json!({"codex.chatGptBacking":"remote"}).to_string(), NEW],
        )
        .unwrap();
        let before = f.state();
        for (account, root) in [("other", f.root.as_str()), (owner.as_str(), other.as_str())] {
            put_backup(account, root);
            let strict = sidebar_projects(home, SidebarAction::Check).unwrap();
            assert!(!strict.can_repair);
            assert!(
                strict.detail.contains("有 1 个项目已有原分组")
                    && strict.detail.contains("其他账号")
                    && strict.detail.contains("目录不匹配 1 条")
                    && strict.detail.contains("ChatGPT 云端账号")
            );
        }
        assert_eq!(
            sidebar_projects_with_options(home, SidebarAction::Repair, true)
                .unwrap()
                .changed_count,
            1
        );
        assert_eq!(f.state(), before);
        sidebar_projects(home, SidebarAction::Undo).unwrap();
        put_backup(&owner, &f.root);
        let checked = sidebar_projects(home, SidebarAction::Check).unwrap();
        assert_eq!((checked.planned_projects, checked.planned_threads), (1, 0));
        assert!(!checked.detail.contains("有 1 个项目已有原分组"));
        f.db.execute(
            "UPDATE threads SET creator_account_id='other' WHERE id='legacy'",
            [],
        )
        .unwrap();
        assert!(
            !sidebar_projects_with_options(home, SidebarAction::Check, true)
                .unwrap()
                .can_repair
        );
    }

    #[test]
    fn visible_recreated_project_restores_both_missing_links_and_preserves_manual_moves() {
        let f = Fixture::new();
        f.project(OLD, &f.root, 1);
        f.project(NEW, &f.root, 2);
        let other = format!("{}/sub", f.root);
        let hidden = format!("{}/unlisted", f.root);
        f.project(OTHER, &other, 1);
        f.project(HIDDEN, &hidden, 1);
        f.thread("manual-1", &f.root, Some(NEW));
        f.thread("manual-2", &f.root, Some(NEW));
        for i in 0..60 {
            f.thread(&format!("old-{i}"), &f.root, Some(OLD));
        }
        for i in 0..40 {
            f.thread(&format!("unassigned-{i}"), &f.root, None);
        }
        f.thread("user-moved-elsewhere", &f.root, Some(OTHER));
        f.thread("archived", &f.root, Some(OLD));
        f.db.execute("UPDATE threads SET archived=1 WHERE id='archived'", [])
            .unwrap();
        f.thread("foreign", &f.root, None);
        f.db.execute(
            "UPDATE threads SET creator_account_id='another' WHERE id='foreign'",
            [],
        )
        .unwrap();
        f.thread("other-source", &f.root, None);
        f.db.execute(
            "UPDATE threads SET model_provider='elsewhere' WHERE id='other-source'",
            [],
        )
        .unwrap();
        f.thread("hidden-root", &hidden, None);
        f.thread("different-directory", &other, Some(OLD));

        let before = f.state();
        let candidates = f.preview(&[NEW, OTHER]);
        let plan: Vec<_> = candidates
            .iter()
            .flat_map(|p| &p.pending_assignments)
            .collect();
        assert_eq!(plan.len(), 101);
        assert_eq!(
            plan.iter()
                .filter(|a| a.previous_project_id.as_deref() == Some(OLD))
                .count(),
            60
        );
        assert_eq!(f.state(), before);
        let changes = repair_thread_memberships(f.tmp.path(), &candidates).unwrap();
        assert_eq!(changes.len(), 101);
        for ((id, previous), (_, now)) in before.iter().zip(&f.state()) {
            if id.starts_with("old-") || id.starts_with("unassigned-") {
                assert_eq!(now.as_deref(), Some(NEW));
            } else if id == "hidden-root" {
                assert_eq!(now.as_deref(), Some(HIDDEN));
            } else {
                assert_eq!(now, previous, "unexpected move: {id}");
            }
        }
        assert!(
            f.preview(&[NEW, OTHER])
                .iter()
                .all(|p| p.pending_assignments.is_empty())
        );
        f.db.execute(
            "UPDATE threads SET project_id=?1 WHERE id='unassigned-9'",
            [OTHER],
        )
        .unwrap();
        let manual = f.state();
        assert!(undo_thread_memberships(f.tmp.path(), &changes).is_err());
        assert_eq!(f.state(), manual);
        f.db.execute(
            "UPDATE threads SET project_id=?1 WHERE id='unassigned-9'",
            [NEW],
        )
        .unwrap();
        undo_thread_memberships(f.tmp.path(), &changes).unwrap();
        assert_eq!(f.state(), before);
    }

    #[test]
    fn sidebar_only_samples_must_have_verified_native_origin_and_preserve_explicit_choices() {
        let f = Fixture::new();
        let child = format!("{}/another", f.root);
        f.project(OLD, &f.root, 1);
        f.project(NEW, &f.root, 2);
        f.project(OTHER, &child, 1);
        f.thread("sample", &f.root, Some(OLD));
        f.thread("other-dir", &child, None);
        f.thread("foreign", &f.root, None);
        f.db.execute(
            "UPDATE threads SET creator_account_id='someone-else' WHERE id='foreign'",
            [],
        )
        .unwrap();
        f.thread("foreign-provider", &f.root, None);
        f.db.execute(
            "UPDATE threads SET model_provider='other' WHERE id='foreign-provider'",
            [],
        )
        .unwrap();
        f.thread("remaining", &f.root, Some(OLD));
        f.thread("manual-other", &f.root, None);
        f.thread("manual-none", &f.root, None);

        let visible = [NEW.to_owned(), OTHER.to_owned()];
        let mut placements = HashMap::from([
            ("foreign".into(), NEW.into()),
            ("foreign-provider".into(), NEW.into()),
            ("other-dir".into(), NEW.into()),
            ("missing-thread".into(), NEW.into()),
            ("manual-other".into(), OTHER.into()),
            ("manual-none".into(), String::new()),
        ]);
        assert!(
            local_project_candidates_with_anchors(f.tmp.path(), &visible, &placements)
                .unwrap()
                .is_empty()
        );
        placements.insert("sample".into(), NEW.into());
        let candidates =
            local_project_candidates_with_anchors(f.tmp.path(), &visible, &placements).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].predecessor_id.as_deref(), Some(OLD));
        let assignments = repair_thread_memberships(f.tmp.path(), &candidates).unwrap();
        assert_eq!(
            assignments,
            vec![ThreadAssignment {
                thread_id: "remaining".into(),
                project_id: NEW.into(),
                previous_project_id: Some(OLD.into()),
                legacy: false
            }]
        );
    }

    #[test]
    fn partial_sidebar_with_no_prior_memberships_repairs_four_cards_and_nineteen_chats() {
        use super::super::{SidebarAction, sidebar_projects};
        let f = Fixture::new();
        let home = f.tmp.path();
        f.project(OLD, &f.root, 1);
        f.project(NEW, &f.root, 2);
        let independent: Vec<_> = (0..10).map(|n| format!("no-project-{n}")).collect();
        for id in &independent {
            f.thread(id, &f.root, None);
        }
        let mut expected = HashMap::new();
        for (group, count) in [13, 2, 1, 3].into_iter().enumerate() {
            let project = format!("00000000-0000-4000-8000-{group:012}");
            let path = home.join(format!("project{group}")).display().to_string();
            f.project(&project, &path, 1);
            for n in 0..count {
                let id = format!("group-{group}-chat-{n}");
                f.thread(&id, &path, None);
                expected.insert(id, project.clone());
            }
        }
        let host = format!("local:{}", home.display());
        let state_path = home.join(".codex-global-state.json");
        let state = json!({
            "local-projects":{"manual-card":{"id":"manual-card","name":"手动重建","rootPaths":[f.root]}},
            "project-order":["manual-card"],
            "thread-project-assignments":{},
            "projectless-thread-ids":independent,
            "app-server-project-id-by-legacy-project-id-by-host":{(&host):{"manual-card":NEW,"vanished-card":OLD}},
            "electron-account-renderer-state-v1":{"owner":local_project_owner(home).unwrap().unwrap()}
        });
        fs::write(&state_path, state.to_string()).unwrap();
        let original = f.state();
        assert!(original.iter().all(|(_, p)| p.is_none()));
        let check = sidebar_projects(home, SidebarAction::Check).unwrap();
        assert!(check.can_repair);
        assert_eq!(f.state(), original);
        let repaired = sidebar_projects(home, SidebarAction::Repair).unwrap();
        assert_eq!(
            (
                repaired.changed_count,
                repaired.grouped_thread_count,
                repaired.active_project_count
            ),
            (4, 19, 5)
        );
        let after: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        assert_eq!(after["local-projects"].as_object().unwrap().len(), 5);
        assert_eq!(
            after["local-projects"]["manual-card"],
            state["local-projects"]["manual-card"]
        );
        assert_eq!(
            after["projectless-thread-ids"],
            state["projectless-thread-ids"]
        );
        assert_eq!(
            after["thread-project-assignments"]
                .as_object()
                .unwrap()
                .len(),
            19
        );
        for (id, project) in f.state() {
            assert_eq!(project.as_ref(), expected.get(&id));
            if let Some(native) = expected.get(&id) {
                let card = after["thread-project-assignments"][&id]["projectId"]
                    .as_str()
                    .unwrap();
                assert_eq!(
                    after["app-server-project-id-by-legacy-project-id-by-host"][&host][card]
                        .as_str(),
                    Some(native.as_str())
                );
            } else {
                assert!(
                    !after["thread-project-assignments"]
                        .as_object()
                        .unwrap()
                        .contains_key(&id)
                );
            }
        }
        assert!(
            !sidebar_projects(home, SidebarAction::Check)
                .unwrap()
                .can_repair
        );
        sidebar_projects(home, SidebarAction::Undo).unwrap();
        assert_eq!(f.state(), original);
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(&state_path).unwrap()).unwrap(),
            state
        );

        // After an accidental undo in the previous release, the next Apply must
        // restore the old batch *and* the new selection in one operation.
        let picked = independent[0].clone();
        let selected = std::collections::BTreeMap::from([(picked.clone(), NEW.to_owned())]);
        let result = super::super::sidebar_projects_with_selection(
            home,
            SidebarAction::Repair,
            false,
            &selected,
        )
        .unwrap();
        assert_eq!(
            (
                result.changed_count,
                result.grouped_thread_count,
                result.active_project_count
            ),
            (4, 20, 5)
        );
        let restored: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        expected.insert(picked.clone(), NEW.to_owned());
        for (thread, project) in f.state() {
            assert_eq!(project.as_ref(), expected.get(&thread));
            if let Some(native) = project {
                let card = restored["thread-project-assignments"][&thread]["projectId"]
                    .as_str()
                    .unwrap();
                assert_eq!(
                    restored["app-server-project-id-by-legacy-project-id-by-host"][&host][card],
                    native
                );
            }
        }
        assert_eq!(restored["projectless-thread-ids"], json!(&independent[1..]));
        assert!(
            !sidebar_projects(home, SidebarAction::Check)
                .unwrap()
                .can_repair
        );
    }

    #[test]
    fn partial_sidebar_rebuild_and_explicit_legacy_repair_are_owner_scoped_and_reversible() {
        let f = Fixture::new();
        f.project(OLD, &f.root, 1);
        f.project(NEW, &f.root, 2);
        let second = format!("{}/another", f.root);
        f.project(OTHER, &second, 1);
        let unknown = format!("{}/unknown", f.root);
        f.project(HIDDEN, &unknown, 1);
        f.thread("known", &second, None);
        for (id, cwd) in [
            ("visible-legacy", &f.root),
            ("hidden-legacy", &second),
            ("protected-legacy", &second),
            ("unknown-origin", &second),
            ("partial-identity", &second),
            ("only-unknown", &unknown),
        ] {
            f.thread(id, cwd, None);
        }
        f.db.execute(
            "UPDATE threads SET creator_user_id=NULL, creator_account_id=NULL WHERE id!='known'",
            [],
        )
        .unwrap();
        f.db.execute(
            "UPDATE threads SET thread_source=NULL WHERE id='unknown-origin'",
            [],
        )
        .unwrap();
        f.db.execute(
            "UPDATE threads SET creator_user_id='someone' WHERE id='partial-identity'",
            [],
        )
        .unwrap();
        let choices = HashMap::from([("protected-legacy".to_owned(), String::new())]);
        let inspect = |opt| {
            inspect_project_candidates(f.tmp.path(), &[NEW.to_owned()], &choices, opt).unwrap()
        };
        let before = f.state();
        let (strict, summary) = inspect(false);
        assert_eq!(summary.eligible_legacy_count, 2);
        assert_eq!(summary.eligible_legacy_by_project.len(), 2);
        assert_eq!(summary.eligible_legacy_by_project[NEW], 1);
        assert!(!strict.iter().any(|p| p.id == HIDDEN));
        let strict_updates = repair_thread_memberships(f.tmp.path(), &strict).unwrap();
        assert_eq!(strict_updates.len(), 1);
        assert_eq!(strict_updates[0].thread_id, "known");
        undo_thread_memberships(f.tmp.path(), &strict_updates).unwrap();
        let (opt, _) = inspect(true);
        let changes = repair_thread_memberships(f.tmp.path(), &opt).unwrap();
        assert_eq!(changes.len(), 3);
        assert_eq!(changes.iter().filter(|a| a.legacy).count(), 2);
        let null_users:u64=f.db.query_row("SELECT count(*) FROM threads WHERE creator_user_id IS NULL AND creator_account_id IS NULL",[],|r|r.get(0)).unwrap();
        assert_eq!(null_users, 5);
        assert_eq!(inspect(true).1.eligible_legacy_count, 0);
        undo_thread_memberships(f.tmp.path(), &changes).unwrap();
        assert_eq!(f.state(), before);
    }

    #[test]
    fn choose_one_projectless_chat_for_a_rebuilt_card_and_undo_only_that_choice() {
        use super::super::{SidebarAction, sidebar_projects, sidebar_projects_with_selection};
        use std::collections::BTreeMap;
        let f = Fixture::new();
        let home = f.tmp.path();
        f.project(OLD, &f.root, 1);
        f.project(NEW, &f.root, 2);
        let unrelated = home.join("other").display().to_string();
        f.project(OTHER, &unrelated, 1);
        for id in [
            "wanted",
            "leave-alone",
            "archived",
            "other-user",
            "other-host",
            "placed",
            "legacy",
        ] {
            f.thread(id, &f.root, None);
        }
        f.thread("unrelated", &unrelated, None);
        f.db.execute_batch(
            "ALTER TABLE threads ADD COLUMN name TEXT;
            ALTER TABLE threads ADD COLUMN title TEXT;
            UPDATE threads SET name='桌面标题',title='较长的旧标题' WHERE id='wanted';
            UPDATE threads SET archived=1 WHERE id='archived';
            UPDATE threads SET creator_user_id='different' WHERE id='other-user';
            UPDATE threads SET creator_user_id=NULL, creator_account_id=NULL WHERE id='legacy';",
        )
        .unwrap();
        let host = format!("local:{}", home.display());
        let path = home.join(".codex-global-state.json");
        let state = json!({
            "local-projects":{"rebuilt":{"id":"rebuilt","name":"重建项目","rootPaths":[f.root]}},
            "project-order":["rebuilt"],
            "app-server-project-id-by-legacy-project-id-by-host":{(&host):{"rebuilt":NEW}},
            "projectless-thread-ids":["unrelated","wanted","leave-alone","archived","other-user","other-host","placed","legacy"],
            "thread-project-membership-host-ids":{"other-host":"remote:abc"},
            "thread-project-assignments":{"placed":{"projectKind":"remote","projectId":"external"}}
        });
        fs::write(&path, state.to_string()).unwrap();
        let before = f.state();
        let checked =
            sidebar_projects_with_selection(home, SidebarAction::Check, false, &BTreeMap::new())
                .unwrap();
        assert!(!checked.can_repair);
        assert_eq!(checked.projectless_choices.len(), 2);
        let target = checked
            .projectless_choices
            .iter()
            .find(|c| c.thread_id == "wanted")
            .unwrap();
        assert_eq!(target.project_id, NEW);
        assert_eq!(target.title, "桌面标题");
        let stale = BTreeMap::from([("wanted".into(), OLD.into())]);
        assert!(
            sidebar_projects_with_selection(home, SidebarAction::Repair, false, &stale).is_err()
        );
        assert_eq!(f.state(), before);
        let selected = BTreeMap::from([("wanted".into(), NEW.into())]);
        let changed =
            sidebar_projects_with_selection(home, SidebarAction::Repair, false, &selected).unwrap();
        assert_eq!(
            (changed.changed_count, changed.grouped_thread_count),
            (0, 1)
        );
        let mut after: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(after["local-projects"], state["local-projects"]);
        assert_eq!(
            after["thread-project-assignments"]["wanted"],
            json!({"projectKind":"local","projectId":"rebuilt"})
        );
        assert!(f.state().iter().all(|(id, p)| if id == "wanted" {
            p.as_deref() == Some(NEW)
        } else {
            p.is_none()
        }));
        let flags = after["projectless-thread-ids"].as_array_mut().unwrap();
        assert!(
            flags.iter().all(|id| id != "wanted") && flags.iter().any(|id| id == "leave-alone")
        );
        flags.push(json!("new-independent"));
        fs::write(&path, after.to_string()).unwrap();
        let recheck =
            sidebar_projects_with_selection(home, SidebarAction::Check, false, &BTreeMap::new())
                .unwrap();
        assert!(
            recheck
                .projectless_choices
                .iter()
                .all(|c| c.thread_id != "wanted")
        );
        sidebar_projects(home, SidebarAction::Undo).unwrap();
        let undone: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            undone["thread-project-assignments"],
            state["thread-project-assignments"]
        );
        let flags = undone["projectless-thread-ids"].as_array().unwrap();
        assert!(
            flags.iter().any(|id| id == "new-independent")
                && state["projectless-thread-ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|id| flags.contains(id))
        );
        assert_eq!(f.state(), before);
    }

    #[test]
    fn duplicate_roots_require_a_unique_visible_target_and_existing_sample() {
        let f = Fixture::new();
        f.project(OLD, &f.root, 1);
        f.thread("not-yet-assigned", &f.root, None);
        assert_eq!(f.preview(&[])[0].pending_assignments.len(), 1);
        f.project(NEW, &f.root, 2);
        f.thread("older-link", &f.root, Some(OLD));
        let before_seed = f.preview(&[NEW]);
        assert_eq!(before_seed[0].pending_assignments.len(), 1);
        assert_eq!(
            before_seed[0].pending_assignments[0].thread_id,
            "not-yet-assigned"
        );
        assert!(before_seed[0].predecessor_id.is_none());
        f.thread("seed", &f.root, Some(NEW));
        assert!(f.preview(&[OLD, NEW]).is_empty());
        assert!(f.preview(&[]).is_empty());
        f.thread("foreign-old", &f.root, Some(OLD));
        f.db.execute(
            "UPDATE threads SET creator_user_id='different' WHERE id='foreign-old'",
            [],
        )
        .unwrap();
        let candidates = f.preview(&[NEW]);
        assert_eq!(candidates[0].pending_assignments.len(), 1);
        assert_eq!(
            candidates[0].pending_assignments[0].thread_id,
            "not-yet-assigned"
        );
    }
}
