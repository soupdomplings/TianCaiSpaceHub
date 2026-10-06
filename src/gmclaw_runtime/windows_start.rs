//! Launch only the verified TianGong desktop without inheriting Hub handles.
//! The token belongs to the child's environment, never its command line.
use std::{
    cmp::Ordering,
    ffi::{OsStr, OsString},
    os::windows::ffi::OsStrExt,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    Globalization::{CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringOrdinal},
    System::Threading::{
        CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION,
        STARTUPINFOW,
    },
};

pub(super) fn start(executable: &Path, token: &str, display_port: u16) -> Result<()> {
    let application = terminated(executable.as_os_str())?;
    let cwd = terminated(executable.parent().context("天工安装目录无效")?.as_os_str())?;
    let mut command_line = desktop_command_line(executable, display_port)?;
    let mut environment = desktop_environment(std::env::vars_os(), OsStr::new(token))?;
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let mut information: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    // CREATE_NO_WINDOW keeps helper consoles hidden. No STARTF_USESTDHANDLES
    // or inherited stdio is supplied: this is a GUI executable. Closing the
    // returned handles does not terminate the independently running desktop.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0, // bInheritHandles = FALSE, including any socket/file/pipe.
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            cwd.as_ptr(),
            &startup,
            &mut information,
        )
    };
    let failure = (created == 0).then(std::io::Error::last_os_error);
    environment.fill(0);
    if let Some(error) = failure {
        return Err(error.into());
    }
    unsafe {
        CloseHandle(information.hThread);
        CloseHandle(information.hProcess);
    }
    Ok(())
}

fn wide(value: &OsStr) -> Result<Vec<u16>> {
    let value: Vec<u16> = value.encode_wide().collect();
    ensure!(
        !value.contains(&0) && value.len() <= i32::MAX as usize,
        "天工启动参数或环境格式无效"
    );
    Ok(value)
}

fn terminated(value: &OsStr) -> Result<Vec<u16>> {
    let mut value = wide(value)?;
    value.push(0);
    Ok(value)
}

fn desktop_command_line(executable: &Path, display_port: u16) -> Result<Vec<u16>> {
    // A verified Windows executable path cannot contain a quote or end in a
    // directory separator. lpApplicationName is also explicit, so no PATH or
    // Program.exe fallback can select a different executable.
    let path = wide(executable.as_os_str())?;
    ensure!(
        executable.is_absolute() && !path.is_empty() && !path.contains(&(b'"' as u16)),
        "天工启动程序路径无效"
    );
    let mut result = vec![b'"' as u16];
    result.extend(path);
    result.push(b'"' as u16);
    result.extend(
        format!(" --remote-debugging-address=127.0.0.1 --remote-debugging-port={display_port}")
            .encode_utf16(),
    );
    result.push(0);
    Ok(result)
}

fn compare_keys(left: &[u16], right: &[u16]) -> Result<Ordering> {
    let result = unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        )
    };
    match result {
        CSTR_LESS_THAN => Ok(Ordering::Less),
        CSTR_EQUAL => Ok(Ordering::Equal),
        CSTR_GREATER_THAN => Ok(Ordering::Greater),
        _ => anyhow::bail!("天工子进程环境排序未完成"),
    }
}

fn desktop_environment(
    inherited: impl Iterator<Item = (OsString, OsString)>,
    token: &OsStr,
) -> Result<Vec<u16>> {
    let token_key: Vec<u16> = "GMCLAW_AUTH_TOKEN".encode_utf16().collect();
    let electron_key: Vec<u16> = "ELECTRON_RUN_AS_NODE".encode_utf16().collect();
    let mut entries = Vec::new();
    for (key, value) in inherited {
        let key = wide(&key)?;
        // Preserve Windows' hidden per-drive keys (for example =C:). Only a
        // leading '=' is allowed in a key; no arbitrary name=value injection.
        ensure!(
            !key.is_empty() && !key[1..].contains(&(b'=' as u16)),
            "天工子进程环境名称无效"
        );
        if compare_keys(&key, &token_key)? == Ordering::Equal
            || compare_keys(&key, &electron_key)? == Ordering::Equal
        {
            continue;
        }
        entries.push((key, wide(&value)?));
    }
    entries.push((token_key, wide(token)?));
    // Windows environment keys use ordinal case-insensitive ordering, rather
    // than locale-sensitive lowercasing or lossy Unicode conversion.
    let mut comparison_failed = false;
    entries.sort_by(|left, right| match compare_keys(&left.0, &right.0) {
        Ok(order) => order,
        Err(_) => {
            comparison_failed = true;
            Ordering::Equal
        }
    });
    ensure!(!comparison_failed, "天工子进程环境排序未完成");
    entries.dedup_by(|right, left| match compare_keys(&right.0, &left.0) {
        Ok(order) => order == Ordering::Equal,
        Err(_) => {
            comparison_failed = true;
            false
        }
    });
    ensure!(!comparison_failed, "天工子进程环境排序未完成");
    let mut result = Vec::new();
    for (key, value) in entries {
        result.extend(key);
        result.push(b'=' as u16);
        result.extend(value);
        result.push(0);
    }
    result.push(0);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_environment_replaces_token_removes_node_and_preserves_unicode() {
        let inherited = [
            ("Path", "fixture-original"),
            ("PATH", "fixture-duplicate"),
            ("gmclaw_auth_token", "fixture-old"),
            ("Electron_Run_As_Node", "1"),
            ("=C:", r"C:\fixture directory"),
            ("模型目录", "测试路径"),
        ]
        .into_iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)));
        let environment = desktop_environment(inherited, OsStr::new("fixture-token")).unwrap();
        assert!(environment.ends_with(&[0, 0]));
        let values = String::from_utf16(&environment).unwrap();
        let entries = values
            .split('\0')
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 4);
        assert!(entries.contains(&"GMCLAW_AUTH_TOKEN=fixture-token"));
        assert!(entries.contains(&"Path=fixture-original"));
        assert!(entries.contains(&"=C:=C:\\fixture directory"));
        assert!(entries.contains(&"模型目录=测试路径"));
        assert!(!values.contains("fixture-old"));
        assert!(!values.to_ascii_uppercase().contains("ELECTRON_RUN_AS_NODE"));
    }

    #[test]
    fn desktop_launch_quotes_exact_path_and_rejects_nul_values() {
        let executable = Path::new(r"D:\Program Files\天工\tiangong-desktop.exe");
        let arguments = desktop_command_line(executable, 18769).unwrap();
        assert_eq!(
            String::from_utf16(&arguments).unwrap(),
            "\"D:\\Program Files\\天工\\tiangong-desktop.exe\" --remote-debugging-address=127.0.0.1 --remote-debugging-port=18769\0"
        );
        assert!(desktop_environment(std::iter::empty(), OsStr::new("fixture\0token")).is_err());
        let invalid = std::iter::once((OsString::from("name=value"), OsString::from("fixture")));
        assert!(desktop_environment(invalid, OsStr::new("fixture")).is_err());
    }
}
