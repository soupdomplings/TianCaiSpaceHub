//! When a deleted config is re-created, preserve its explicit history location,
//! not its credentials, project trust or other application settings.
use super::{
    backup_path, managed_backup_paths, parse_existing_config_toml_for_update, write_file_atomically,
};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use toml_edit::DocumentMut;

const SNAPSHOT: &str = "history-location.json";

#[derive(Serialize, Deserialize)]
struct Location {
    // Also distinguish case-sensitive homes that share a legacy backup-directory key.
    home: PathBuf,
    sqlite_home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_for_manual_repair: Option<String>,
}

fn explicit(doc: &DocumentMut) -> Option<String> {
    doc.get("sqlite_home")?
        .as_str()
        .filter(|s| local_absolute(Path::new(s)))
        .map(str::to_owned)
}

fn local_absolute(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
    }
    #[cfg(not(windows))]
    {
        true
    }
}

pub(super) fn remember(home: &Path, doc: &DocumentMut) {
    let path = managed_backup_paths(home).dir.join(SNAPSHOT);
    let sqlite_home = explicit(doc);
    // A v1.0.36 re-init may have produced a config without the old history
    // location, with its previous value existing only in the rotating .bak.
    // Never automatically restore a cleared key. Keep one previous location
    // solely for the user's explicit "repair" action.
    let previous_for_manual_repair = if sqlite_home.is_none() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Location>(&raw).ok())
            .filter(|saved| saved.home == home)
            .map(|saved| saved.sqlite_home.or(saved.previous_for_manual_repair))
            .unwrap_or_else(|| {
                fs::read_to_string(home.join("config.toml.bak"))
                    .ok()
                    .and_then(|raw| {
                        raw.trim_start_matches('\u{feff}')
                            .parse::<DocumentMut>()
                            .ok()
                    })
                    .and_then(|doc| explicit(&doc))
            })
    } else {
        None
    };
    let location = Location {
        home: home.to_owned(),
        sqlite_home,
        previous_for_manual_repair,
    };
    let result = serde_json::to_vec(&location)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| write_file_atomically(&path, &bytes));
    if result.is_err() {
        // The app can still initialize and the normal config.toml.bak remains useful.
        tracing::warn!("Codex history location snapshot could not be saved");
    }
}

pub(super) fn open_or_initialize(path: &Path) -> Result<DocumentMut> {
    if path.try_exists()? {
        return parse_existing_config_toml_for_update(path);
    }
    let mut doc = DocumentMut::new();
    let home = path.parent().unwrap_or(Path::new("."));
    if let Some(location) = last_location(home, path, false)? {
        doc["sqlite_home"] = toml_edit::value(location);
    }
    Ok(doc)
}

fn last_location(home: &Path, path: &Path, manual: bool) -> Result<Option<String>> {
    let snapshot = managed_backup_paths(home).dir.join(SNAPSHOT);
    let backup = backup_path(path)?;
    let mut sources: Vec<_> = [(&snapshot, true), (&backup, false)]
        .into_iter()
        .filter_map(|(path, dedicated)| {
            Some((fs::metadata(path).ok()?.modified().ok()?, dedicated, path))
        })
        .collect();
    // An explicit reset in the latest readable record is authoritative, including
    // when its directory has disappeared. Never resurrect an older location.
    sources.sort_unstable_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
    for (_, dedicated, path) in sources {
        let Ok(raw) = fs::read_to_string(path) else {
            continue;
        };
        let location = if dedicated {
            let Ok(saved) = serde_json::from_str::<Location>(&raw) else {
                continue;
            };
            if saved.home != home {
                continue;
            }
            if manual {
                saved.sqlite_home.or(saved.previous_for_manual_repair)
            } else {
                saved.sqlite_home
            }
        } else {
            let Ok(saved) = raw.trim_start_matches('\u{feff}').parse::<DocumentMut>() else {
                continue;
            };
            explicit(&saved)
        };
        if manual && location.is_none() {
            // A recorded reset is final for automatic recovery only. The
            // exclusively manual route may offer the last explicit location.
            continue;
        }
        return Ok(location.filter(|location| contains_native_history(Path::new(location))));
    }
    Ok(None)
}

/// Opt-in repair for an older Hub that already rebuilt the config. The user's
/// existing keys and the historical config.toml.bak must survive the repair.
pub(crate) fn repair_history_location(home: &Path, apply: bool) -> Result<bool> {
    let _access = if apply {
        crate::codex_access::repair()?
    } else {
        crate::codex_access::operation()?
    };
    repair_with_default(home, apply, || {
        crate::codex_project_status::sqlite_dir(home)
    })
}

