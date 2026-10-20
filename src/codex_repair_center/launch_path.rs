//! Repair the GUI's CLI override independently of SQLite. Never run the CLI or
//! mutate Rust's process-wide environment from the GUI worker thread.
use super::*;
use serde::{Deserialize, Serialize};

const NAME: &str = "CODEX_CLI_PATH";
const JOURNAL: &str = "launch-path.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Stored {
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registry: Option<RegistryValue>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct RegistryValue {
    kind: u32,
    bytes: Vec<u8>,
}

trait Environment {
    fn scope(&self) -> &'static str;
    fn read(&self) -> Result<Option<Stored>>;
    fn encode(&self, text: &str) -> Stored;
    fn write(&self, expected: &Option<Stored>, next: &Option<Stored>) -> Result<()>;
}

struct Desktop;
#[cfg(windows)]
impl Environment for Desktop {
    fn scope(&self) -> &'static str {
        "windows-user"
    }
    fn read(&self) -> Result<Option<Stored>> {
        use winreg::{
            RegKey,
            enums::{HKEY_CURRENT_USER, REG_EXPAND_SZ, REG_SZ},
        };
        let key = match RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment") {
            Ok(key) => key,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let raw = match key.get_raw_value(NAME) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        ensure!(
            matches!(raw.vtype, REG_SZ | REG_EXPAND_SZ),
            "CODEX_CLI_PATH 不是文本类型，未覆盖"
        );
        ensure!(
            raw.bytes.len() % 2 == 0,
            "CODEX_CLI_PATH 文本编码异常，未覆盖"
        );
        let units = raw
            .bytes
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect::<Vec<_>>();
        let text = String::from_utf16(&units)?
            .trim_end_matches('\0')
            .to_string();
        Ok(Some(Stored {
            text,
            registry: Some(RegistryValue {
                kind: raw.vtype as u32,
                bytes: raw.bytes,
            }),
        }))
    }
    fn encode(&self, text: &str) -> Stored {
        Stored {
            text: text.into(),
            registry: Some(RegistryValue {
                kind: 1,
                bytes: text
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            }),
        }
    }
    fn write(&self, expected: &Option<Stored>, next: &Option<Stored>) -> Result<()> {
        use winreg::{
            RegKey, RegValue,
            enums::{HKEY_CURRENT_USER, REG_EXPAND_SZ, REG_SZ},
        };
        ensure!(self.read()? == *expected, "程序路径在检查后被修改，未覆盖");
        let raw = next
            .as_ref()
            .map(|v| -> Result<RegValue> {
                let raw = v.registry.as_ref().context("程序路径备份缺少注册表数据")?;
                let vtype = match raw.kind {
                    1 => REG_SZ,
                    2 => REG_EXPAND_SZ,
                    _ => bail!("程序路径备份类型未知"),
                };
                let units = raw
                    .bytes
                    .chunks_exact(2)
                    .map(|v| u16::from_le_bytes([v[0], v[1]]))
                    .collect::<Vec<_>>();
                ensure!(
                    raw.bytes.len() % 2 == 0
                        && String::from_utf16(&units)?.trim_end_matches('\0') == v.text,
                    "程序路径备份文本不一致"
                );
                Ok(RegValue {
                    vtype,
                    bytes: raw.bytes.clone(),
                })
            })
            .transpose()?;
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey("Environment")?;
        if let Some(raw) = raw {
            key.set_raw_value(NAME, &raw)?;
        } else {
            match key.delete_value(NAME) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        ensure!(self.read()? == *next, "程序路径写入后核对失败，请保留备份");
        super::package::broadcast_environment();
        Ok(())
    }
}
#[cfg(target_os = "macos")]
impl Environment for Desktop {
    fn scope(&self) -> &'static str {
        "macos-login-session"
    }
    fn read(&self) -> Result<Option<Stored>> {
        Ok(startup::desktop_env(NAME)?.map(|text| self.encode(&text)))
    }
    fn encode(&self, text: &str) -> Stored {
        Stored {
            text: text.into(),
            registry: None,
        }
    }
    fn write(&self, expected: &Option<Stored>, next: &Option<Stored>) -> Result<()> {
        ensure!(self.read()? == *expected, "程序路径在检查后被修改，未覆盖");
        if let Some(value) = next {
            startup::system_output("/bin/launchctl", &["setenv", NAME, &value.text])?;
        } else {
            startup::system_output("/bin/launchctl", &["unsetenv", NAME])?;
        }
        ensure!(self.read()? == *next, "程序路径写入后核对失败，请保留备份");
        Ok(())
    }
}
#[cfg(not(any(windows, target_os = "macos")))]
impl Environment for Desktop {
    fn scope(&self) -> &'static str {
        "unsupported"
    }
    fn read(&self) -> Result<Option<Stored>> {
        bail!("程序路径修复仅支持 Windows 和 macOS")
    }
    fn encode(&self, text: &str) -> Stored {
        Stored {
            text: text.into(),
            registry: None,
        }
    }
    fn write(&self, _: &Option<Stored>, _: &Option<Stored>) -> Result<()> {
        bail!("当前平台不支持程序路径修复")
    }
}

