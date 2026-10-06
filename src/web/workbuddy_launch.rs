//! Explicit WorkBuddy desktop launch only; this is not an IM task interface.
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;

// Serialize all UI/API clicks and avoid spawning again while the first desktop
// instance is still initializing. This state does not enter user configuration.
static LAST_START: Mutex<Option<Instant>> = Mutex::const_new(None);

#[derive(Deserialize)]
pub(super) struct StartRequest {
    launch: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartResult {
    detail: String,
    running: bool,
    launched: bool,
}

pub(super) async fn start(Json(request): Json<StartRequest>) -> axum::response::Response {
    if !request.launch {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "启动请求无效"})),
        )
            .into_response();
    }
    let mut last_start = LAST_START.lock().await;
    let previous_start = *last_start;
    let result = tokio::task::spawn_blocking(move || start_desktop(previous_start)).await;
    match result {
        Ok(Ok(result)) => {
            if result.launched {
                *last_start = Some(Instant::now());
            }
            Json(result).into_response()
        }
        result => {
            let error = match result {
                Ok(Err(error)) => error.to_string(),
                Err(_) => "无法检查或启动 WorkBuddy，请稍后重试".into(),
                Ok(Ok(_)) => unreachable!(),
            };
            (StatusCode::CONFLICT, Json(json!({"error": error}))).into_response()
        }
    }
}

fn start_desktop(previous_start: Option<Instant>) -> Result<StartResult> {
    let (running, installed) = discover()?;
    if running {
        return Ok(StartResult {
            detail: "WorkBuddy 已运行，本次未重复启动。外部消息 /wb 任务接入暂不支持。".into(),
            running: true,
            launched: false,
        });
    }
    if previous_start.is_some_and(|started| started.elapsed() < Duration::from_secs(30)) {
        return Ok(StartResult {
            detail: "已发起 WorkBuddy 启动，请等待客户端打开；本次未重复启动。外部消息 /wb 任务接入暂不支持。".into(),
            running: false,
            launched: false,
        });
    }
    let executable = if let Some(configured) = std::env::var_os("WORKBUDDY_DESKTOP_PATH") {
        resolve_executable(&PathBuf::from(configured))
            .context("WORKBUDDY_DESKTOP_PATH 无效，请指定已安装的 WorkBuddy 桌面程序或安装目录")?
    } else {
        installed.context(
            "未找到 WorkBuddy 安装，请先安装；自定义位置可用 WORKBUDDY_DESKTOP_PATH 指定桌面程序或安装目录后重新打开 Hub",
        )?
    };
    launch_executable(&executable)?;
    Ok(StartResult {
        detail: "已发起 WorkBuddy 启动，请在客户端查看。外部消息 /wb 任务接入暂不支持。".into(),
        running: false,
        launched: true,
    })
}

#[cfg(windows)]
fn launch_executable(executable: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let mut child = Command::new(executable)
        .current_dir(executable.parent().context("WorkBuddy 安装目录无效")?)
        .env_remove("ELECTRON_RUN_AS_NODE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW: no helper console.
        .spawn()
        .context("无法启动 WorkBuddy，请检查安装和程序访问权限")?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_executable(executable: &Path) -> Result<()> {
    let bundle = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .context("WorkBuddy App 目录无效")?;
    let status = Command::new("/usr/bin/open")
        .arg("-a")
        .arg(bundle)
        .env_remove("ELECTRON_RUN_AS_NODE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("无法打开 WorkBuddy App，请检查安装")?;
    ensure!(
        status.success(),
        "无法打开 WorkBuddy App，请检查安装和访问权限"
    );
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn launch_executable(_: &Path) -> Result<()> {
    anyhow::bail!("WorkBuddy 桌面启动仅支持 Windows 和 macOS")
}

#[cfg(windows)]
fn valid_executable(path: &Path) -> bool {
    path.is_absolute()
        && path.is_file()
        && path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("WorkBuddy.exe"))
        && path
            .parent()
            .is_some_and(|parent| parent.join("resources/app.asar").is_file())
}

#[cfg(target_os = "macos")]
fn valid_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_absolute()
        && path
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        && path.parent().is_some_and(|parent| {
            parent.file_name().is_some_and(|name| name == "MacOS")
                && parent.parent().is_some_and(|contents| {
                    contents.join("Resources/app.asar").is_file()
                        && contents.parent().is_some_and(|bundle| {
                            bundle.file_name().is_some_and(|name| {
                                name.to_string_lossy().eq_ignore_ascii_case("WorkBuddy.app")
                            })
                        })
                })
        })
}

#[cfg(not(any(windows, target_os = "macos")))]
fn valid_executable(_: &Path) -> bool {
    false
}

fn resolve_executable(path: &Path) -> Option<PathBuf> {
    if valid_executable(path) {
        return Some(path.to_path_buf());
    }
    if !path.is_absolute() || !path.is_dir() {
        return None;
    }
    #[cfg(windows)]
    {
        let executable = path.join("WorkBuddy.exe");
        if valid_executable(&executable) {
            return Some(executable);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let bundle = if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("WorkBuddy.app"))
        {
            path.to_path_buf()
        } else {
            path.join("WorkBuddy.app")
        };
        let output = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", "Print :CFBundleExecutable"])
            .arg(bundle.join("Contents/Info.plist"))
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let name = String::from_utf8_lossy(&output.stdout);
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\']) {
            return None;
        }
        let executable = bundle.join("Contents/MacOS").join(name);
        if valid_executable(&executable) {
            return Some(executable);
        }
    }
    None
}

#[cfg(windows)]
fn discover() -> Result<(bool, Option<PathBuf>)> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_NO_MORE_FILES, GetLastError, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
    };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    ensure!(
        snapshot != INVALID_HANDLE_VALUE,
        "无法检查 WorkBuddy 进程，本次未启动"
    );
    let result = (|| -> Result<(bool, Option<PathBuf>)> {
        let mut running = false;
        let mut executable = None;
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut available = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        loop {
            if !available {
                ensure!(
                    unsafe { GetLastError() } == ERROR_NO_MORE_FILES,
                    "无法读取 WorkBuddy 进程列表，本次未启动"
                );
                break;
            }
            let end = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..end])
                .eq_ignore_ascii_case("WorkBuddy.exe")
            {
                // An unreadable matching process also blocks duplicate startup.
                running = true;
                let process = unsafe {
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID)
                };
                if !process.is_null() {
                    let mut image = vec![0_u16; 32_768];
                    let mut length = image.len() as u32;
                    let readable = unsafe {
                        QueryFullProcessImageNameW(process, 0, image.as_mut_ptr(), &mut length)
                    } != 0;
                    unsafe { CloseHandle(process) };
                    if readable {
                        let path =
                            PathBuf::from(String::from_utf16_lossy(&image[..length as usize]));
                        if valid_executable(&path) {
                            executable = Some(path);
                        }
                    }
                }
            }
            available = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        Ok((running, executable))
    })();
    unsafe { CloseHandle(snapshot) };
    let (running, executable) = result?;
    Ok((
        running,
        executable.or_else(|| {
            install_candidates()
                .into_iter()
                .find_map(|path| resolve_executable(&path))
        }),
    ))
}

