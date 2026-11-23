//! Desktop global-state storage for the project repair. The current renderer
//! reads committed SQLite first; the JSON remains a migration source.
use super::{
    CARDS, DELETIONS, MEMBERS, MIGRATION, ORDER, PROJECTLESS, SERVER_IDS, STATE_FILE, State,
    write_file_atomically,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde_json::value::RawValue;
use std::{fs, path::Path};

pub(super) const SQLITE: &str = ".codex-global-state.sqlite";
pub(super) const OWNER: &str = "electron-renderer-content-owner";
const KEYS: &[&str] = &[
    OWNER,
    CARDS,
    MEMBERS,
    ORDER,
    PROJECTLESS,
    SERVER_IDS,
    MIGRATION,
    DELETIONS,
    "remote-projects",
    "thread-project-membership-host-ids",
];

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) enum Store {
    #[default]
    Json,
    Sqlite,
}

impl Store {
    pub(super) fn read(home: &Path, owner: &str) -> Result<Option<(Self, State)>> {
        let path = home.join(SQLITE);
        if path.try_exists()? {
            ensure!(
                fs::symlink_metadata(&path)?.is_file(),
                "桌面侧栏数据库不是本机普通文件，请核对 Codex 用户目录。"
            );
            let state = crate::codex_project_status::with_readonly_database(&path, |tx| {
                read_sql(tx, owner)
            })?;
            return Ok(Some((Self::Sqlite, state)));
        }
        match fs::read(home.join(STATE_FILE)) {
            Ok(bytes) => Ok(Some((Self::Json, serde_json::from_slice(&bytes)?))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub(super) fn replace(
        self,
        home: &Path,
        owner: &str,
        before: &State,
        after: &State,
    ) -> Result<()> {
        if self == Self::Json {
            let current = Self::read(home, owner)?;
            ensure!(
                current
                    .as_ref()
                    .is_some_and(|(store, state)| *store == self && same(state, before)),
                "桌面侧栏已变化，请重新检查。"
            );
            return write_file_atomically(&home.join(STATE_FILE), &serde_json::to_vec(after)?);
        }
        let path = home.join(SQLITE);
        ensure!(
            fs::symlink_metadata(&path)?.is_file(),
            "桌面侧栏数据库已变化，请重新检查。"
        );
        let mut db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            same(&read_sql(&tx, owner)?, before),
            "桌面项目或当前账户已变化，请重新检查。"
        );
        for key in [CARDS, ORDER, SERVER_IDS, MEMBERS, PROJECTLESS] {
            let prior = before.get(key).map(|v| v.get());
            let next = after.get(key).map(|v| v.get());
            if prior == next {
                continue;
            }
            if let Some(value) = next {
                tx.execute("INSERT INTO settings(scope,key,value) VALUES('global',?1,?2) ON CONFLICT(scope,key) DO UPDATE SET value=excluded.value", params![key, value])?;
            } else {
                tx.execute(
                    "DELETE FROM settings WHERE scope='global' AND key=?1",
                    [key],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn same(left: &State, right: &State) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .all(|(k, v)| right.get(k).is_some_and(|r| r.get() == v.get()))
}

fn read_sql(tx: &Transaction<'_>, owner: &str) -> Result<State> {
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let known: bool = tx.query_row("SELECT count(*)=3 AND count(CASE WHEN type='TEXT' AND [notnull]=1 AND ((name='scope' AND pk=1) OR (name='key' AND pk=2) OR (name='value' AND pk=0)) THEN 1 END)=3 FROM pragma_table_info('settings')", [], |r| r.get(0))?;
    ensure!(
        version == 1 && known,
        "桌面已有 SQLite 侧栏，但没有可核实的已提交结构。请用原账号打开 Codex 并正常退出后重新检查，保留原库与备份。"
    );
    let mut query = tx.prepare("SELECT value FROM settings WHERE scope='global' AND key=?1")?;
    let active: Option<String> = query.query_row([OWNER], |r| r.get(0)).optional()?;
    ensure!(
        active
            .as_deref()
            .and_then(|v| serde_json::from_str::<String>(v).ok())
            .as_deref()
            == Some(owner),
        "新版侧栏的活动账号不匹配或尚未记录。请用原账号打开 Codex，再完全退出并重新检查。"
    );
    let mut state = State::new();
    for &key in KEYS {
        // Native desktop stores these two global preferences under the active
        // owner's physical key; inactive owners' settings never enter the plan.
        let physical = if [MIGRATION, DELETIONS].contains(&key) {
            format!("electron-owner-value:{key}:{owner}")
        } else {
            key.to_owned()
        };
        if let Some(value) = query
            .query_row([&physical], |r| r.get::<_, String>(0))
            .optional()?
        {
            state.insert(
                key.to_owned(),
                RawValue::from_string(value).context("无法读取当前账户的桌面项目设置。")?,
            );
        }
    }
    Ok(state)
}