#[derive(Serialize, Deserialize)]
struct Journal {
    version: u32,
    kind: String,
    scope: String,
    stage: String,
    cli: PathBuf,
    cli_sha256: String,
    before: Option<Stored>,
    after: Stored,
}
fn normalized_target(cli: &Path) -> Result<String> {
    let path = cli.to_str().context("内置程序路径无法编码")?;
    // Registry values use a conventional Win32 path, including non-system
    // drives. Canonicalization on Windows may have added an extended prefix.
    #[cfg(windows)]
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    ensure!(!path.contains(['\0', '\r', '\n']), "内置程序路径格式异常");
    Ok(path.into())
}
fn matches_target(current: &Option<Stored>, cli: &Path) -> bool {
    current.as_ref().is_some_and(|v| {
        let path = Path::new(&v.text);
        path.is_absolute() && path.canonicalize().ok().as_deref() == Some(cli)
    })
}

pub(super) fn run(apply: bool) -> Result<Outcome> {
    let _access = if apply {
        crate::codex_access::repair()?
    } else {
        crate::codex_access::operation()?
    };
    if apply {
        startup::require_stopped()?;
    }
    let home = startup::user_directory()?;
    let cli = startup::locate_cli()?;
    repair(
        &Desktop,
        &cli,
        &backup_root(&home),
        apply,
        &startup::require_stopped,
    )
}
fn repair(
    store: &impl Environment,
    cli: &Path,
    root: &Path,
    apply: bool,
    stopped: &impl Fn() -> Result<()>,
) -> Result<Outcome> {
    let before = store.read()?;
    let target = normalized_target(cli)?;
    let needs_repair = !matches_target(&before, cli);
    let scope = if store.scope() == "windows-user" {
        "Windows 当前用户设置"
    } else {
        "macOS 当前登录会话"
    };
    let current = before
        .as_ref()
        .map(|v| v.text.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("未设置固定路径");
    let mut message =
        format!("当前内置程序：{target}\n当前 CODEX_CLI_PATH：{current}\n作用范围：{scope}。\n");
    message.push_str(if needs_repair {
        "可以重新绑定当前安装包内置程序。未设置固定路径本身不代表错误；若仍提示无法加载组织设置，可以执行这项路径修复，无需存在数据库冲突。"
    } else {
        "程序路径已经指向当前内置程序，无需更新。"
    });
    if !apply || !needs_repair {
        return Ok(Outcome {
            message,
            backup: None,
            applied: false,
            can_restore: false,
            needs_repair,
        });
    }
    stopped()?;
    let source_hash = digest(cli)?;
    let after = store.encode(&target);
    let directory = root.join(format!(
        "{}-{}-launch-path",
        crate::types::now_ms(),
        uuid::Uuid::new_v4().simple()
    ));
    let mut journal = Journal {
        version: 1,
        kind: "startup".into(),
        scope: store.scope().into(),
        stage: "backed-up".into(),
        cli: cli.into(),
        cli_sha256: source_hash.clone(),
        before: before.clone(),
        after: after.clone(),
    };
    let operation = (|| -> Result<()> {
        private_write(
            &directory.join(JOURNAL),
            &serde_json::to_vec_pretty(&journal)?,
        )?;
        stopped()?;
        ensure!(
            digest(cli)? == source_hash,
            "安装包在检查后已更新，请重新检查"
        );
        ensure!(store.read()? == before, "程序路径在备份后被修改，未覆盖");
        journal.stage = "writing".into();
        private_write(
            &directory.join(JOURNAL),
            &serde_json::to_vec_pretty(&journal)?,
        )?;
        store.write(&before, &Some(after))?;
        journal.stage = "applied".into();
        private_write(
            &directory.join(JOURNAL),
            &serde_json::to_vec_pretty(&journal)?,
        )?;
        Ok(())
    })();
    operation
        .with_context(|| format!("程序路径修复未全部完成，原值备份：{}", directory.display()))?;
    message = format!(
        "已将 CODEX_CLI_PATH 重新绑定到当前内置程序：{target}\n原路径已备份，未修改数据库或清空历史。请完全退出 ChatGPT/Codex 后重新增强启动验证；Hub 保持运行。"
    );
    if store.scope() == "macos-login-session" {
        message.push_str("\nmacOS 设置作用于本次登录会话；注销、重启或客户端更新后如再次出现路径问题，请重新检查。");
    }
    Ok(Outcome {
        message,
        backup: Some(directory),
        applied: true,
        can_restore: true,
        needs_repair: false,
    })
}

pub(super) fn is_record(path: &Path) -> bool {
    path.join(JOURNAL).is_file()
}
pub(super) fn restored(path: &Path) -> bool {
    fs::read(path.join(JOURNAL))
        .ok()
        .and_then(|v| serde_json::from_slice::<Journal>(&v).ok())
        .is_some_and(|v| v.stage == "restored")
}
pub(super) fn restore(path: &Path) -> Result<Outcome> {
    restore_with(&Desktop, path, &startup::require_stopped)
}
fn restore_with(
    store: &impl Environment,
    path: &Path,
    stopped: &impl Fn() -> Result<()>,
) -> Result<Outcome> {
    let mut journal: Journal = serde_json::from_slice(&fs::read(path.join(JOURNAL))?)?;
    ensure!(
        journal.version == 1 && journal.kind == "startup" && journal.scope == store.scope(),
        "程序路径备份不属于当前平台或格式未知"
    );
    ensure!(
        ["backed-up", "writing", "applied", "restoring", "restored"]
            .contains(&journal.stage.as_str()),
        "程序路径备份状态未知"
    );
    let current = store.read()?;
    ensure!(
        current == journal.before || current.as_ref() == Some(&journal.after),
        "CODEX_CLI_PATH 在修复后被修改，未覆盖后续设置"
    );
    stopped()?;
    journal.stage = "restoring".into();
    private_write(&path.join(JOURNAL), &serde_json::to_vec_pretty(&journal)?)?;
    if current != journal.before {
        store.write(&current, &journal.before)?;
    }
    journal.stage = "restored".into();
    private_write(&path.join(JOURNAL), &serde_json::to_vec_pretty(&journal)?)?;
    Ok(Outcome {
        message: "已恢复修复前的程序路径设置。原路径问题也可能恢复；数据库和会话保持原样。".into(),
        backup: Some(path.into()),
        applied: false,
        can_restore: false,
        needs_repair: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[derive(Default)]
    struct Fake {
        value: RefCell<Option<Stored>>,
    }
    impl Environment for Fake {
        fn scope(&self) -> &'static str {
            "windows-user"
        }
        fn read(&self) -> Result<Option<Stored>> {
            Ok(self.value.borrow().clone())
        }
        fn encode(&self, text: &str) -> Stored {
            Stored {
                text: text.into(),
                registry: None,
            }
        }
        fn write(&self, expected: &Option<Stored>, next: &Option<Stored>) -> Result<()> {
            ensure!(self.read()? == *expected, "changed");
            *self.value.borrow_mut() = next.clone();
            Ok(())
        }
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let cli = temp.path().join("codex.exe");
        fs::write(&cli, "fixture-only").unwrap();
        let cli = cli.canonicalize().unwrap();
        let root = temp.path().join("backups");
        (temp, cli, root)
    }
    #[test]
    fn no_database_conflict_is_required_for_path_repair() {
        let (_temp, cli, root) = fixture();
        let store = Fake::default();
        let check = repair(&store, &cli, &root, false, &|| Ok(())).unwrap();
        assert!(check.needs_repair);
        assert!(store.read().unwrap().is_none());
        assert!(!root.exists());
        let result = repair(&store, &cli, &root, true, &|| Ok(())).unwrap();
        assert!(result.applied);
        let backup = result.backup.unwrap();
        assert!(
            !repair(&store, &cli, &root, false, &|| Ok(()))
                .unwrap()
                .needs_repair
        );
        restore_with(&store, &backup, &|| Ok(())).unwrap();
        assert!(store.read().unwrap().is_none());
        assert!(restored(&backup));
    }
    #[test]
    fn retired_path_is_backed_up_and_later_changes_are_not_overwritten() {
        let (_temp, cli, root) = fixture();
        let store = Fake::default();
        let original = Some(store.encode("D:/old-package/codex.exe"));
        *store.value.borrow_mut() = original.clone();
        let result = repair(&store, &cli, &root, true, &|| Ok(())).unwrap();
        let backup = result.backup.unwrap();
        let patched = store.read().unwrap();
        *store.value.borrow_mut() = Some(store.encode("D:/new-choice/codex.exe"));
        assert!(restore_with(&store, &backup, &|| Ok(())).is_err());
        *store.value.borrow_mut() = patched;
        restore_with(&store, &backup, &|| Ok(())).unwrap();
        assert_eq!(store.read().unwrap(), original);
    }
    #[test]
    fn stopped_check_and_source_changes_prevent_path_writes() {
        let (_temp, cli, root) = fixture();
        let store = Fake::default();
        assert!(repair(&store, &cli, &root, true, &|| bail!("client running")).is_err());
        assert!(!root.exists());
        assert!(store.read().unwrap().is_none());
        let calls = std::cell::Cell::new(0);
        assert!(
            repair(&store, &cli, &root, true, &|| {
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    fs::write(&cli, "updated")?;
                }
                Ok(())
            })
            .is_err()
        );
        assert!(store.read().unwrap().is_none());
    }
    #[test]
    fn interrupted_path_write_can_be_restored_without_cli_or_node() {
        let (_temp, cli, root) = fixture();
        let store = Fake::default();
        let outcome = repair(&store, &cli, &root, true, &|| Ok(())).unwrap();
        let path = outcome.backup.unwrap();
        let mut journal: Journal =
            serde_json::from_slice(&fs::read(path.join(JOURNAL)).unwrap()).unwrap();
        journal.stage = "writing".into();
        private_write(&path.join(JOURNAL), &serde_json::to_vec(&journal).unwrap()).unwrap();
        fs::remove_file(cli).unwrap();
        restore_with(&store, &path, &|| Ok(())).unwrap();
        assert!(store.read().unwrap().is_none());
    }
}
