//! Native, installation-contained repair. All SQL runs against private copies;
//! the installed CLI alone owns SQLx migrations. No Python/runtime download.
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags, backup::Backup, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

const DB: &str = "state_5.sqlite";
const BACKUPS: &str = "hub-startup-repair-backups";
const TABLE: &str = "remote_control_enrollments";
const BASE: [&str; 7] = [
    "websocket_url",
    "account_id",
    "app_server_client_name",
    "server_id",
    "environment_id",
    "server_name",
    "updated_at",
];
const ENABLED: &str = "remote_control_enabled";
const DDL: &str = "CREATE TABLE remote_control_enrollments (websocket_url TEXT NOT NULL,account_id TEXT NOT NULL,app_server_client_name TEXT NOT NULL,server_id TEXT NOT NULL,environment_id TEXT NOT NULL,server_name TEXT NOT NULL,updated_at INTEGER NOT NULL,PRIMARY KEY(websocket_url,account_id,app_server_client_name))";
type Hashes = BTreeMap<String, String>;
type Rows = Vec<Vec<SqlValue>>;

#[derive(Clone, Debug)]
pub struct RepairOutcome {
    pub message: String,
    pub backup: Option<PathBuf>,
    pub applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Conflict {
    EarlyTable,
    EarlyColumn,
}

#[derive(Debug)]
struct State {
    conflict: Option<Conflict>,
    native24: bool,
    native37: bool,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    source: PathBuf,
    original: Hashes,
    installed: Option<Hashes>,
    stage: String,
    conflict: Conflict,
}

#[derive(Clone, Debug, Default, Serialize)]
struct Probe {
    ok: bool,
    sqlite_failure: bool,
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

fn system_output(program: &str, args: &[&str]) -> Result<Vec<u8>> {
    runtime()?.block_on(async {
        let mut command = Command::new(program);
        command.args(args).kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let output = tokio::time::timeout(Duration::from_secs(12), command.output())
            .await
            .context("系统检查超时，请稍后重试。")??;
        ensure!(
            output.status.success(),
            "系统检查未成功，请关闭相关程序后重试。"
        );
        ensure!(
            output.stdout.len() < 4 * 1024 * 1024,
            "系统检查返回过多数据，已停止。"
        );
        Ok(output.stdout)
    })
}

/// Hub stays running and cooperates through codex_access; only native clients block.
pub fn blocking_processes() -> Result<Vec<(u32, String)>> {
    let mut found = Vec::new();
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
            System::Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
        };
        unsafe {
            let handle = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            ensure!(
                handle != INVALID_HANDLE_VALUE,
                "无法检查正在运行的应用，已停止修复。"
            );
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut valid = Process32FirstW(handle, &mut entry);
            if valid == 0 {
                CloseHandle(handle);
                bail!("无法读取正在运行的应用列表，已停止修复。");
            }
            while valid != 0 {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&ch| ch == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if blocks_repair(entry.th32ProcessID, &name) {
                    found.push((entry.th32ProcessID, name));
                }
                valid = Process32NextW(handle, &mut entry);
            }
            CloseHandle(handle);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let output = system_output("/bin/ps", &["-axo", "pid=,comm="])?;
        for line in String::from_utf8_lossy(&output).lines() {
            let line = line.trim();
            if let Some((pid, name)) = line.split_once(char::is_whitespace) {
                if let Ok(pid) = pid.parse() {
                    let name = Path::new(name.trim())
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    if blocks_repair(pid, &name) {
                        found.push((pid, name.into_owned()));
                    }
                }
            }
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    bail!("此修复入口仅支持 Windows 和 macOS。");
    Ok(found)
}

fn blocks_repair(pid: u32, name: &str) -> bool {
    pid != std::process::id()
        && matches!(
            name.to_ascii_lowercase().trim_end_matches(".exe"),
            "codex" | "chatgpt" | "codex-code-mode-host"
        )
}

fn require_stopped() -> Result<()> {
    let processes = blocking_processes()?;
    ensure!(
        processes.is_empty(),
        "请先完全退出 ChatGPT/Codex（包括托盘和后台进程），关闭后点击“重新检查”。Hub 保持打开。仍在运行：{}",
        processes
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join("、")
    );
    Ok(())
}

fn desktop_env(name: &str) -> Result<Option<String>> {
    #[cfg(windows)]
    {
        use winreg::{
            RegKey,
            enums::{HKEY_CURRENT_USER, KEY_READ},
        };
        let key = match RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Environment", KEY_READ)
        {
            Ok(key) => key,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        match key.get_value::<String, _>(name) {
            Ok(value) => return Ok(Some(value)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        }
    }
    #[cfg(target_os = "macos")]
    {
        return runtime()?.block_on(async {
            let mut command = Command::new("/bin/launchctl");
            command.args(["getenv", name]).kill_on_drop(true);
            let output = tokio::time::timeout(Duration::from_secs(12), command.output())
                .await
                .context("桌面环境检查超时，未修改文件。")??;
            // launchctl exits 1 with no stdout for an unset variable.
            if output.status.code() == Some(1) && output.stdout.is_empty() {
                return Ok(None);
            }
            ensure!(
                output.status.success() && output.stdout.len() < 1024 * 1024,
                "无法检查桌面环境变量，未修改文件。"
            );
            Ok(
                Some(String::from_utf8(output.stdout)?.trim().to_string())
                    .filter(|s| !s.is_empty()),
            )
        });
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = name;
        Ok(None)
    }
}

fn env_path(name: &str) -> Result<Option<PathBuf>> {
    let shell = std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let desktop = desktop_env(name)?.filter(|value| !value.trim().is_empty());
    let values = [shell, desktop]
        .into_iter()
        .flatten()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    for path in &values {
        ensure!(
            path.is_absolute(),
            "{name} 使用了相对路径，无法自动判断实际用户目录；未修改文件。"
        );
    }
    if values.len() == 2 {
        ensure!(
            values[0] == values[1]
                || fs::canonicalize(&values[0])
                    .ok()
                    .zip(fs::canonicalize(&values[1]).ok())
                    .is_some_and(|(a, b)| a == b),
            "{name} 的桌面与当前进程设置不一致，未修改文件。请导出诊断进一步确认。"
        );
    }
    Ok(values.into_iter().next())
}

fn locations() -> Result<(PathBuf, PathBuf)> {
    let home = env_path("CODEX_HOME")?.unwrap_or_else(crate::codex_app_config::default_codex_home);
    ensure!(
        home.is_dir(),
        "尚未找到 Codex 用户目录。请确认已安装并至少打开过 Codex。"
    );
    let home = home.canonicalize()?;
    let config = home.join("config.toml");
    let configured_sqlite = if config.is_file() {
        let raw = fs::read_to_string(config)?;
        let config = raw
            .trim_start_matches('\u{feff}')
            .parse::<toml::Table>()
            .context("Codex 配置格式有误，未修改配置。请导出连接诊断包进一步检查。")?;
        match config.get("sqlite_home") {
            None => None,
            Some(value) => Some(PathBuf::from(
                value
                    .as_str()
                    .context("sqlite_home 不是文本路径，已停止修复。")?,
            )),
        }
    } else {
        None
    };
    let directory = match configured_sqlite {
        Some(path) => path,
        None => env_path("CODEX_SQLITE_HOME")?.unwrap_or_else(|| home.clone()),
    };
    ensure!(
        directory.is_absolute(),
        "状态库使用相对路径，无法自动确定位置；未修改文件。"
    );
    ensure!(
        directory.is_dir(),
        "未找到实际状态库目录，未修改文件。请导出连接诊断包进一步检查。"
    );
    Ok((home, directory.canonicalize()?.join(DB)))
}

fn locate_cli() -> Result<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    {
        let source = "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); @(Get-AppxPackage | Where-Object {$_.Name -in @('OpenAI.Codex','OpenAI.ChatGPT')} | Select-Object -ExpandProperty InstallLocation) | ConvertTo-Json -Compress";
        let raw = system_output(
            "powershell.exe",
            &["-NoProfile", "-NonInteractive", "-Command", source],
        )?;
        let parsed: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
        let roots = match parsed {
            Value::String(s) => vec![s],
            Value::Array(items) => items
                .into_iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect(),
            _ => vec![],
        };
        for root in roots {
            for rel in [
                "app/resources/codex.exe",
                "resources/codex.exe",
                "app/resources/codex-cli/codex.exe",
            ] {
                candidates.push(PathBuf::from(&root).join(rel));
            }
        }
    }
    #[cfg(target_os = "macos")]
    for directory in [
        PathBuf::from("/Applications"),
        crate::codex_app_config::default_codex_home()
            .parent()
            .unwrap_or(Path::new("/nonexistent"))
            .join("Applications"),
    ] {
        for app in ["ChatGPT.app", "Codex.app"] {
            for rel in [
                "Contents/Resources/codex",
                "Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex",
            ] {
                candidates.push(directory.join(app).join(rel));
            }
        }
    }
    candidates = candidates
        .into_iter()
        .filter(|p| p.is_file())
        .map(fs::canonicalize)
        .collect::<std::io::Result<Vec<_>>>()?;
    candidates.sort();
    candidates.dedup();
    ensure!(
        !candidates.is_empty(),
        "未找到桌面应用的内置程序。请确认 Codex/ChatGPT 已安装；本次没有修改用户文件。"
    );
    ensure!(
        candidates.len() == 1,
        "检测到多份 Codex/ChatGPT 内置程序，无法确认当前使用哪一份；未修改文件。请导出连接诊断进一步确认。"
    );
    let cli = candidates.remove(0);
    if let Some(override_path) = env_path("CODEX_CLI_PATH")? {
        ensure!(
            override_path.canonicalize().ok().as_ref() == Some(&cli),
            "CODEX_CLI_PATH 指向了其他程序或失效路径，可能导致组织设置加载失败。本次没有改全局环境变量或状态库，请导出连接诊断进一步确认。"
        );
    }
    Ok(cli)
}

fn family(source: &Path) -> Vec<PathBuf> {
    ["", "-wal", "-shm"]
        .iter()
        .map(|suffix| {
            source.with_file_name(format!(
                "{}{}",
                source.file_name().unwrap().to_string_lossy(),
                suffix
            ))
        })
        .collect()
}

fn hash(path: &Path) -> Result<String> {
    let mut stream = File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut digest = Sha256::new();
    loop {
        let size = stream.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn fingerprint(source: &Path) -> Result<Hashes> {
    ensure!(
        !source.with_file_name(format!("{DB}-journal")).exists(),
        "检测到崩溃恢复用的回滚日志，已停止自动处理，请保留原库进一步诊断。"
    );
    let mut hashes = Hashes::new();
    for path in family(source) {
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                ensure!(
                    meta.is_file() && !meta.file_type().is_symlink(),
                    "状态库或边车不是普通文件，停止自动修复。"
                );
                hashes.insert(
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    hash(&path)?,
                );
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(hashes)
}

fn private_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)?;
    }
    #[cfg(not(unix))]
    fs::create_dir(path)?;
    Ok(())
}

fn copy_family(source: &Path, directory: &Path) -> Result<()> {
    private_directory(directory)?;
    for path in family(source) {
        if path.is_file() {
            fs::copy(&path, directory.join(path.file_name().unwrap()))?;
        }
    }
    Ok(())
}

fn open_copy(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE,
    )?)
}

fn snapshot(source: &Path, destination: &Path) -> Result<()> {
    let source = open_copy(source)?;
    let mut destination = Connection::open(destination)?;
    let backup = Backup::new(&source, &mut destination)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        use rusqlite::backup::StepResult;
        match backup.step(256)? {
            StepResult::Done => break,
            StepResult::More => {}
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
        ensure!(
            Instant::now() < deadline,
            "状态库副本生成超时，原库未改动。"
        );
    }
    Ok(())
}

fn inspect(path: &Path) -> Result<State> {
    let db = open_copy(path)?;
    let quick: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    ensure!(
        quick == "ok",
        "状态库完整性检查失败，不会自动清空历史。请保留原库进一步诊断。"
    );
    let ledger: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations')",
        [],
        |r| r.get(0),
    )?;
    ensure!(ledger, "缺少原生迁移记录，无法确认状态库来源；未修改原库。");
    let receipts = db
        .prepare("SELECT version, success FROM _sqlx_migrations")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        receipts.iter().all(|(_, ok)| *ok),
        "存在未完成的原生迁移，停止自动处理。请保留诊断资料。"
    );
    let versions: HashSet<_> = receipts.into_iter().map(|(v, _)| v).collect();
    let max = versions.iter().copied().max().unwrap_or_default();
    let columns = columns(&db)?;
    let conflict = if !columns.is_empty() && !versions.contains(&24) && max < 24 {
        Some(Conflict::EarlyTable)
    } else if columns.contains(&ENABLED.to_string())
        && versions.contains(&24)
        && !versions.contains(&37)
        && max < 37
    {
        Some(Conflict::EarlyColumn)
    } else {
        None
    };
    Ok(State {
        conflict,
        native24: versions.contains(&24),
        native37: versions.contains(&37),
    })
}

