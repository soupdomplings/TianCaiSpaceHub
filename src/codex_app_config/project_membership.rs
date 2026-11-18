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
    pub(crate) matching_thread_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadAssignment {
    pub(crate) thread_id: String,
    pub(crate) project_id: String,
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
    with_database(&dir, |tx| read_candidates(tx, &preferred))
        .context("无法核实本机原项目和会话来源。")
}

/// Restore only unassigned threads whose exact working directory identifies a
/// single verified local project. Existing assignments and non-Hub identities
/// are never changed.
pub(crate) fn repair_thread_memberships(
    home: &Path,
    candidates: &[LocalProjectCandidate],
) -> Result<Vec<ThreadAssignment>> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let (dir, provider) = database_settings(home)?;
    if provider.as_deref() != Some(AI_GATEWAY_PROVIDER_NAME) {
        return Ok(Vec::new());
    }
    let path = dir.join(super::CODEX_APP_STATE_DB);
    ensure!(path.is_file(), "找不到本机 Codex 历史库，未恢复会话归属。");
    let mut db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("无法打开 Codex 历史库 {}", path.display()))?;
    let tx = db.transaction()?;
    let mut roots = HashMap::<String, String>::new();
    for candidate in candidates {
        let Some(key) = root_key(&candidate.root) else {
            continue;
        };
        if roots
            .insert(root_identity(&key), candidate.id.clone())
            .is_some()
        {
            return Ok(Vec::new());
        }
    }
    let mut query = tx.prepare(
        "SELECT id,cwd FROM threads WHERE creator_user_id=?1 AND creator_account_id=?2
         AND model_provider=?3 AND source='vscode' AND thread_source='user'
         AND project_id IS NULL AND attached_thread_parent_id IS NULL
         AND thread_section_id IS NULL",
    )?;
    let mut rows = query.query(params![HUB_USER, HUB_ACCOUNT, AI_GATEWAY_PROVIDER_NAME])?;
    let mut assignments = Vec::new();
    while let Some(row) = rows.next()? {
        let thread_id: String = row.get(0)?;
        let Some(cwd) = row
            .get::<_, Option<String>>(1)?
            .as_deref()
            .and_then(root_key)
        else {
            continue;
        };
        let Some(project_id) = roots.get(&root_identity(&cwd)) else {
            continue;
        };
        assignments.push(ThreadAssignment {
            thread_id,
            project_id: project_id.clone(),
        });
    }
    drop(rows);
    drop(query);
    for assignment in &assignments {
        tx.execute(
            "UPDATE threads SET project_id=?1 WHERE id=?2 AND project_id IS NULL",
            params![assignment.project_id, assignment.thread_id],
        )?;
    }
    tx.commit()?;
    Ok(assignments)
}

