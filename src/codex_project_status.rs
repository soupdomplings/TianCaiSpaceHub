//! Read-only summary of the current local Codex database. This is not the
//! signed-in account's visible sidebar and never reconstructs memberships.
use std::{
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ProjectState {
    Available,
    MissingDatabase,
    UnsupportedSchema,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectStatus {
    pub(crate) state: ProjectState,
    pub(crate) project_count: Option<u64>,
    pub(crate) grouped_thread_count: Option<u64>,
    /// Currently grouped, non-archived threads with an explicit, different
    /// provider. This does not prove a switch, or that a thread is hidden.
    pub(crate) other_provider_grouped_thread_count: Option<u64>,
}

impl ProjectStatus {
    fn unknown(state: ProjectState) -> Self {
        Self {
            state,
            project_count: None,
            grouped_thread_count: None,
            other_provider_grouped_thread_count: None,
        }
    }
}

fn settings(home: &Path, env_dir: Option<PathBuf>) -> Result<(PathBuf, Option<String>)> {
    let config = match fs::read_to_string(home.join("config.toml")) {
        Ok(raw) => raw.trim_start_matches('\u{feff}').parse::<toml::Table>()?,
        Err(e) if e.kind() == ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e.into()),
    };
    let directory = match config.get("sqlite_home") {
        Some(value) => PathBuf::from(value.as_str().context("sqlite_home must be a path")?),
        None => env_dir
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| home.to_owned()),
    };
    ensure!(directory.is_absolute(), "sqlite_home must be absolute");
    // A profile can select a different provider; do not guess its effective
    // provider or silently treat a missing setting as a provider change.
    let provider = config
        .get("model_provider")
        .and_then(|v| v.as_str())
        .filter(|p| !p.is_empty() && !config.contains_key("profile"))
        .map(str::to_owned);
    Ok((directory, provider))
}

pub(crate) fn sqlite_dir(codex_home: &Path) -> Result<PathBuf> {
    settings(
        codex_home,
        std::env::var_os("CODEX_SQLITE_HOME").map(Into::into),
    )
    .map(|(dir, _)| dir)
}

pub(crate) fn inspect(codex_home: &Path) -> ProjectStatus {
    inspect_in(
        codex_home,
        std::env::var_os("CODEX_SQLITE_HOME").map(Into::into),
    )
    .unwrap_or_else(|_| ProjectStatus::unknown(ProjectState::Unreadable))
}