fn columns(db: &Connection) -> Result<Vec<String>> {
    Ok(db
        .prepare("PRAGMA table_info(remote_control_enrollments)")?
        .query_map([], |r| r.get(1))?
        .collect::<rusqlite::Result<_>>()?)
}

fn stage(path: &Path, conflict: Conflict) -> Result<(Vec<String>, Rows)> {
    let mut db = open_copy(path)?;
    let names = columns(&db)?;
    let base = BASE.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut extended = base.clone();
    extended.push(ENABLED.to_string());
    ensure!(
        names == base || names == extended,
        "远程控制表结构不属于已确认范围，原库未修改。"
    );
    let definitions = db
        .prepare("PRAGMA table_info(remote_control_enrollments)")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (index, (name, kind, not_null, default, pk)) in definitions.iter().enumerate() {
        ensure!(
            kind.eq_ignore_ascii_case(if name == ENABLED || name == "updated_at" {
                "INTEGER"
            } else {
                "TEXT"
            }) && *not_null == (index < 7)
                && default.is_none()
                && *pk == if index < 3 { index as i64 + 1 } else { 0 },
            "远程控制表类型或约束发生变化，原库未修改。"
        );
    }
    let ddl: String = db.query_row(
        "SELECT sql FROM sqlite_master WHERE name=?1 AND type='table'",
        [TABLE],
        |r| r.get(0),
    )?;
    ensure!(!regex::Regex::new(r"(?i)\b(CHECK|REFERENCES|GENERATED|STRICT|WITHOUT|UNIQUE|COLLATE|CONFLICT|DEFERRABLE|ASC|DESC|AUTOINCREMENT)\b")?.is_match(&ddl), "远程控制表存在额外约束，停止自动修复。");
    let custom: i64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE tbl_name=?1 AND type IN ('trigger','index') AND sql IS NOT NULL", [TABLE], |r| r.get(0))?;
    ensure!(
        custom == 0,
        "远程控制表存在自定义索引或触发器，停止自动修复。"
    );
    let tables = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for table in tables {
        let query = format!(
            "PRAGMA foreign_key_list(\"{}\")",
            table.replace('"', "\"\"")
        );
        let references = db
            .prepare(&query)?
            .query_map([], |r| r.get::<_, String>(2))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ensure!(
            !references.iter().any(|name| name == TABLE),
            "存在其他表引用，停止自动修复。"
        );
    }
    let rows = read_remote_rows(&db, &names)?;
    let transaction = db.transaction()?;
    transaction.execute_batch("DROP TABLE remote_control_enrollments")?;
    if conflict == Conflict::EarlyColumn {
        transaction.execute_batch(DDL)?;
        for row in &rows {
            transaction.execute(
                "INSERT INTO remote_control_enrollments VALUES (?,?,?,?,?,?,?)",
                params_from_iter(row.iter().take(7)),
            )?;
        }
    }
    transaction.commit()?;
    Ok((names, rows))
}

