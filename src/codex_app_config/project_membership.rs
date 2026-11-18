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
    if local_project_owner(home)?.is_none() {
        return Ok(vec![]);
    }
    let (dir, provider) = database_settings(home)?;
    if provider.as_deref() != Some(AI_GATEWAY_PROVIDER_NAME)
        || !dir.join(super::CODEX_APP_STATE_DB).try_exists()?
    {
        return Ok(vec![]);
    }
    let preferred = preferred_project_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    with_database(&dir, |tx| read_candidates(tx, &preferred, manual))
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
            "UPDATE threads SET project_id=?4 WHERE id=?5 AND project_id IS ?6 AND {THREAD_SCOPE} RETURNING cwd"
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
                            assignment.previous_project_id
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

const THREAD_SCOPE: &str = "creator_user_id=?1 AND creator_account_id=?2 AND model_provider=?3
    AND source='vscode' AND thread_source='user' AND archived=0
    AND attached_thread_parent_id IS NULL AND thread_section_id IS NULL";

pub(crate) fn undo_thread_memberships(home: &Path, assignments: &[ThreadAssignment]) -> Result<()> {
    if assignments.is_empty() {
        return Ok(());
    }
    let mut db = open_for_repair(home)?;
    let tx = db.transaction()?;
    {
        let mut update = tx.prepare(&format!(
            "UPDATE threads SET project_id=?4 WHERE id=?5 AND project_id=?6 AND {THREAD_SCOPE}"
        ))?;
        for assignment in assignments {
            ensure!(
                update.execute(params![
                    HUB_USER,
                    HUB_ACCOUNT,
                    AI_GATEWAY_PROVIDER_NAME,
                    assignment.previous_project_id,
                    assignment.thread_id,
                    assignment.project_id
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

fn read_candidates(
    tx: &Transaction<'_>,
    preferred_project_ids: &HashSet<String>,
    manual: &HashMap<String, String>,
) -> Result<Vec<LocalProjectCandidate>> {
    // An unrecognized project scope/ownership column is a new contract. Do
    // not use default assumptions for it.
    let known_schema: bool = tx.query_row(
        "SELECT (SELECT count(*)=6 AND count(CASE WHEN name IN ('id','name','metadata','position','created_at_ms','updated_at_ms') THEN 1 END)=6 FROM pragma_table_info('projects'))
         AND (SELECT count(*)=3 AND count(CASE WHEN name IN ('project_id','position','path') THEN 1 END)=3 FROM pragma_table_info('project_roots'))
         AND (SELECT count(*)=10 FROM pragma_table_info('threads') WHERE name IN
             ('creator_user_id','creator_account_id','source','thread_source','archived','attached_thread_parent_id','thread_section_id','model_provider','cwd','project_id'))",
        [], |r| r.get(0))?;
    if !known_schema {
        return Ok(vec![]);
    }
    let mut projects = tx.prepare(
        "SELECT p.id,p.name,p.metadata,p.created_at_ms,p.updated_at_ms,r.path,
            count(*) OVER (PARTITION BY r.project_id),r.position
         FROM projects p INNER JOIN project_roots r ON r.project_id=p.id
         ORDER BY p.position,p.id,r.position",
    )?;
    let mut roots = projects.query([])?;
    let mut counts = HashMap::<String, usize>::new();
    let mut visible_counts = HashMap::<String, usize>::new();
    let mut candidates = Vec::<(String, LocalProjectCandidate)>::new();
    while let Some(row) = roots.next()? {
        let root: String = row.get(5)?;
        let Some(key) = root_key(&root) else {
            continue;
        };
        // Treat case-only duplicates as ambiguous even on case-sensitive
        // directories; the caller may disambiguate a manually recreated card
        // by passing the exact project ID currently shown in the sidebar.
        *counts.entry(root_identity(&key)).or_default() += 1;
        let id: String = row.get(0)?;
        if preferred_project_ids.contains(&id) {
            *visible_counts.entry(root_identity(&key)).or_default() += 1;
        }
        let name: String = row.get(1)?;
        let metadata: String = row.get(2)?;
        let created_at_ms: i64 = row.get(3)?;
        let updated_at_ms: i64 = row.get(4)?;
        if row.get::<_, i64>(6)? != 1
            || row.get::<_, i64>(7)? != 0
            || !local_id(&id)
            || name.trim().is_empty()
            || name.chars().any(char::is_control)
            || !serde_json::from_str::<Value>(&metadata)
                .is_ok_and(|v| v.as_object().is_some_and(|o| o.is_empty()))
            || created_at_ms < 0
            || updated_at_ms < created_at_ms
            || updated_at_ms > 9_007_199_254_740_991
        {
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
    let mut conflicts = tx.prepare(
        "SELECT DISTINCT project_id FROM threads WHERE project_id IS NOT NULL
         AND (creator_user_id IS NOT ?1 OR creator_account_id IS NOT ?2)",
    )?;
    let foreign = conflicts
        .query_map([HUB_USER, HUB_ACCOUNT], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    candidates.retain(|(_, p)| !foreign.contains(&p.id));

    // A predecessor is provable only if there are exactly two project rows
    // for this root and the other one is a valid, older, invisible local row.
    let hidden: HashMap<_, _> = candidates
        .iter()
        .filter(|(_, p)| !preferred_project_ids.contains(&p.id))
        .map(|(key, p)| (key.clone(), (p.id.clone(), p.created_at_ms)))
        .collect();
    candidates.retain(|(key, p)| {
        let identity = root_identity(key);
        if preferred_project_ids.is_empty() {
            counts.get(&identity) == Some(&1)
        } else {
            preferred_project_ids.contains(&p.id) && visible_counts.get(&identity) == Some(&1)
        }
    });
    if candidates.is_empty() {
        return Ok(vec![]);
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
    let mut evidence = tx.prepare(&format!(
        "SELECT id,cwd,project_id FROM threads WHERE {THREAD_SCOPE} ORDER BY id"
    ))?;
    let mut rows = evidence.query(params![HUB_USER, HUB_ACCOUNT, AI_GATEWAY_PROVIDER_NAME])?;
    let mut seeded = HashSet::new();
    while let Some(row) = rows.next()? {
        let Some(key) = row
            .get::<_, Option<String>>(1)?
            .as_deref()
            .and_then(root_key)
        else {
            continue;
        };
        let Some(&i) = index.get(&key) else { continue };
        let project = &mut candidates[i].1;
        let thread_id: String = row.get(0)?;
        if let Some(target) = manual.get(&thread_id) {
            if target == &project.id && preferred_project_ids.contains(target) {
                seeded.insert(i);
            }
            // Keep an explicit placement, including manual "no project".
            continue;
        }
        let prev: Option<String> = row.get(2)?;
        if prev.as_deref() == Some(project.id.as_str()) {
            seeded.insert(i);
            continue;
        }
        if prev.is_some() && prev != project.predecessor_id {
            continue;
        }
        project.pending_assignments.push(ThreadAssignment {
            thread_id,
            project_id: project.id.clone(),
            previous_project_id: prev,
        });
    }
    Ok(candidates
        .into_iter()
        .enumerate()
        .filter_map(|(i, (key, project))| {
            // Duplicate roots need a sample already in the visible card. This
            // distinguishes a deliberate recreated target from an unrelated card.
            let has_seed = seeded.contains(&i);
            if !has_seed && counts.get(&root_identity(&key)) != Some(&1) {
                return None;
            }
            (has_seed || !project.pending_assignments.is_empty()).then_some(project)
        })
        .collect())
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
        assert_eq!(plan.len(), 100);
        assert_eq!(
            plan.iter()
                .filter(|a| a.previous_project_id.as_deref() == Some(OLD))
                .count(),
            60
        );
        assert_eq!(f.state(), before);
        let changes = repair_thread_memberships(f.tmp.path(), &candidates).unwrap();
        assert_eq!(changes.len(), 100);
        for ((id, previous), (_, now)) in before.iter().zip(&f.state()) {
            if id.starts_with("old-") || id.starts_with("unassigned-") {
                assert_eq!(now.as_deref(), Some(NEW));
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
                previous_project_id: Some(OLD.into())
            }]
        );
    }

    #[test]
    fn duplicate_roots_require_a_unique_visible_target_and_existing_sample() {
        let f = Fixture::new();
        f.project(OLD, &f.root, 1);
        f.thread("not-yet-assigned", &f.root, None);
        assert_eq!(f.preview(&[])[0].pending_assignments.len(), 1);
        f.project(NEW, &f.root, 2);
        f.thread("older-link", &f.root, Some(OLD));
        assert!(f.preview(&[NEW]).is_empty());
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
