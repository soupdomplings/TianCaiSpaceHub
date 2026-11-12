use super::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

pub(super) struct EntryState {
    pub entry: HistoryEntry,
    pub task_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct MemoryKey {
    pub layer: i64,
    pub slug: String,
}

struct Task {
    id: String,
    title: String,
    scenario_id: String,
    work_dir: Option<String>,
    updated_at: String,
    message_count: u64,
    execution_status: String,
}

pub(super) fn open(database: &Path, writable: bool) -> Result<Connection> {
    crate::desktop::transaction::reject_links(database)?;
    ensure!(
        database.is_file(),
        "未找到天工历史数据库，请先安装并打开天工"
    );
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(database, flags)
        .context("无法打开天工历史数据库，请确认已退出天工且文件可访问")?;
    connection.busy_timeout(Duration::from_secs(3))?;
    Ok(connection)
}

fn columns(connection: &Connection, table: &str) -> Result<HashMap<String, String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement
        .query_map([], |row| Ok((row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<HashMap<String, String>>>()?;
    Ok(rows)
}

fn require(
    connection: &Connection,
    table: &str,
    fields: &[&str],
) -> Result<HashMap<String, String>> {
    let actual = columns(connection, table)?;
    ensure!(
        fields.iter().all(|name| actual.contains_key(*name)),
        "当前天工历史结构不兼容：{table} 缺少必要字段，未重建或修改数据库"
    );
    Ok(actual)
}

pub(super) fn check_schema(connection: &Connection) -> Result<()> {
    require(
        connection,
        "sessions",
        &[
            "session_id",
            "title",
            "user_id",
            "agent_id",
            "project_id",
            "status",
            "message_count",
            "updated_at",
            "extra_data",
        ],
    )?;
    require(connection, "session_messages", &["session_id"])?;
    let tasks = require(
        connection,
        "tasks",
        &[
            "task_id",
            "session_id",
            "scenario_id",
            "title",
            "updated_at",
        ],
    )?;
    let scenarios = require(connection, "scenarios", &["scenario_id", "work_dir"])?;
    ensure!(
        !tasks.contains_key("user_id") && !scenarios.contains_key("user_id"),
        "此天工版本增加了任务或项目账号归属，须先适配，未修改数据库"
    );
    require(
        connection,
        "task_messages",
        &["id", "task_id", "execution_json"],
    )?;
    for (table, fields) in [
        (
            "memory_entries",
            &["user_id", "project_id", "session_id"][..],
        ),
        (
            "memory_index",
            &["user_id", "project_id", "layer", "slug", "source_session"][..],
        ),
        (
            "memory_sync_state",
            &["user_id", "project_id", "layer", "slug"][..],
        ),
    ] {
        if !columns(connection, table)?.is_empty() {
            require(connection, table, fields)?;
        }
    }
    Ok(())
}

fn tasks(connection: &Connection, filter: Option<&str>) -> Result<HashMap<String, Vec<Task>>> {
    let mut statement = connection.prepare(
        "SELECT t.task_id,t.title,t.scenario_id,s.work_dir,t.updated_at,
          (SELECT count(*) FROM task_messages m WHERE m.task_id=t.task_id),
          coalesce((SELECT CASE WHEN json_valid(m.execution_json)
              AND json_extract(m.execution_json,'$.version')=1
            THEN coalesce(json_extract(m.execution_json,'$.status'),'unknown') ELSE 'unknown' END
            FROM task_messages m WHERE m.task_id=t.task_id
              AND m.execution_json IS NOT NULL AND m.execution_json NOT IN ('','null')
            ORDER BY m.id DESC LIMIT 1),''),t.session_id
         FROM tasks t LEFT JOIN scenarios s ON s.scenario_id=t.scenario_id
         WHERE (?1 IS NULL OR t.session_id=?1) ORDER BY t.updated_at DESC,t.task_id",
    )?;
    let mut groups: HashMap<String, Vec<Task>> = HashMap::new();
    for row in statement.query_map([filter], |row| {
        Ok((
            row.get::<_, String>(7)?,
            Task {
                id: row.get(0)?,
                title: row.get(1)?,
                scenario_id: row.get(2)?,
                work_dir: row.get(3)?,
                updated_at: row.get(4)?,
                message_count: row.get(5)?,
                execution_status: row.get(6)?,
            },
        ))
    })? {
        let (session, task) = row?;
        groups.entry(session).or_default().push(task);
    }
    Ok(groups)
}

pub(super) fn load(
    connection: &Connection,
    database: &Path,
    filter: Option<&str>,
) -> Result<Vec<EntryState>> {
    let mut statement = connection.prepare(
        "SELECT s.session_id,s.title,s.user_id,coalesce(s.agent_id,''),coalesce(s.project_id,''),
         s.status,s.updated_at,max(s.message_count,(SELECT count(*) FROM session_messages m WHERE m.session_id=s.session_id)),
         coalesce(s.extra_data,'{}') FROM sessions s WHERE (?1 IS NULL OR s.session_id=?1)")?;
    let mut rows: BTreeMap<String, (HistoryEntry, Option<Value>)> = statement
        .query_map([filter], |row| {
            let session_id: String = row.get(0)?;
            let user_id: String = row.get(2)?;
            let extra: String = row.get(8)?;
            let parsed = serde_json::from_str::<Value>(&extra)
                .ok()
                .filter(Value::is_object);
            let entry = HistoryEntry {
                session_id: session_id.clone(),
                title: row.get(1)?,
                is_gateway: user_id == GATEWAY_USER_ID,
                user_id,
                agent_id: row.get(3)?,
                project_id: row.get(4)?,
                status: row.get(5)?,
                updated_at: row.get(6)?,
                message_count: row.get(7)?,
                work_dir: String::new(),
                can_move: false,
                can_restore: false,
                migration_id: None,
                unavailable_reason: if parsed.is_none() {
                    Some("会话项目记录格式不兼容".into())
                } else {
                    None
                },
            };
            Ok((session_id, (entry, parsed)))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut orphan = connection.prepare(
        "SELECT DISTINCT t.session_id FROM tasks t LEFT JOIN sessions s ON s.session_id=t.session_id
         WHERE s.session_id IS NULL AND t.session_id<>'' AND (?1 IS NULL OR t.session_id=?1)")?;
    for id in orphan.query_map([filter], |row| row.get::<_, String>(0))? {
        let id = id?;
        rows.insert(
            id.clone(),
            (
                HistoryEntry {
                    session_id: id,
                    title: String::new(),
                    user_id: String::new(),
                    agent_id: String::new(),
                    project_id: String::new(),
                    work_dir: String::new(),
                    status: String::new(),
                    updated_at: String::new(),
                    message_count: 0,
                    is_gateway: false,
                    can_move: false,
                    can_restore: false,
                    migration_id: None,
                    unavailable_reason: Some("此任务尚无会话归属记录，不能推定所属账号".into()),
                },
                None,
            ),
        );
    }
    ensure!(
        rows.len() <= 20_000,
        "天工历史超过 20000 条，请先在天工整理历史"
    );
    let mut task_groups = tasks(connection, filter)?;
    let mut result = Vec::with_capacity(rows.len());
    for (_, (mut entry, extra)) in rows {
        let tasks = task_groups.remove(&entry.session_id).unwrap_or_default();
        let reason = enrich(connection, database, &mut entry, extra.as_ref(), &tasks)
            .err()
            .map(|error| error.to_string());
        if entry.unavailable_reason.is_none() {
            entry.unavailable_reason = reason;
        }
        result.push(EntryState {
            entry,
            task_ids: tasks.into_iter().map(|task| task.id).collect(),
        });
    }
    Ok(result)
}

fn enrich(
    connection: &Connection,
    database: &Path,
    entry: &mut HistoryEntry,
    extra: Option<&Value>,
    tasks: &[Task],
) -> Result<()> {
    let default = database
        .parent()
        .context("天工数据库缺少所在目录")?
        .join("workspace");
    let mut directories = BTreeMap::new();
    for task in tasks {
        if entry.title.is_empty() {
            entry.title = task.title.clone();
        }
        if task.updated_at > entry.updated_at {
            entry.updated_at = task.updated_at.clone();
        }
        entry.message_count = entry.message_count.max(task.message_count);
        let directory = task.work_dir.as_ref().context("会话引用的项目已不存在")?;
        let directory = if directory.is_empty() && task.scenario_id == "default" {
            default.to_string_lossy().into_owned()
        } else {
            directory.clone()
        };
        if !directory.is_empty() {
            directories.insert(memory::path_key(Path::new(&directory)), directory);
        }
    }
    if let Some(project) = extra.and_then(|value| value.get("project")) {
        ensure!(project.is_object(), "会话项目记录格式不兼容");
        if let Some(path) = project.get("path") {
            let path = path.as_str().context("会话项目目录格式不兼容")?;
            if !path.is_empty() {
                directories.insert(memory::path_key(Path::new(path)), path.into());
            }
        }
        if let Some(id) = project.get("id").and_then(Value::as_str) {
            ensure!(id == entry.project_id, "会话记录的项目身份不一致");
        }
    }
    if directories.is_empty() && !entry.project_id.is_empty() {
        let work_dir: Option<String> = connection
            .query_row(
                "SELECT work_dir FROM scenarios WHERE scenario_id=?1",
                [&entry.project_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(path) = work_dir.filter(|path| !path.is_empty()) {
            directories.insert(memory::path_key(Path::new(&path)), path);
        } else if matches!(entry.project_id.as_str(), "0" | "default") {
            directories.insert(
                memory::path_key(&default),
                default.to_string_lossy().into_owned(),
            );
        }
    }
    ensure!(
        directories.len() <= 1,
        "会话对应多个不同工作目录，请先在天工核对项目"
    );
    if let Some(directory) = directories.into_values().next() {
        entry.work_dir = directory;
    }
    if entry.title.is_empty() {
        entry.title = "未命名会话".into();
    }
    memory::validate_component(&entry.session_id)?;
    memory::validate_component(&entry.user_id)?;
    memory::validate_component(&entry.agent_id)?;
    memory::validate_component(&entry.project_id)?;
    ensure!(!entry.work_dir.is_empty(), "未能确定此会话的真实工作目录");
    memory::validate_directory(Path::new(&entry.work_dir))?;
    ensure!(
        matches!(
            entry.status.as_str(),
            "active" | "paused" | "completed" | "terminated"
        ),
        "此会话正在运行、等待确认或状态未知，请先在天工结束任务"
    );
    for task in tasks {
        check_terminal(&task.execution_status)?;
    }
    Ok(())
}

fn check_terminal(status: &str) -> Result<()> {
    ensure!(
        matches!(
            status,
            "" | "completed" | "stopped" | "failed" | "interrupted"
        ),
        "此会话有未结束、待审批或状态未知的运行记录，请先在天工处理"
    );
    Ok(())
}

pub(super) fn selected(
    connection: &Connection,
    database: &Path,
    selected: &SessionSelection,
) -> Result<EntryState> {
    let state = load(connection, database, Some(&selected.session_id))?
        .into_iter()
        .next()
        .context("所选天工会话已不存在，请刷新历史列表")?;
    ensure!(
        state.entry.user_id == selected.user_id,
        "此会话归属已变化，请刷新历史列表后重试"
    );
    if let Some(reason) = &state.entry.unavailable_reason {
        bail!("{reason}");
    }
    check_runs(database, &selected.session_id)?;
    Ok(state)
}

fn check_runs(database: &Path, session: &str) -> Result<()> {
    let runs_path = database
        .parent()
        .context("天工数据库目录无效")?
        .join("harness-data/chat_runs.sqlite3");
    crate::desktop::transaction::reject_links(&runs_path)?;
    if !runs_path.exists() {
        return Ok(());
    }
    let runs = open(&runs_path, false)?;
    require(&runs, "runs", &["record"])?;
    let invalid: bool = runs.query_row(
        "SELECT EXISTS(SELECT 1 FROM runs WHERE NOT json_valid(record))",
        [],
        |row| row.get(0),
    )?;
    ensure!(!invalid, "天工运行记录格式不兼容，无法确认任务是否结束");
    let mut statement = runs.prepare(
        "SELECT coalesce(json_extract(record,'$.status'),'unknown'),
         json_extract(record,'$.awaiting_confirmation'),
         EXISTS(SELECT 1 FROM json_each(record,'$.prompts') p WHERE json_extract(p.value,'$.status')='queued')
         FROM runs WHERE json_extract(record,'$.session_id')=?1")?;
    for row in statement.query_map([session], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<bool>>(1)?,
            row.get::<_, bool>(2)?,
        ))
    })? {
        let (status, awaiting, queued) = row?;
        check_terminal(&status)?;
        ensure!(
            awaiting == Some(false) && !queued,
            "此会话还有待审批或排队的任务，请先在天工处理"
        );
    }
    Ok(())
}

pub(super) fn memory_keys(connection: &Connection, entry: &HistoryEntry) -> Result<Vec<MemoryKey>> {
    memory_keys_for(connection, entry, &entry.user_id)
}

pub(super) fn memory_keys_for(
    connection: &Connection,
    entry: &HistoryEntry,
    owner: &str,
) -> Result<Vec<MemoryKey>> {
    if columns(connection, "memory_index")?.is_empty() {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "SELECT cast(layer AS INTEGER),slug FROM memory_index WHERE source_session=?1 AND user_id=?2 AND project_id=?3")?;
    Ok(statement
        .query_map(params![entry.session_id, owner, entry.project_id], |row| {
            Ok(MemoryKey {
                layer: row.get(0)?,
                slug: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn move_memory(
    connection: &Connection,
    entry: &HistoryEntry,
    from: &str,
    to: &str,
    keys: &[MemoryKey],
) -> Result<()> {
    if !columns(connection, "memory_entries")?.is_empty() {
        connection.execute("UPDATE memory_entries SET user_id=?1 WHERE session_id=?2 AND user_id=?3 AND project_id=?4",
            params![to, entry.session_id, from, entry.project_id])?;
    }
    let has_sync = !columns(connection, "memory_sync_state")?.is_empty();
    for key in keys {
        let conflicting: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_index WHERE user_id=?1 AND project_id=?2 AND cast(layer AS INTEGER)=?3 AND slug=?4
              AND (user_id<>?5 OR coalesce(source_session,'')<>?6))",
            params![to, entry.project_id, key.layer, key.slug, from, entry.session_id], |row|row.get(0))?;
        ensure!(!conflicting, "目标账号存在同名记忆，未覆盖或迁移历史");
        if has_sync {
            let occupied: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM memory_sync_state WHERE user_id=?1 AND project_id=?2 AND layer=?3 AND slug=?4)",
                params![to,entry.project_id,key.layer,key.slug],|row|row.get(0))?;
            ensure!(!occupied, "目标账号存在同名记忆同步记录，未覆盖历史");
            connection.execute("UPDATE memory_sync_state SET user_id=?1 WHERE user_id=?2 AND project_id=?3 AND layer=?4 AND slug=?5",
                params![to,from,entry.project_id,key.layer,key.slug])?;
        }
    }
    if !columns(connection, "memory_index")?.is_empty() {
        connection.execute("UPDATE memory_index SET user_id=?1 WHERE user_id=?2 AND project_id=?3 AND source_session=?4",
            params![to,from,entry.project_id,entry.session_id])?;
    }
    Ok(())
}

pub(super) fn change_owner(
    connection: &Connection,
    entry: &HistoryEntry,
    from: &str,
    to: &str,
    keys: &[MemoryKey],
) -> Result<()> {
    let changed = connection.execute(
        "UPDATE sessions SET user_id=?1 WHERE session_id=?2 AND user_id=?3",
        params![to, entry.session_id, from],
    )?;
    ensure!(changed == 1, "会话归属已变化，未修改历史");
    move_memory(connection, entry, from, to, keys)
}

pub(super) fn restore_owner(
    connection: &Connection,
    entry: &HistoryEntry,
    to: &str,
    keys: &[MemoryKey],
) -> Result<()> {
    let changed = connection.execute(
        "UPDATE sessions SET user_id=?1 WHERE session_id=?2 AND user_id=?3",
        params![to, entry.session_id, entry.user_id],
    )?;
    ensure!(changed == 1, "会话归属已变化，未撤销历史迁移");
    move_memory(connection, entry, GATEWAY_USER_ID, to, keys)
}