fn read_remote_rows(db: &Connection, names: &[String]) -> Result<Rows> {
    Ok(db.prepare(&format!("SELECT {} FROM remote_control_enrollments ORDER BY websocket_url,account_id,app_server_client_name", names.join(",")))?.query_map([], |r| (0..names.len()).map(|i| r.get(i)).collect())?.collect::<rusqlite::Result<_>>()?)
}

fn restore_rows(path: &Path, names: &[String], rows: &Rows) -> Result<()> {
    let mut db = open_copy(path)?;
    ensure!(
        names
            .iter()
            .all(|name| columns(&db).is_ok_and(|cols| cols.contains(name))),
        "原生程序未建立所需结构，原库未修改。"
    );
    let sql = format!(
        "INSERT OR REPLACE INTO remote_control_enrollments ({}) VALUES ({})",
        names.join(","),
        vec!["?"; names.len()].join(",")
    );
    let transaction = db.transaction()?;
    for row in rows {
        transaction.execute(&sql, params_from_iter(row))?;
    }
    transaction.commit()?;
    Ok(())
}

fn thread_ids(path: &Path) -> Result<HashSet<String>> {
    let db = open_copy(path)?;
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='threads')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(HashSet::new());
    }
    Ok(db
        .prepare("SELECT id FROM threads")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

async fn read_lines<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    sender: tokio::sync::mpsc::Sender<(bool, Vec<u8>)>,
    error: bool,
) -> bool {
    use tokio::io::AsyncReadExt;
    let mut reader = BufReader::new(reader).take(4 * 1024 * 1024 + 1);
    let mut total = 0;
    loop {
        let mut line = Vec::new();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) => return true,
            Err(_) => return false,
            Ok(size) => {
                total += size;
                if line.len() > 1024 * 1024 || total > 4 * 1024 * 1024 {
                    return false;
                }
                if sender.send((error, line)).await.is_err() {
                    return false;
                }
            }
        }
    }
}