fn repair_with_default(
    home: &Path,
    apply: bool,
    default: impl FnOnce() -> Result<PathBuf>,
) -> Result<bool> {
    let path = home.join("config.toml");
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("无法读取当前 Codex 配置。"),
    };
    let mut doc = raw
        .trim_start_matches('\u{feff}')
        .parse::<DocumentMut>()
        .context("当前 Codex 配置格式有误，未执行历史位置修复。")?;
    if doc.contains_key("sqlite_home") {
        return Ok(false);
    }
    let Some(location) = last_location(home, &path, true)? else {
        return Ok(false);
    };
    let actual_default = default()?;
    if Path::new(&location) == actual_default
        || fs::canonicalize(&location)
            .ok()
            .zip(fs::canonicalize(&actual_default).ok())
            .is_some_and(|(a, b)| a == b)
    {
        return Ok(false);
    }
    if apply {
        let single = toml_edit::value(location);
        // Keep the original spelling, comments, CRLF and optional BOM byte-for-byte.
        // A single root entry can be inserted before the first existing table.
        let (bom, rest) = raw
            .strip_prefix('\u{feff}')
            .map_or(("", raw.as_str()), |rest| ("\u{feff}", rest));
        let newline = if rest.contains("\r\n") { "\r\n" } else { "\n" };
        write_file_atomically(
            &path,
            format!("{bom}sqlite_home = {single}{newline}{rest}").as_bytes(),
        )?;
        doc["sqlite_home"] = single;
        remember(home, &doc);
    }
    Ok(true)
}