#[cfg(windows)]
fn install_candidates() -> Vec<PathBuf> {
    use winreg::{
        RegKey,
        enums::{
            HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
        },
    };
    let mut candidates = Vec::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let root = RegKey::predef(hive);
            if let Ok(key) = root.open_subkey_with_flags(
                "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\WorkBuddy.exe",
                KEY_READ | view,
            ) {
                if let Ok(path) = key.get_value::<String, _>("") {
                    candidates.push(PathBuf::from(path.trim().trim_matches('"')));
                }
            }
            let Ok(uninstall) = root.open_subkey_with_flags(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                KEY_READ | view,
            ) else {
                continue;
            };
            for name in uninstall.enum_keys().filter_map(Result::ok) {
                let Ok(key) = uninstall.open_subkey_with_flags(name, KEY_READ | view) else {
                    continue;
                };
                let display: String = key.get_value("DisplayName").unwrap_or_default();
                if !display.to_ascii_lowercase().contains("workbuddy") {
                    continue;
                }
                if let Ok(location) = key.get_value::<String, _>("InstallLocation") {
                    candidates.push(PathBuf::from(location).join("WorkBuddy.exe"));
                }
                if let Ok(icon) = key.get_value::<String, _>("DisplayIcon") {
                    let icon = icon.trim();
                    let icon = if let Some(quoted) = icon.strip_prefix('"') {
                        quoted.split('"').next().unwrap_or_default()
                    } else {
                        icon.rsplit_once(',').map_or(icon, |(path, _)| path)
                    };
                    candidates.push(PathBuf::from(icon.trim()));
                }
            }
        }
    }
    for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(base) = std::env::var_os(key) {
            candidates.push(PathBuf::from(base).join("WorkBuddy/WorkBuddy.exe"));
        }
    }
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        let base = PathBuf::from(base);
        candidates.push(base.join("Programs/WorkBuddy/WorkBuddy.exe"));
        candidates.push(base.join("WorkBuddy/WorkBuddy.exe"));
    }
    candidates
}

#[cfg(target_os = "macos")]
fn discover() -> Result<(bool, Option<PathBuf>)> {
    // Query executable names only, never command arguments or private state.
    let output = Command::new("/bin/ps")
        .args(["-axo", "comm="])
        .output()
        .context("无法检查 WorkBuddy 进程，本次未启动")?;
    ensure!(
        output.status.success(),
        "无法读取 WorkBuddy 进程列表，本次未启动"
    );
    let mut running = false;
    let mut executable = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let path = PathBuf::from(line.trim());
        if line
            .to_ascii_lowercase()
            .contains("workbuddy.app/contents/macos/")
        {
            running = true;
            if valid_executable(&path) {
                executable = Some(path);
            }
        }
    }
    if executable.is_none() {
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home_dir) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home_dir).join("Applications"));
        }
        executable = roots
            .into_iter()
            .find_map(|root| resolve_executable(&root.join("WorkBuddy.app")));
    }
    Ok((running, executable))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn discover() -> Result<(bool, Option<PathBuf>)> {
    anyhow::bail!("WorkBuddy 桌面启动仅支持 Windows 和 macOS")
}