fn is_sqlite_error(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("failed to initialize sqlite state runtime")
        || text.contains("failed to open state db")
        || (text.contains("remote_control_enrollments") && text.contains("already exists"))
        || (text.contains("duplicate column name") && text.contains(ENABLED))
}

#[derive(Debug, PartialEq)]
enum Reply {
    Ignore,
    ReadConfig,
    Complete,
    Failed(bool),
}

fn reply(line: &[u8], expected: i64) -> Reply {
    let Ok(response) = serde_json::from_slice::<Value>(line) else {
        return Reply::Ignore;
    };
    if response.get("id").and_then(Value::as_i64) != Some(expected) {
        return Reply::Ignore;
    }
    if let Some(error) = response.get("error") {
        return Reply::Failed(is_sqlite_error(&error.to_string()));
    }
    if !response["result"].is_object() {
        return Reply::Failed(false);
    }
    match expected {
        1 if response["result"]["userAgent"].is_string() => Reply::ReadConfig,
        2 => Reply::Complete,
        _ => Reply::Failed(false),
    }
}

fn probe(cli: &Path, home: &Path, sqlite: &Path) -> Result<Probe> {
    runtime()?.block_on(async {
        let mut command = Command::new(cli);
        command.args(["-c", &format!("sqlite_home={}", serde_json::to_string(&sqlite.to_string_lossy())?), "-c", &format!("log_dir={}", serde_json::to_string(&home.join("probe-logs").to_string_lossy())?), "app-server"])
            .env("CODEX_HOME", home).env("CODEX_SQLITE_HOME", sqlite).env("OTEL_SDK_DISABLED", "true").env("RUST_LOG", "warn")
            .current_dir(home).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(windows)] command.creation_flags(0x08000000);
        #[cfg(target_os = "macos")] command.process_group(0);
        let mut child = command.spawn().context("无法运行当前应用的内置检查程序，原库未修改。")?;
        let mut input = child.stdin.take().context("无法建立检查通道。")?;
        let (sender, mut receiver) = tokio::sync::mpsc::channel(128);
        let mut stdout = tokio::spawn(read_lines(child.stdout.take().unwrap(), sender.clone(), false));
        let mut stderr = tokio::spawn(read_lines(child.stderr.take().unwrap(), sender, true));
        let mut result = Probe::default();
        let protocol = async {
            input.write_all((json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"hub_startup_repair","title":"Hub Startup Repair","version":env!("CARGO_PKG_VERSION")}}}).to_string()+"\n").as_bytes()).await?;
            let mut expected = 1;
            while let Some((error,line)) = receiver.recv().await {
                if error { result.sqlite_failure |= is_sqlite_error(&String::from_utf8_lossy(&line)); continue; }
                match reply(&line, expected) {
                    Reply::Ignore => {},
                    Reply::ReadConfig => {
                        input.write_all(b"{\"method\":\"initialized\",\"params\":{}}\n{\"id\":2,\"method\":\"config/read\",\"params\":{\"includeLayers\":false}}\n").await?;
                        expected = 2;
                    },
                    Reply::Complete => { result.ok = true; break; },
                    Reply::Failed(sqlite) => { result.sqlite_failure |= sqlite; break; },
                }
            }
            Ok::<_, std::io::Error>(())
        };
        if !matches!(tokio::time::timeout(Duration::from_secs(30), protocol).await, Ok(Ok(()))) { result.ok = false; }
        drop(input);
        // Drain while waiting for exit: short stderr lines can fill the bounded
        // channel, and a final SQLite error may arrive after the JSON response.
        let shutdown = async {
            match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                Ok(Ok(status)) => return status.success(),
                Ok(Err(_)) => return false,
                Err(_) => {}
            }
            #[cfg(windows)]
            if let Some(pid) = child.id() {
                let mut kill = Command::new("taskkill.exe"); kill.args(["/PID", &pid.to_string(), "/T", "/F"]).creation_flags(0x08000000).kill_on_drop(true);
                let _ = tokio::time::timeout(Duration::from_secs(5), kill.output()).await;
            }
            #[cfg(target_os = "macos")]
            if let Some(pid) = child.id() { unsafe { libc::kill(-(pid as i32), libc::SIGTERM); } }
            matches!(tokio::time::timeout(Duration::from_secs(2), child.kill()).await, Ok(Ok(())))
        };
        let drain = async {
            let mut sqlite_failure = false;
            while let Some((error, line)) = receiver.recv().await {
                if error { sqlite_failure |= is_sqlite_error(&String::from_utf8_lossy(&line)); }
            }
            sqlite_failure
        };
        let (exited_ok, drained) = tokio::join!(shutdown, tokio::time::timeout(Duration::from_secs(10), drain));
        let readers = tokio::time::timeout(Duration::from_secs(1), async { tokio::join!(&mut stdout, &mut stderr) }).await;
        let clean = matches!(readers, Ok((Ok(true), Ok(true)))) && drained.is_ok();
        stdout.abort(); stderr.abort();
        ensure!(clean, "检查程序输出不完整或超过限制，已停止；原库未修改。");
        result.ok &= exited_ok;
        result.sqlite_failure |= drained.unwrap_or_default();
        if result.sqlite_failure { result.ok = false; }
        Ok(result)
    })
}

fn write_manifest(directory: &Path, manifest: &Manifest) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer_pretty(&mut file, manifest)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(directory.join("manifest.json"))?;
    Ok(())
}

