//! Current-user Windows login launch. The Run entry is the preference.
use anyhow::{Context, Result, ensure};
use std::{io, os::windows::ffi::OsStrExt, path::Path};
use winreg::{
    RegKey, RegValue,
    enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_SZ},
    types::FromRegValue,
};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVAL: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "TianCaiSpaceHub";

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Status {
    Off,
    On,
    OldLocation,
    DisabledByWindows,
}

fn command(exe: &Path) -> Result<String> {
    let path = exe
        .to_str()
        .context("安装位置无法作为 Windows 启动命令读取")?;
    ensure!(
        exe.is_absolute() && !path.contains(['"', '\0']),
        "Hub 安装位置无效"
    );
    let value = format!(r#""{path}" gui"#);
    ensure!(
        std::ffi::OsStr::new(&value).encode_wide().count() <= 260,
        "安装路径过长，无法添加到 Windows 启动项"
    );
    Ok(value)
}

fn raw_value(root: &RegKey, subkey: &str) -> io::Result<Option<RegValue>> {
    match root
        .open_subkey_with_flags(subkey, KEY_QUERY_VALUE)
        .and_then(|k| k.get_raw_value(NAME))
    {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        other => other.map(Some),
    }
}

fn owned_command(raw: &RegValue, exe: &Path) -> Result<String> {
    ensure!(raw.vtype == REG_SZ, "同名启动项已被其他程序使用");
    let value = String::from_reg_value(raw)?;
    let Some(path) = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix("\" gui"))
    else {
        anyhow::bail!("同名启动项已更改，请在 Windows 启动应用中核对");
    };
    ensure!(!path.contains(['"', '\0']), "同名启动项有其他参数，无法确认为 Hub 启动项");
    let path = Path::new(path);
    ensure!(
        path.is_absolute()
            && path
                .file_name()
                .is_some_and(|n| exe.file_name().is_some_and(|e| n.eq_ignore_ascii_case(e))),
        "同名启动项指向其他程序"
    );
    Ok(value)
}

fn status(root: &RegKey, exe: &Path) -> Result<Status> {
    let Some(value) = raw_value(root, RUN)? else {
        return Ok(Status::Off);
    };
    let registered = owned_command(&value, exe)?;
    if let Some(approval) = raw_value(root, APPROVAL)? {
        ensure!(
            approval.vtype == REG_BINARY && approval.bytes.len() >= 4,
            "Windows 启动应用状态无法读取"
        );
        match u32::from_le_bytes(approval.bytes[..4].try_into()?) {
            3 | 7 => return Ok(Status::DisabledByWindows),
            2 | 6 => {}
            _ => anyhow::bail!("请在 Windows 设置的「启动应用」中核对该项状态"),
        }
    }
    Ok(if registered.eq_ignore_ascii_case(&command(exe)?) {
        Status::On
    } else {
        Status::OldLocation
    })
}

fn set(root: &RegKey, exe: &Path, enable: bool) -> Result<()> {
    let before = status(root, exe)?;
    match (enable, before) {
        (true, Status::DisabledByWindows) => {
            anyhow::bail!("请在 Windows 设置 → 应用 → 启动中重新启用 TianCaiSpaceHub")
        }
        (true, Status::On) | (false, Status::Off) => return Ok(()),
        (true, _) => {
            let value = command(exe)?;
            let (key, _) = root.create_subkey_with_flags(RUN, KEY_SET_VALUE)?;
            key.set_value(NAME, &value)?;
        }
        (false, _) => {
            root.open_subkey_with_flags(RUN, KEY_SET_VALUE)?
                .delete_value(NAME)?;
        }
    }
    let after = status(root, exe)?;
    ensure!(after != Status::DisabledByWindows, "Windows 已停用此项。请在系统设置 → 应用 → 启动中重新启用 TianCaiSpaceHub");
    ensure!(
        after == if enable { Status::On } else { Status::Off },
        "Windows 启动设置未生效，请核对系统「启动应用」"
    );
    Ok(())
}

pub(crate) fn read() -> Result<Status> {
    status(
        &RegKey::predef(HKEY_CURRENT_USER),
        &std::env::current_exe()?,
    )
}
pub(crate) fn update(enable: bool) -> Result<()> {
    set(
        &RegKey::predef(HKEY_CURRENT_USER),
        &std::env::current_exe()?,
        enable,
    )
}

pub(crate) fn remove_on_uninstall() -> Result<()> {
    uninstall(
        &RegKey::predef(HKEY_CURRENT_USER),
        &std::env::current_exe()?,
    )
}
fn uninstall(root: &RegKey, exe: &Path) -> Result<()> {
    if let Some(value) = raw_value(root, RUN)?
        && owned_command(&value, exe)
            .is_ok_and(|v| command(exe).is_ok_and(|expected| expected.eq_ignore_ascii_case(&v)))
    {
        root.open_subkey_with_flags(RUN, KEY_SET_VALUE)?
            .delete_value(NAME)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_startup_roundtrip_and_ownership() {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let path = format!(r"Software\TianCaiSpaceHubTests\{}", uuid::Uuid::new_v4());
        let (root, _) = user.create_subkey(&path).unwrap();
        // All product-relative keys below are nested under this unique test root.
        let exe = Path::new(r"C:\Program Files\天才 Hub\TianCaiSpaceHub.exe");
        let moved = Path::new(r"D:\New Folder\TianCaiSpaceHub.exe");
        assert_eq!(status(&root, exe).unwrap(), Status::Off);
        set(&root, exe, true).unwrap();
        assert_eq!(
            root.open_subkey(RUN)
                .unwrap()
                .get_value::<String, _>(NAME)
                .unwrap(),
            r#""C:\Program Files\天才 Hub\TianCaiSpaceHub.exe" gui"#
        );
        assert_eq!(status(&root, moved).unwrap(), Status::OldLocation);
        set(&root, moved, true).unwrap();
        uninstall(&root, exe).unwrap();
        assert_eq!(status(&root, moved).unwrap(), Status::On);
        let (approval, _) = root.create_subkey(APPROVAL).unwrap();
        approval
            .set_raw_value(
                NAME,
                &RegValue {
                    vtype: REG_BINARY,
                    bytes: vec![3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                },
            )
            .unwrap();
        assert_eq!(status(&root, moved).unwrap(), Status::DisabledByWindows);
        assert!(set(&root, moved, true).is_err());
        set(&root, moved, false).unwrap();
        approval.delete_value(NAME).unwrap();
        set(&root, moved, true).unwrap();
        uninstall(&root, moved).unwrap();
        assert_eq!(status(&root, moved).unwrap(), Status::Off);
        let key = root.open_subkey_with_flags(RUN, KEY_SET_VALUE).unwrap();
        key.set_value(NAME, &"different-program").unwrap();
        assert!(set(&root, exe, true).is_err());
        uninstall(&root, exe).unwrap();
        assert_eq!(
            root.open_subkey(RUN)
                .unwrap()
                .get_value::<String, _>(NAME)
                .unwrap(),
            "different-program"
        );
        drop(key);
        drop(approval);
        drop(root);
        user.delete_subkey_all(&path).unwrap();
    }
}
