#[cfg(target_os = "macos")]
mod macos;

#[cfg(windows)]
pub fn show_startup_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let body: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "TianCaiSpace Hub".encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(target_os = "macos")]
pub fn show_startup_error(message: &str) {
    // The message is an argument, never interpolated into AppleScript source.
    // Callers pass only redacted local errors, never URLs or remote response text.
    let result = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg("on run argv\n display alert \"TianCaiSpace Hub\" message (item 1 of argv) as critical\nend run")
        .arg(message)
        .output();
    if !result.is_ok_and(|output| output.status.success()) {
        eprintln!("{message}");
    }
}

#[cfg(any(windows, test))]
pub fn command_for(executable: &std::path::Path) -> Result<String, String> {
    let path = executable
        .to_str()
        .ok_or("程序路径无法编码 / Invalid executable path")?;
    if path.contains('"') || path.chars().any(char::is_control) || !executable.is_absolute() {
        return Err("程序路径无效 / Invalid executable path".into());
    }
    Ok(format!("\"{path}\" import-url \"%1\""))
}

pub fn register() -> Result<(), String> {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let exe =
            std::env::current_exe().map_err(|_| "无法读取程序位置 / Cannot locate executable")?;
        let command = command_for(&exe)?;
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = root
            .create_subkey(r"Software\Classes\tiancaispacehub")
            .map_err(|_| "无法注册网页导入 / Registration failed")?;
        let write = || -> std::io::Result<()> {
            key.set_value("", &"URL:TianCaiSpace Hub Import")?;
            key.set_value("URL Protocol", &"")?;
            key.set_value("TianCaiSpaceHubOwner", &command)?;
            key.create_subkey("DefaultIcon")?
                .0
                .set_value("", &format!("\"{}\",0", exe.display()))?;
            key.create_subkey(r"shell\open\command")?
                .0
                .set_value("", &command)?;
            Ok(())
        };
        return write().map_err(|_| "无法写入网页导入注册项 / Registration failed".into());
    }
    #[cfg(target_os = "macos")]
    {
        macos::register()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    Err("当前仅支持 Windows 网页导入注册 / Windows registration only".into())
}

pub fn unregister() -> Result<(), String> {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let command = command_for(
            &std::env::current_exe().map_err(|_| "无法读取程序位置 / Cannot locate executable")?,
        )?;
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let key = match root.open_subkey(r"Software\Classes\tiancaispacehub") {
            Ok(key) => key,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("无法读取注册项 / Cannot read registration".into()),
        };
        let owner: String = key.get_value("TianCaiSpaceHubOwner").unwrap_or_default();
        let actual: String = key
            .open_subkey(r"shell\open\command")
            .and_then(|k| k.get_value(""))
            .unwrap_or_default();
        if owner != command || actual != command {
            return Err("当前网页导入关联属于其他安装位置，已保留 / Registration belongs to another installation".into());
        }
        return root
            .delete_subkey_all(r"Software\Classes\tiancaispacehub")
            .map_err(|_| "无法解除注册 / Unregistration failed".into());
    }
    #[cfg(target_os = "macos")]
    {
        Err("macOS 由系统管理 App 的协议关联；退出并移除不再使用的 App 副本，或在保留的版本中重新注册 / macOS manages app URL associations".into())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    Err("当前仅支持 Windows 网页导入注册 / Windows registration only".into())
}