fn promote(
    source: &Path,
    candidate: &Path,
    backup: &Path,
    manifest: &mut Manifest,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    check()?;
    ensure!(
        fingerprint(source)? == manifest.original,
        "原库在检查后发生变化，停止替换。"
    );
    let mut pending = tempfile::NamedTempFile::new_in(source.parent().unwrap())?;
    std::io::copy(&mut File::open(candidate)?, &mut pending)?;
    pending.as_file().sync_all()?;
    ensure!(
        hash(pending.path())? == hash(candidate)?,
        "候选库复制校验失败，原库未修改。"
    );
    check()?;
    ensure!(
        fingerprint(source)? == manifest.original,
        "原库发生变化，已取消替换。"
    );
    let displaced = backup.join("displaced");
    private_directory(&displaced)?;
    manifest.stage = "replacing".into();
    write_manifest(backup, manifest)?;
    let mut moved = Vec::new();
    let mut installed = false;
    let attempt = (|| -> Result<()> {
        for path in family(source) {
            if path.exists() {
                fs::rename(&path, displaced.join(path.file_name().unwrap()))?;
                moved.push(path);
            }
        }
        pending.persist(source)?;
        installed = true;
        manifest.installed = Some(fingerprint(source)?);
        manifest.stage = "applied".into();
        write_manifest(backup, manifest)?;
        Ok(())
    })();
    if let Err(error) = attempt {
        if installed {
            fs::rename(source, backup.join("failed-candidate.sqlite"))?;
        }
        for path in moved.iter().rev() {
            ensure!(
                !path.exists(),
                "恢复目标被其他程序占用，请保留备份目录进一步处理。"
            );
            fs::rename(displaced.join(path.file_name().unwrap()), path)?;
        }
        manifest.stage = "rolled-back".into();
        write_manifest(backup, manifest)?;
        return Err(error);
    }
    Ok(())
}