fn contains_native_history(directory: &Path) -> bool {
    if !local_absolute(directory) {
        return false;
    }
    let db = directory.join(super::CODEX_APP_STATE_DB);
    if !db.is_file() {
        return false;
    }
    let Ok(mut uri) = url::Url::from_file_path(&db) else {
        return false;
    };
    // Inspect the persisted schema only. Do not create a database, WAL or SHM
    // file, read thread records, or touch a database which requires recovery.
    uri.set_query(Some("mode=ro&immutable=1"));
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    Connection::open_with_flags(uri.as_str(), flags).and_then(|conn|
        conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='threads') AND
            (SELECT count(*) FROM pragma_table_info('threads') WHERE name IN ('id','cwd','rollout_path')) = 3",
            [], |row| row.get::<_, bool>(0))
    ).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn database(root: &Path, name: &str) -> PathBuf {
        let folder = root.join(name);
        fs::create_dir(&folder).unwrap();
        let conn = Connection::open(folder.join(super::super::CODEX_APP_STATE_DB)).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads(id TEXT PRIMARY KEY, cwd TEXT, rollout_path TEXT)",
        )
        .unwrap();
        folder
    }

    fn config(root: &Path) -> DocumentMut {
        let mut doc: DocumentMut = "model_provider='old'\n[projects.example]\ntrust_level='trusted'\n[model_providers.old]\napi_key='fixture'".parse().unwrap();
        doc["sqlite_home"] = toml_edit::value(root.to_str().unwrap());
        doc
    }

    fn stamp(path: &Path, secs: u64) {
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn missing_config_restores_only_the_native_sqlite_home_and_respects_newer_clears() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let a = database(home, "A");
        let b = database(home, "另一个 # & %");
        let target = home.join("config.toml");
        let backup = backup_path(&target).unwrap();
        let snapshot = managed_backup_paths(home).dir.join(SNAPSHOT);
        remember(home, &config(&a));
        let saved = fs::read_to_string(&snapshot).unwrap();
        assert!(!saved.contains("api_key") && !saved.contains("trusted"));
        fs::write(&backup, config(&b).to_string()).unwrap();
        stamp(&snapshot, 1_700_000_100);
        stamp(&backup, 1_700_000_000);
        let recovered = open_or_initialize(&target).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered["sqlite_home"].as_str(), a.to_str());
        stamp(&backup, 1_700_000_200);
        assert_eq!(
            open_or_initialize(&target).unwrap()["sqlite_home"].as_str(),
            b.to_str()
        );
        fs::write(&backup, "# explicitly use the default location").unwrap();
        assert!(open_or_initialize(&target).unwrap().is_empty());
        fs::write(&backup, config(&a).to_string()).unwrap();
        stamp(&backup, 1_700_000_000);
        remember(home, &DocumentMut::new());
        assert!(open_or_initialize(&target).unwrap().is_empty());
        fs::write(&target, "model='user-choice'").unwrap();
        let existing = open_or_initialize(&target).unwrap();
        assert_eq!(existing.len(), 1);
        assert_eq!(existing["model"].as_str(), Some("user-choice"));
        fs::write(&target, "").unwrap();
        assert!(open_or_initialize(&target).unwrap().is_empty());
        assert!(!a.join("state_5.sqlite-shm").exists());
        assert!(!b.join("state_5.sqlite-wal").exists());
    }

    #[test]
    fn missing_config_never_guesses_an_unusable_or_foreign_history_directory() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let target = home.join("config.toml");
        let backup = backup_path(&target).unwrap();
        let snapshot = managed_backup_paths(home).dir.join(SNAPSHOT);
        let good = database(home, "older");
        remember(home, &config(&good));
        stamp(&snapshot, 1_700_000_000);
        let absent = home.join("not-created");
        let wrong = home.join("unrelated");
        fs::create_dir(&wrong).unwrap();
        let conn = Connection::open(wrong.join("state_5.sqlite")).unwrap();
        conn.execute_batch("CREATE TABLE unrelated(dummy)").unwrap();
        drop(conn);
        for candidate in [&absent, &wrong, Path::new("relative-home")] {
            fs::write(&backup, config(candidate).to_string()).unwrap();
            assert!(open_or_initialize(&target).unwrap().is_empty());
        }
        assert!(!absent.exists());
        fs::remove_file(backup).unwrap();
        let other_home = home.join("other-home");
        let other_config = other_home.join("config.toml");
        assert!(open_or_initialize(&other_config).unwrap().is_empty());
        let other_snapshot = managed_backup_paths(&other_home).dir.join(SNAPSHOT);
        fs::create_dir_all(other_snapshot.parent().unwrap()).unwrap();
        fs::copy(&snapshot, other_snapshot).unwrap();
        assert!(open_or_initialize(&other_config).unwrap().is_empty());
    }

    #[test]
    fn each_first_writer_recovers_the_same_location_after_the_user_deletes_config() {
        use super::super::{
            prepare_codex_app_config_recovery_snapshot, prepare_codex_app_conversation_mode,
            set_codex_app_provider_websocket, tests::test_configure_options, write_config_toml,
        };

        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let target = home.join("config.toml");
        let history = database(home, "State");
        let opts = test_configure_options(home.to_owned());
        fs::write(&target, config(&history).to_string()).unwrap();
        write_config_toml(&target, &opts).unwrap();
        let backup = prepare_codex_app_config_recovery_snapshot(Some(home.to_owned()))
            .unwrap()
            .unwrap();
        // Full config backups may also have been removed; keep only the per-home location snapshot.
        fs::remove_file(backup).unwrap();
        for entry in 0..3 {
            fs::remove_file(&target).unwrap();
            match entry {
                0 => write_config_toml(&target, &opts).unwrap(),
                1 => assert!(
                    prepare_codex_app_conversation_mode(Some(home.to_owned()), false).unwrap()
                ),
                _ => {
                    set_codex_app_provider_websocket(Some(home.to_owned()), "sample", true)
                        .unwrap();
                }
            }
            let rebuilt = fs::read_to_string(&target)
                .unwrap()
                .parse::<DocumentMut>()
                .unwrap();
            assert_eq!(rebuilt["sqlite_home"].as_str(), history.to_str());
            assert!(rebuilt.get("projects").is_none());
            assert!(
                rebuilt
                    .get("model_providers")
                    .and_then(|p| p.get("old"))
                    .is_none()
            );
        }
    }

    #[test]
    fn manual_repair_preserves_the_only_pointer_through_backup_rotation_and_edits_one_key() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let target = home.join("config.toml");
        let backup = backup_path(&target).unwrap();
        let history = database(home, "previous database");
        // This is the state left by an earlier Hub: new config already exists,
        // no dedicated snapshot yet and only config.toml.bak has the old location.
        let rebuilt = "\u{feff}# preserve formatting\r\nmodel_provider='now'\r\n[projects.abc]\r\ntrust_level='untrusted'";
        fs::write(&target, rebuilt).unwrap();
        fs::write(&backup, config(&history).to_string()).unwrap();
        assert!(!repair_with_default(home, false, || Ok(history.clone())).unwrap());
        assert!(repair_with_default(home, false, || Ok(home.to_owned())).unwrap());
        assert_eq!(fs::read_to_string(&target).unwrap(), rebuilt);
        super::super::backup_existing(&target).unwrap(); // ordinary "Update" rotates the old .bak
        assert_eq!(fs::read_to_string(&backup).unwrap(), rebuilt);
        assert!(repair_with_default(home, false, || Ok(home.to_owned())).unwrap());
        assert!(repair_with_default(home, true, || Ok(home.to_owned())).unwrap());
        let after = fs::read_to_string(&target).unwrap();
        assert!(after.starts_with("\u{feff}sqlite_home = "));
        assert!(after.ends_with(rebuilt.trim_start_matches('\u{feff}')));
        let doc = after
            .trim_start_matches('\u{feff}')
            .parse::<DocumentMut>()
            .unwrap();
        assert_eq!(doc["sqlite_home"].as_str(), history.to_str());
        assert_eq!(doc["model_provider"].as_str(), Some("now"));
        assert_eq!(fs::read_to_string(&backup).unwrap(), rebuilt);
        assert!(!repair_with_default(home, true, || Ok(home.to_owned())).unwrap());
        // Explicit settings, including explicitly invalid ones, are never replaced.
        fs::write(&target, "sqlite_home = ''").unwrap();
        assert!(!repair_with_default(home, true, || Ok(home.to_owned())).unwrap());
    }
}