fn inspect_in(home: &Path, env_dir: Option<PathBuf>) -> Result<ProjectStatus> {
    let (dir, provider) = settings(home, env_dir)?;
    let path = dir.join("state_5.sqlite");
    if !path.try_exists()? {
        for legacy in ["state.v2.sqlite", "state.sqlite", "codex.sqlite"] {
            if dir.join(legacy).try_exists()? {
                return Ok(ProjectStatus::unknown(ProjectState::UnsupportedSchema));
            }
        }
        return Ok(ProjectStatus::unknown(ProjectState::MissingDatabase));
    }
    let wal = dir.join("state_5.sqlite-wal");
    let shm = dir.join("state_5.sqlite-shm");
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let has_wal = wal.try_exists()?;
    // A normal read-only open of a checkpointed WAL database can still
    // create -wal/-shm. Use its checkpoint only when there is no live WAL;
    // a live database must have both existing sidecars and be read with locks.
    ensure!(has_wal == shm.try_exists()?, "incomplete WAL sidecars");
    // Byte 18 of the SQLite file header is 2 for a WAL-format database.
    // For rollback journals, use regular SQLite locking even if no journal
    // currently exists.
    let mut header = [0; 20];
    let wal_format = fs::File::open(&path)?.read(&mut header)? == header.len() && header[18] == 2;
    let checkpoint_only = !has_wal && wal_format;
    let mut db = if checkpoint_only {
        let mut uri = url::Url::from_file_path(&path)
            .map_err(|_| anyhow::anyhow!("invalid database path"))?;
        uri.query_pairs_mut().append_pair("immutable", "1");
        Connection::open_with_flags(uri.as_str(), flags | OpenFlags::SQLITE_OPEN_URI)?
    } else {
        Connection::open_with_flags(&path, flags)?
    };
    let tx = db.transaction()?;
    let has_schema: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('projects') WHERE name='id')
            AND (SELECT count(*) FROM pragma_table_info('threads') WHERE name IN ('archived','project_id'))=2",
        [], |r| r.get(0),
    )?;
    let result = if has_schema {
        let project_count = tx.query_row("SELECT count(*) FROM projects", [], |r| r.get(0))?;
        let has_provider: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('threads') WHERE name='model_provider')",
            [],
            |r| r.get(0),
        )?;
        let (grouped, other) = match provider.as_deref().filter(|_| has_provider) {
            Some(id) => tx.query_row(
                "SELECT count(*), count(CASE WHEN t.model_provider<>'' AND t.model_provider<>?1 THEN 1 END)
                 FROM threads t JOIN projects p ON p.id=t.project_id WHERE t.archived=0",
                [id], |r| Ok((r.get(0)?, Some(r.get(1)?))),
            )?,
            None => tx.query_row(
                "SELECT count(*) FROM threads t JOIN projects p ON p.id=t.project_id WHERE t.archived=0",
                [], |r| Ok((r.get(0)?, None)),
            )?,
        };
        ProjectStatus {
            state: ProjectState::Available,
            project_count: Some(project_count),
            grouped_thread_count: Some(grouped),
            other_provider_grouped_thread_count: other,
        }
    } else {
        ProjectStatus::unknown(ProjectState::UnsupportedSchema)
    };
    if checkpoint_only {
        ensure!(!wal.try_exists()?, "database changed during inspection");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(dir: &Path, wal: bool) -> Connection {
        let conn = Connection::open(dir.join("state_5.sqlite")).unwrap();
        if wal {
            conn.pragma_update(None, "journal_mode", "wal").unwrap();
        }
        conn.execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY);
            CREATE TABLE threads (project_id TEXT, archived INTEGER, model_provider TEXT);
            INSERT INTO projects VALUES ('one'),('two');
            INSERT INTO threads VALUES ('one',0,'ai-gateway'),('one',0,'openai'),('two',0,NULL),
            ('two',1,'openai'),(NULL,0,'openai'),('absent',0,'openai');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn configured_database_precedes_env_and_counts_existing_memberships_only() {
        let home = tempfile::tempdir().unwrap();
        let env_dir = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        let _writer = seed(custom.path(), true);
        fs::write(
            home.path().join("config.toml"),
            format!(
                "sqlite_home={}\nmodel_provider=\"ai-gateway\"",
                serde_json::to_string(custom.path()).unwrap()
            ),
        )
        .unwrap();
        let report = inspect_in(home.path(), Some(env_dir.path().to_owned())).unwrap();
        assert_eq!(report.state, ProjectState::Available);
        assert_eq!(
            (
                report.project_count,
                report.grouped_thread_count,
                report.other_provider_grouped_thread_count
            ),
            (Some(2), Some(3), Some(1))
        );
        assert!(!home.path().join("state_5.sqlite").exists());
        assert!(!env_dir.path().join("state_5.sqlite").exists());
    }

    #[test]
    fn checkpointed_wal_read_does_not_create_sidecars_and_unknown_provider_stays_unknown() {
        let home = tempfile::tempdir().unwrap();
        let database_dir = tempfile::tempdir().unwrap();
        drop(seed(database_dir.path(), true));
        let wal = database_dir.path().join("state_5.sqlite-wal");
        let shm = database_dir.path().join("state_5.sqlite-shm");
        assert!(!wal.exists() && !shm.exists());
        let report = inspect_in(home.path(), Some(database_dir.path().to_owned())).unwrap();
        assert_eq!(report.grouped_thread_count, Some(3));
        assert_eq!(report.other_provider_grouped_thread_count, None);
        assert!(!wal.exists() && !shm.exists());
    }

    #[test]
    fn missing_old_empty_and_unreadable_are_distinct_and_do_not_fall_back() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("state_5.sqlite");
        let read = || {
            inspect_in(home.path(), None)
                .unwrap_or_else(|_| ProjectStatus::unknown(ProjectState::Unreadable))
        };
        assert_eq!(
            read(),
            ProjectStatus::unknown(ProjectState::MissingDatabase)
        );
        assert!(!path.exists());
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (id TEXT); CREATE TABLE threads (archived INTEGER)",
        )
        .unwrap();
        assert_eq!(
            read(),
            ProjectStatus::unknown(ProjectState::UnsupportedSchema)
        );
        conn.execute("ALTER TABLE threads ADD COLUMN project_id TEXT", [])
            .unwrap();
        let result = read();
        assert_eq!(result.state, ProjectState::Available);
        assert_eq!(
            (
                result.project_count,
                result.grouped_thread_count,
                result.other_provider_grouped_thread_count
            ),
            (Some(0), Some(0), None)
        );
        drop(conn);
        fs::write(&path, b"not a SQLite database").unwrap();
        assert_eq!(read(), ProjectStatus::unknown(ProjectState::Unreadable));
        let missing = home.path().join("not-created");
        fs::write(
            home.path().join("config.toml"),
            format!("sqlite_home={}", serde_json::to_string(&missing).unwrap()),
        )
        .unwrap();
        assert_eq!(
            read(),
            ProjectStatus::unknown(ProjectState::MissingDatabase)
        );
        assert!(!missing.exists());
        for bad in ["sqlite_home=42", "sqlite_home='relative'", "[broken"] {
            fs::write(home.path().join("config.toml"), bad).unwrap();
            assert_eq!(read(), ProjectStatus::unknown(ProjectState::Unreadable));
        }
    }
}