pub fn run(mut progress: impl FnMut(&str)) -> Result<RepairOutcome> {
    let _access = crate::codex_access::repair()?;
    require_stopped()?;
    progress("正在定位 Codex 和检查状态库…");
    let (home, source) = locations()?;
    let cli = locate_cli()?;
    if !source.is_file() {
        return Ok(RepairOutcome {
            message:
                "未找到本工具支持的状态库，没有修改文件。如果仍然报错，请导出连接诊断包进一步检查。"
                    .into(),
            backup: None,
            applied: false,
        });
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(source.parent().unwrap().join("hub-startup-repair.lock"))?;
    lock.try_lock_exclusive()
        .context("已有修复任务在执行，请等待完成。")?;
    let hashes = fingerprint(&source)?;
    let work = tempfile::tempdir()?;
    copy_family(&source, &work.path().join("inspect"))?;
    ensure!(
        fingerprint(&source)? == hashes,
        "检查期间状态库发生变化，请退出相关程序后重新检查。"
    );
    let state = inspect(&work.path().join("inspect").join(DB))?;
    let Some(conflict) = state.conflict else {
        return Ok(RepairOutcome { message:"未发现已知的状态库迁移冲突，没有修改原文件。若仍然无法加载组织设置，请导出连接诊断包进一步检查。".into(), backup:None, applied:false});
    };
    let parent = source.parent().unwrap().join(BACKUPS);
    if !parent.exists() {
        private_directory(&parent)?;
    }
    let backup = parent.join(format!(
        "{}-{}",
        crate::types::now_ms(),
        uuid::Uuid::new_v4().simple()
    ));
    private_directory(&backup)?;
    let result = repair(
        &home,
        &source,
        &cli,
        conflict,
        &hashes,
        &backup,
        &mut progress,
        &require_stopped,
        &probe,
    );
    match result {
        Ok(applied) => Ok(RepairOutcome {
            message: if applied {
                "修复完成，已保留原会话索引和远程控制记录。关闭此窗口后，可使用增强启动打开 ChatGPT/Codex。"
            } else {
                "当前内置程序能够读取原库副本，未复现需修复的冲突，没有替换原文件。"
            }
            .into(),
            backup: Some(backup),
            applied,
        }),
        Err(error) => bail!("{}\n备份目录：{}", error, backup.display()),
    }
}

#[allow(clippy::too_many_arguments)]
fn repair(
    home: &Path,
    source: &Path,
    cli: &Path,
    conflict: Conflict,
    expected: &Hashes,
    backup: &Path,
    progress: &mut dyn FnMut(&str),
    check: &dyn Fn() -> Result<()>,
    probe: &dyn Fn(&Path, &Path, &Path) -> Result<Probe>,
) -> Result<bool> {
    check()?;
    ensure!(
        &fingerprint(source)? == expected,
        "原库在诊断后发生变化，停止修复。"
    );
    progress("已定位迁移冲突，正在完整备份状态库…");
    copy_family(source, &backup.join("original"))?;
    ensure!(
        &fingerprint(&backup.join("original").join(DB))? == expected
            && &fingerprint(source)? == expected,
        "备份校验未通过，原库保持不变。"
    );
    let mut manifest = Manifest {
        version: 1,
        source: source.into(),
        original: expected.clone(),
        installed: None,
        stage: "backed-up".into(),
        conflict,
    };
    write_manifest(backup, &manifest)?;
    let work = tempfile::tempdir()?;
    let probe_home = work.path().join("home");
    private_directory(&probe_home)?;
    for file in ["config.toml", "auth.json", "installation_id"] {
        if home.join(file).is_file() {
            fs::copy(home.join(file), probe_home.join(file))?;
        }
    }
    copy_family(&backup.join("original").join(DB), &work.path().join("seed"))?;
    let baseline = work.path().join("baseline");
    private_directory(&baseline)?;
    snapshot(&work.path().join("seed").join(DB), &baseline.join(DB))?;
    let threads = thread_ids(&baseline.join(DB))?;
    progress("正在副本上确认启动错误，原文件保持不变…");
    let first = probe(cli, &probe_home, &baseline)?;
    if first.ok {
        return Ok(false);
    }
    ensure!(
        first.sqlite_failure,
        "副本未确认是状态库启动错误，原库保持不变。请导出连接诊断进一步检查。"
    );
    let staged = work.path().join("staged");
    private_directory(&staged)?;
    snapshot(&work.path().join("seed").join(DB), &staged.join(DB))?;
    let (names, rows) = stage(&staged.join(DB), conflict)?;
    progress("正在修复副本，并让当前内置程序完成升级…");
    ensure!(
        probe(cli, &probe_home, &staged)?.ok,
        "修复副本未通过启动检查，原库保持不变。"
    );
    let state = inspect(&staged.join(DB))?;
    ensure!(
        state.native24 && (!names.contains(&ENABLED.to_string()) || state.native37),
        "原生程序未完成所需迁移，原库保持不变。"
    );
    restore_rows(&staged.join(DB), &names, &rows)?;
    ensure!(
        threads.is_subset(&thread_ids(&staged.join(DB))?),
        "副本未完整保留会话索引，原库保持不变。"
    );
    progress("正在核对历史记录和修复结果…");
    ensure!(
        probe(cli, &probe_home, &staged)?.ok,
        "恢复记录后副本启动未通过，原库保持不变。"
    );
    inspect(&staged.join(DB))?;
    ensure!(
        threads.is_subset(&thread_ids(&staged.join(DB))?),
        "最终副本未完整保留会话索引，原库保持不变。"
    );
    ensure!(
        read_remote_rows(&open_copy(&staged.join(DB))?, &names)? == rows,
        "最终副本未完整保留远程控制记录，原库保持不变。"
    );
    let candidate = work.path().join("candidate.sqlite");
    snapshot(&staged.join(DB), &candidate)?;
    progress("副本检查通过，正在应用修复…");
    promote(source, &candidate, backup, &mut manifest, check)?;
    Ok(true)
}

pub fn restore(backup: &Path) -> Result<()> {
    let _access = crate::codex_access::repair()?;
    require_stopped()?;
    restore_checked(backup, &require_stopped)
}

fn restore_checked(backup: &Path, check: &dyn Fn() -> Result<()>) -> Result<()> {
    let backup = backup.canonicalize()?;
    let mut manifest: Manifest = serde_json::from_slice(&fs::read(backup.join("manifest.json"))?)?;
    let source = &manifest.source;
    ensure!(
        manifest.version == 1
            && manifest.stage == "applied"
            && source.file_name().is_some_and(|name| name == DB)
            && source.is_absolute()
            && source.parent().unwrap().canonicalize()? == source.parent().unwrap()
            && backup.parent() == Some(source.parent().unwrap().join(BACKUPS).as_path()),
        "备份位置或状态不符合恢复要求，已停止。"
    );
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(source.parent().unwrap().join("hub-startup-repair.lock"))?;
    lock.try_lock_exclusive()
        .context("已有修复任务在执行，请等待完成。")?;
    check()?;
    ensure!(
        manifest.installed.as_ref() == Some(&fingerprint(source)?),
        "修复后状态库发生变化，可能已有新记录；不会覆盖，请保留备份进一步核对。"
    );
    let displaced = backup.join("displaced");
    ensure!(
        fingerprint(&displaced.join(DB))? == manifest.original,
        "原库备份哈希核对失败，已停止恢复。"
    );
    let current = backup.join("before-restore");
    private_directory(&current)?;
    manifest.stage = "restoring".into();
    write_manifest(&backup, &manifest)?;
    let mut moved_new = Vec::new();
    let mut moved_old = Vec::new();
    let attempt = (|| -> Result<()> {
        for path in family(source) {
            if path.exists() {
                fs::rename(&path, current.join(path.file_name().unwrap()))?;
                moved_new.push(path);
            }
        }
        for path in family(source) {
            let old = displaced.join(path.file_name().unwrap());
            if old.exists() {
                fs::rename(old, &path)?;
                moved_old.push(path);
            }
        }
        Ok(())
    })();
    if let Err(error) = attempt {
        for path in moved_old.iter().rev() {
            fs::rename(path, displaced.join(path.file_name().unwrap()))?;
        }
        for path in moved_new.iter().rev() {
            fs::rename(current.join(path.file_name().unwrap()), path)?;
        }
        manifest.stage = "applied".into();
        write_manifest(&backup, &manifest)?;
        fs::remove_dir(&current)?;
        return Err(error);
    }
    manifest.stage = "restored".into();
    write_manifest(&backup, &manifest)
        .context("原文件已恢复，但备份清单更新失败。请保留备份目录，勿重复恢复。")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Fixture {
        _root: tempfile::TempDir,
        home: PathBuf,
        source: PathBuf,
        backup: PathBuf,
        conflict: Conflict,
    }

    impl Fixture {
        fn new(native: i64, wal: bool) -> Self {
            let root = tempfile::tempdir().unwrap();
            let base = root.path().canonicalize().unwrap();
            let seed = base.join("seed.sqlite");
            let db = Connection::open(&seed).unwrap();
            if wal {
                db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
                    .unwrap();
            }
            db.execute_batch("CREATE TABLE _sqlx_migrations (version INTEGER PRIMARY KEY, success INTEGER NOT NULL); CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT); INSERT INTO threads VALUES ('existing-thread','synthetic history');").unwrap();
            for version in 1..=native {
                db.execute("INSERT INTO _sqlx_migrations VALUES (?1,1)", [version])
                    .unwrap();
            }
            db.execute_batch(DDL).unwrap();
            db.execute_batch("ALTER TABLE remote_control_enrollments ADD COLUMN remote_control_enabled INTEGER; INSERT INTO remote_control_enrollments VALUES ('ws://127.0.0.1/test','synthetic-account','','server','env','Hub',123,1);").unwrap();
            let home = base.join("home");
            // Include committed WAL records without asking SQLite to open the
            // copied physical files. No actual Codex state or process is used.
            copy_family(&seed, &home).unwrap();
            for (old, new) in family(&seed).into_iter().zip(family(&home.join(DB))) {
                let copied = home.join(old.file_name().unwrap());
                if copied.exists() {
                    fs::rename(copied, new).unwrap();
                }
            }
            drop(db);
            let source = home.join(DB);
            let parent = home.join(BACKUPS);
            private_directory(&parent).unwrap();
            let backup = parent.join("synthetic-test");
            private_directory(&backup).unwrap();
            Self {
                _root: root,
                home,
                source,
                backup,
                conflict: if native < 24 {
                    Conflict::EarlyTable
                } else {
                    Conflict::EarlyColumn
                },
            }
        }

        fn repair_with(
            &self,
            probe: &dyn Fn(&Path, &Path, &Path) -> Result<Probe>,
            check: &dyn Fn() -> Result<()>,
        ) -> Result<bool> {
            repair(
                &self.home,
                &self.source,
                Path::new("never-executed-codex"),
                self.conflict,
                &fingerprint(&self.source)?,
                &self.backup,
                &mut |_| {},
                check,
                probe,
            )
        }

        fn repair(&self) -> Result<bool> {
            let calls = Cell::new(0);
            self.repair_with(
                &|_, _, directory| {
                    let call = calls.replace(calls.get() + 1);
                    if call == 0 {
                        return Ok(Probe {
                            ok: false,
                            sqlite_failure: true,
                        });
                    }
                    migrate_copy(directory)
                },
                &|| Ok(()),
            )
        }
    }

    // Simulate ONLY native schema ownership in temporary test databases.
    fn migrate_copy(directory: &Path) -> Result<Probe> {
        let path = directory.join(DB);
        let state = inspect(&path)?;
        let db = open_copy(&path)?;
        if !state.native24 {
            db.execute_batch(DDL)?;
            db.execute("INSERT INTO _sqlx_migrations VALUES (24,1)", [])?;
        }
        if !state.native37 {
            db.execute_batch(
                "ALTER TABLE remote_control_enrollments ADD COLUMN remote_control_enabled INTEGER",
            )?;
            db.execute("INSERT INTO _sqlx_migrations VALUES (37,1)", [])?;
        }
        Ok(Probe {
            ok: true,
            sqlite_failure: false,
        })
    }

    #[test]
    fn hub_can_stay_open_but_codex_must_close() {
        let other = std::process::id().wrapping_add(1);
        for name in ["TianCaiSpaceHub.exe", "TianCaiSpaceHub", "CodexHub"] {
            assert!(!blocks_repair(other, name));
        }
        for name in ["Codex.exe", "ChatGPT", "codex-code-mode-host.exe"] {
            assert!(blocks_repair(other, name));
        }
    }

    #[test]
    fn migration_count_alone_never_triggers_repair() {
        for version in [23, 42] {
            let fixture = Fixture::new(version, false);
            if version == 23 {
                open_copy(&fixture.source)
                    .unwrap()
                    .execute_batch("DROP TABLE remote_control_enrollments")
                    .unwrap();
            }
            assert!(inspect(&fixture.source).unwrap().conflict.is_none());
        }
    }

    #[test]
    fn unfinished_migration_is_rejected() {
        let fixture = Fixture::new(23, false);
        open_copy(&fixture.source)
            .unwrap()
            .execute_batch("UPDATE _sqlx_migrations SET success=0 WHERE version=23")
            .unwrap();
        assert!(inspect(&fixture.source).is_err());
    }

    #[test]
    fn both_conflicts_preserve_history_preferences_and_restore_exact_bytes() {
        for native in [23, 36] {
            let fixture = Fixture::new(native, false);
            let original = fingerprint(&fixture.source).unwrap();
            assert!(fixture.repair().unwrap());
            let state = inspect(&fixture.source).unwrap();
            assert!(state.native24 && state.native37 && state.conflict.is_none());
            assert!(
                thread_ids(&fixture.source)
                    .unwrap()
                    .contains("existing-thread")
            );
            let enabled: i64 = open_copy(&fixture.source)
                .unwrap()
                .query_row(
                    "SELECT remote_control_enabled FROM remote_control_enrollments",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(enabled, 1);
            assert_eq!(
                fingerprint(&fixture.backup.join("original").join(DB)).unwrap(),
                original
            );
            restore_checked(&fixture.backup, &|| Ok(())).unwrap();
            assert_eq!(fingerprint(&fixture.source).unwrap(), original);
        }
    }

    #[test]
    fn committed_wal_and_physical_backup_are_preserved() {
        let fixture = Fixture::new(23, true);
        let original = fingerprint(&fixture.source).unwrap();
        assert!(original.contains_key("state_5.sqlite-wal"));
        assert!(fixture.repair().unwrap());
        assert!(
            thread_ids(&fixture.source)
                .unwrap()
                .contains("existing-thread")
        );
        assert_eq!(
            fingerprint(&fixture.backup.join("original").join(DB)).unwrap(),
            original
        );
        restore_checked(&fixture.backup, &|| Ok(())).unwrap();
        assert_eq!(fingerprint(&fixture.source).unwrap(), original);
    }

    #[test]
    fn already_working_baseline_does_not_replace_source() {
        let fixture = Fixture::new(23, false);
        let original = fingerprint(&fixture.source).unwrap();
        assert!(
            !fixture
                .repair_with(
                    &|_, _, _| Ok(Probe {
                        ok: true,
                        sqlite_failure: false
                    }),
                    &|| Ok(())
                )
                .unwrap()
        );
        assert_eq!(fingerprint(&fixture.source).unwrap(), original);
        assert!(!fixture.backup.join("displaced").exists());
    }

    #[test]
    fn unknown_startup_failure_never_promotes() {
        let fixture = Fixture::new(23, false);
        let original = fingerprint(&fixture.source).unwrap();
        assert!(
            fixture
                .repair_with(&|_, _, _| Ok(Probe::default()), &|| Ok(()))
                .is_err()
        );
        assert_eq!(fingerprint(&fixture.source).unwrap(), original);
    }

    #[test]
    fn failed_repaired_probe_never_promotes() {
        let fixture = Fixture::new(23, false);
        let original = fingerprint(&fixture.source).unwrap();
        assert!(
            fixture
                .repair_with(
                    &|_, _, _| Ok(Probe {
                        ok: false,
                        sqlite_failure: true
                    }),
                    &|| Ok(())
                )
                .is_err()
        );
        assert_eq!(fingerprint(&fixture.source).unwrap(), original);
    }

    #[test]
    fn extra_structure_is_rejected_without_changing_source() {
        for change in [
            "ALTER TABLE remote_control_enrollments ADD COLUMN future_field TEXT",
            "CREATE INDEX custom_index ON remote_control_enrollments(server_id)",
            "CREATE TABLE child (parent TEXT REFERENCES remote_control_enrollments(websocket_url))",
        ] {
            let fixture = Fixture::new(23, false);
            open_copy(&fixture.source)
                .unwrap()
                .execute_batch(change)
                .unwrap();
            let original = fingerprint(&fixture.source).unwrap();
            assert!(fixture.repair().is_err());
            assert_eq!(fingerprint(&fixture.source).unwrap(), original);
        }
    }

    #[test]
    fn restarted_codex_stops_promotion() {
        let fixture = Fixture::new(23, false);
        let original = fingerprint(&fixture.source).unwrap();
        let calls = Cell::new(0);
        let checks = Cell::new(0);
        let result = fixture.repair_with(
            &|_, _, directory| {
                if calls.replace(calls.get() + 1) == 0 {
                    Ok(Probe {
                        ok: false,
                        sqlite_failure: true,
                    })
                } else {
                    migrate_copy(directory)
                }
            },
            &|| {
                if checks.replace(checks.get() + 1) == 0 {
                    Ok(())
                } else {
                    bail!("synthetic Codex restart")
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(fingerprint(&fixture.source).unwrap(), original);
    }

    #[test]
    fn changed_source_stops_promotion() {
        let fixture = Fixture::new(23, false);
        let calls = Cell::new(0);
        let result = fixture.repair_with(
            &|_, _, directory| {
                let call = calls.replace(calls.get() + 1);
                if call == 0 {
                    return Ok(Probe {
                        ok: false,
                        sqlite_failure: true,
                    });
                }
                if call == 2 {
                    open_copy(&fixture.source)?.execute_batch(
                        "INSERT INTO threads VALUES ('new-thread','concurrent write')",
                    )?;
                }
                migrate_copy(directory)
            },
            &|| Ok(()),
        );
        assert!(result.is_err());
        assert!(thread_ids(&fixture.source).unwrap().contains("new-thread"));
        assert!(!fixture.backup.join("displaced").exists());
    }

    #[test]
    fn restore_never_overwrites_new_records_or_altered_backup() {
        let fixture = Fixture::new(23, false);
        fixture.repair().unwrap();
        open_copy(&fixture.source)
            .unwrap()
            .execute_batch("INSERT INTO threads VALUES ('new-thread','new history')")
            .unwrap();
        let changed = fingerprint(&fixture.source).unwrap();
        assert!(restore_checked(&fixture.backup, &|| Ok(())).is_err());
        assert_eq!(fingerprint(&fixture.source).unwrap(), changed);
        let fixture = Fixture::new(23, false);
        fixture.repair().unwrap();
        fs::write(fixture.backup.join("displaced").join(DB), b"changed backup").unwrap();
        let installed = fingerprint(&fixture.source).unwrap();
        assert!(restore_checked(&fixture.backup, &|| Ok(())).is_err());
        assert_eq!(fingerprint(&fixture.source).unwrap(), installed);
    }

    #[test]
    fn hot_journal_is_not_automatically_repaired() {
        let fixture = Fixture::new(23, false);
        fs::write(
            fixture.home.join("state_5.sqlite-journal"),
            b"recovery needed",
        )
        .unwrap();
        assert!(fingerprint(&fixture.source).is_err());
    }

    #[test]
    fn protocol_requires_matching_ids_object_results_and_no_error() {
        assert_eq!(
            reply(br#"{"id":99,"result":{"userAgent":"test"}}"#, 1),
            Reply::Ignore
        );
        assert_eq!(
            reply(br#"{"id":1,"result":{"userAgent":"test"}}"#, 1),
            Reply::ReadConfig
        );
        assert_eq!(reply(br#"{"id":1,"result":{}}"#, 1), Reply::Failed(false));
        assert_eq!(reply(br#"{"id":2,"result":{}}"#, 2), Reply::Complete);
        assert_eq!(reply(br#"{"id":2,"result":null}"#, 2), Reply::Failed(false));
        assert_eq!(
            reply(
                br#"{"id":2,"error":{"message":"failed to initialize sqlite state runtime"}}"#,
                2
            ),
            Reply::Failed(true)
        );
        assert_eq!(
            reply(
                br#"{"id":2,"error":{"message":"policy error"},"result":{}}"#,
                2
            ),
            Reply::Failed(false)
        );
    }

    #[test]
    fn output_limits_are_errors_and_final_stderr_is_received() {
        runtime().unwrap().block_on(async {
            let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
            assert!(
                read_lines(
                    &b"failed to initialize sqlite state runtime\n"[..],
                    sender,
                    true
                )
                .await
            );
            let (stderr, line) = receiver.recv().await.unwrap();
            assert!(stderr && is_sqlite_error(&String::from_utf8_lossy(&line)));
            assert!(receiver.recv().await.is_none());
            let (sender, _) = tokio::sync::mpsc::channel(4);
            assert!(!read_lines(&vec![b'x'; 1024 * 1024 + 1][..], sender, false).await);
        });
    }
}