pub(crate) fn undo_thread_memberships(home: &Path, assignments: &[ThreadAssignment]) -> Result<()> {
    if assignments.is_empty() {
        return Ok(());
    }
    let (dir, provider) = database_settings(home)?;
    ensure!(
        provider.as_deref() == Some(AI_GATEWAY_PROVIDER_NAME),
        "当前 Codex 历史库不是 Hub 管理来源，未撤销会话归属。"
    );
    let path = dir.join(super::CODEX_APP_STATE_DB);
    let mut db = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let tx = db.transaction()?;
    for assignment in assignments {
        let changed = tx.execute(
            "UPDATE threads SET project_id=NULL WHERE id=?1 AND project_id=?2",
            params![assignment.thread_id, assignment.project_id],
        )?;
        ensure!(
            changed == 1,
            "会话 {} 的项目归属已被修改，未执行撤销。",
            assignment.thread_id
        );
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
                matching_thread_count: 0,
            },
        ));
    }
    let preferred_roots = candidates
        .iter()
        .filter(|(key, project)| {
            preferred_project_ids.contains(&project.id)
                && counts
                    .get(&root_identity(key))
                    .is_some_and(|count| *count > 1)
        })
        .fold(HashMap::<String, usize>::new(), |mut roots, (key, _)| {
            *roots.entry(root_identity(key)).or_default() += 1;
            roots
        });
    candidates.retain(|(key, project)| {
        counts.get(&root_identity(key)) == Some(&1)
            || (preferred_project_ids.contains(&project.id)
                && preferred_roots.get(&root_identity(key)) == Some(&1))
    });
    if candidates.is_empty() {
        return Ok(vec![]);
    }
    // A directly assigned thread from a different or unidentified creator
    // invalidates this project as evidence of the Hub's ownership.
    let mut conflicts = tx.prepare(
        "SELECT DISTINCT project_id FROM threads WHERE project_id IS NOT NULL
         AND (creator_user_id IS NOT ?1 OR creator_account_id IS NOT ?2)",
    )?;
    let foreign: std::collections::HashSet<_> = conflicts
        .query_map([HUB_USER, HUB_ACCOUNT], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    candidates.retain(|(_, p)| !foreign.contains(&p.id));
    let index: HashMap<_, _> = candidates
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (root_identity(k), i))
        .collect();
    let mut evidence = tx.prepare(
        "SELECT cwd,project_id,count(*) FROM threads WHERE
         creator_user_id=?1 AND creator_account_id=?2 AND model_provider=?3
         AND source='vscode' AND thread_source='user' AND archived=0
         AND attached_thread_parent_id IS NULL AND thread_section_id IS NULL
         GROUP BY cwd,project_id",
    )?;
    let mut rows = evidence.query(params![HUB_USER, HUB_ACCOUNT, AI_GATEWAY_PROVIDER_NAME])?;
    while let Some(row) = rows.next()? {
        let Some(key) = row
            .get::<_, Option<String>>(0)?
            .as_deref()
            .and_then(root_key)
        else {
            continue;
        };
        let Some(&i) = index.get(&root_identity(&key)) else {
            continue;
        };
        let project = &mut candidates[i].1;
        if row
            .get::<_, Option<String>>(1)?
            .is_none_or(|id| id == project.id)
        {
            let n: u64 = row.get(2)?;
            project.matching_thread_count = project
                .matching_thread_count
                .checked_add(n)
                .context("项目会话计数超出范围。")?;
        }
    }
    Ok(candidates
        .into_iter()
        .map(|(_, p)| p)
        .filter(|p| p.matching_thread_count != 0)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn restores_only_unassigned_threads_with_an_exact_project_root() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let database = temp.path().join("history");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&database).unwrap();
        let claims = json!({
            "sub": "local|user_codexhub_local",
            "https://api.openai.com/auth": {
                "localhost": true,
                "chatgpt_account_id": HUB_ACCOUNT,
                "chatgpt_user_id": HUB_USER
            }
        });
        let token = format!("e30.{}.sig", super::super::b64url_json(&claims).unwrap());
        std::fs::write(
            home.join("auth.json"),
            serde_json::to_vec(&json!({
                "auth_mode": LOCAL_AUTH_MODE,
                "OPENAI_API_KEY": null,
                "tokens": {
                    "account_id": HUB_ACCOUNT,
                    "id_token": token.clone(),
                    "access_token": token
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let db = Connection::open(database.join(super::super::CODEX_APP_STATE_DB)).unwrap();
        db.execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY,name TEXT,metadata TEXT,position INTEGER,created_at_ms INTEGER,updated_at_ms INTEGER);
             CREATE TABLE project_roots (project_id TEXT,position INTEGER,path TEXT);
             CREATE TABLE threads (id TEXT PRIMARY KEY,creator_user_id TEXT,creator_account_id TEXT,source TEXT,thread_source TEXT,archived INTEGER,attached_thread_parent_id TEXT,thread_section_id TEXT,model_provider TEXT,cwd TEXT,project_id TEXT);
             INSERT INTO projects VALUES ('local-0123456789abcdef0123456789abcdef','Demo','{}',0,1,2);
             INSERT INTO project_roots VALUES ('local-0123456789abcdef0123456789abcdef',0,'C:/work');
             INSERT INTO threads VALUES ('unassigned','user_codexhub_local','acct_codexhub_local','vscode','user',0,NULL,NULL,'ai-gateway','C:/work',NULL);
             INSERT INTO threads VALUES ('already-grouped','user_codexhub_local','acct_codexhub_local','vscode','user',0,NULL,NULL,'ai-gateway','C:/work','local-0123456789abcdef0123456789abcdef');
             INSERT INTO threads VALUES ('other-root','user_codexhub_local','acct_codexhub_local','vscode','user',0,NULL,NULL,'ai-gateway','C:/other',NULL);",
        )
        .unwrap();
        drop(db);
        let escaped = database.display().to_string().replace('\\', "\\\\");
        std::fs::write(
            home.join("config.toml"),
            format!("model_provider = \"ai-gateway\"\nsqlite_home = \"{escaped}\"\n"),
        )
        .unwrap();

        let candidates = local_project_candidates(&home).unwrap();
        assert_eq!(candidates.len(), 1);
        let assignments = repair_thread_memberships(&home, &candidates).unwrap();
        assert_eq!(
            assignments,
            vec![ThreadAssignment {
                thread_id: "unassigned".into(),
                project_id: "local-0123456789abcdef0123456789abcdef".into(),
            }]
        );
        let db = Connection::open(database.join(super::super::CODEX_APP_STATE_DB)).unwrap();
        let grouped: String = db
            .query_row(
                "SELECT project_id FROM threads WHERE id='unassigned'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(grouped, "local-0123456789abcdef0123456789abcdef");
        let other: Option<String> = db
            .query_row(
                "SELECT project_id FROM threads WHERE id='other-root'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(other.is_none());
    }

    #[test]
    fn prefers_a_manually_recreated_project_when_the_old_row_has_the_same_root() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join(".codex");
        let database = temp.path().join("history");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&database).unwrap();
        let claims = json!({
            "sub": "local|user_codexhub_local",
            "https://api.openai.com/auth": {
                "localhost": true,
                "chatgpt_account_id": HUB_ACCOUNT,
                "chatgpt_user_id": HUB_USER
            }
        });
        let token = format!("e30.{}.sig", super::super::b64url_json(&claims).unwrap());
        std::fs::write(
            home.join("auth.json"),
            serde_json::to_vec(&json!({
                "auth_mode": LOCAL_AUTH_MODE,
                "OPENAI_API_KEY": null,
                "tokens": {
                    "account_id": HUB_ACCOUNT,
                    "id_token": token.clone(),
                    "access_token": token
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let recreated = "local-fedcba9876543210fedcba9876543210";
        let old = "local-0123456789abcdef0123456789abcdef";
        let db = Connection::open(database.join(super::super::CODEX_APP_STATE_DB)).unwrap();
        db.execute_batch(&format!(
            "CREATE TABLE projects (id TEXT PRIMARY KEY,name TEXT,metadata TEXT,position INTEGER,created_at_ms INTEGER,updated_at_ms INTEGER);
             CREATE TABLE project_roots (project_id TEXT,position INTEGER,path TEXT);
             CREATE TABLE threads (id TEXT PRIMARY KEY,creator_user_id TEXT,creator_account_id TEXT,source TEXT,thread_source TEXT,archived INTEGER,attached_thread_parent_id TEXT,thread_section_id TEXT,model_provider TEXT,cwd TEXT,project_id TEXT);
             INSERT INTO projects VALUES ('{old}','Old','{{}}',0,1,2);
             INSERT INTO projects VALUES ('{recreated}','Recreated','{{}}',0,3,4);
             INSERT INTO project_roots VALUES ('{old}',0,'C:/work');
             INSERT INTO project_roots VALUES ('{recreated}',0,'C:/work');
             INSERT INTO threads VALUES ('manual','user_codexhub_local','acct_codexhub_local','vscode','user',0,NULL,NULL,'ai-gateway','C:/work','{recreated}');
             INSERT INTO threads VALUES ('remaining','user_codexhub_local','acct_codexhub_local','vscode','user',0,NULL,NULL,'ai-gateway','C:/work',NULL);"
        ))
        .unwrap();
        drop(db);
        let escaped = database.display().to_string().replace('\\', "\\\\");
        std::fs::write(
            home.join("config.toml"),
            format!("model_provider = \"ai-gateway\"\nsqlite_home = \"{escaped}\"\n"),
        )
        .unwrap();

        let candidates = local_project_candidates_for_ids(&home, &[recreated.into()]).unwrap();
        assert_eq!(
            candidates.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec![recreated]
        );
        let assignments = repair_thread_memberships(&home, &candidates).unwrap();
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].thread_id, "remaining");
        assert_eq!(assignments[0].project_id, recreated);
    }
}
