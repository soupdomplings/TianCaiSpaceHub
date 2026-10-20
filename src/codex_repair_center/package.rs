use super::*;

pub(super) fn windows_build() -> u32 {
    #[cfg(windows)]
    {
        return winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
            .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
            .ok()
            .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber").ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
    }
    #[cfg(not(windows))]
    {
        0
    }
}
#[cfg(windows)]
fn registry_value(key: &str) -> Result<Option<winreg::RegValue>> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    ensure!(
        key.starts_with("Software\\Google\\Chrome\\NativeMessagingHosts\\")
            && key.rsplit('\\').next().is_some_and(|s| s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._".contains(c))),
        "浏览器注册路径不在受支持范围"
    );
    match RegKey::predef(HKEY_CURRENT_USER).open_subkey(key) {
        Ok(k) => match k.get_raw_value("") {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub(super) fn registry(
    home: &Path,
    value: &Value,
    apply: bool,
    result: &mut Outcome,
) -> Result<()> {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let key = value["key"].as_str().context("浏览器注册路径缺失")?;
        let target = value["value"].as_str().context("浏览器注册文件缺失")?;
        let current = registry_value(key)?;
        let expected = winreg::RegValue {
            vtype: winreg::enums::REG_SZ,
            bytes: target
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        };
        if current.as_ref() == Some(&expected) {
            return Ok(());
        }
        result.needs_repair = true;
        result
            .message
            .push_str("\n\nChrome 本机连接的注册路径需要更新。");
        if !apply {
            return Ok(());
        }
        let directory = result.backup.clone().unwrap_or_else(|| {
            backup_root(home).join(format!(
                "{}-{}",
                crate::types::now_ms(),
                uuid::Uuid::new_v4().simple()
            ))
        });
        fs::create_dir_all(&directory)?;
        let snapshot = json!({"schemaVersion":1,"key":key,"before":current.as_ref().map(|v|json!({"bytes":v.bytes,"type":v.vtype.clone() as u32})),"after":{"bytes":expected.bytes,"type":expected.vtype.clone() as u32}});
        private_write(
            &directory.join("native-registry.json"),
            &serde_json::to_vec_pretty(&snapshot)?,
        )?;
        ensure!(
            registry_value(key)? == current,
            "注册信息在修复期间发生变化，已保留备份，未覆盖"
        );
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(key)?;
        key.set_raw_value("", &expected)?;
        result.backup = Some(directory);
        result.can_restore = true;
        result.applied = true;
    }
    #[cfg(not(windows))]
    let _ = (home, value, apply, result);
    Ok(())
}
pub(super) fn registry_restore(backup: &Path, apply: bool) -> Result<()> {
    #[cfg(windows)]
    {
        use winreg::{
            RegKey, RegValue,
            enums::{HKEY_CURRENT_USER, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ},
        };
        let file = backup.join("native-registry.json");
        if !file.is_file() {
            return Ok(());
        }
        let mut value: Value = serde_json::from_slice(&fs::read(&file)?)?;
        ensure!(value["schemaVersion"] == 1, "浏览器注册备份结构未识别");
        let key = value["key"].as_str().context("注册备份缺少路径")?;
        let decode = |v: &Value| -> Result<Option<RegValue>> {
            if v.is_null() {
                return Ok(None);
            }
            let bytes: Vec<u8> = serde_json::from_value(v["bytes"].clone())?;
            let vtype = match v["type"].as_u64() {
                Some(1) => REG_SZ,
                Some(2) => REG_EXPAND_SZ,
                _ => bail!("注册备份类型未知，未覆盖"),
            };
            Ok(Some(RegValue { bytes, vtype }))
        };
        let before = decode(&value["before"])?;
        let after = decode(&value["after"])?;
        let current = registry_value(key)?;
        ensure!(
            current == before || current == after,
            "Chrome 注册信息在修复后被修改，未覆盖"
        );
        if !apply {
            return Ok(());
        }
        if current != before {
            let k = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(key, KEY_SET_VALUE)?;
            if let Some(before) = before {
                k.set_raw_value("", &before)?;
            } else {
                k.delete_value("")?;
            }
        }
        value["restored"] = json!(true);
        private_write(&file, &serde_json::to_vec(&value)?)?;
    }
    #[cfg(not(windows))]
    let _ = (backup, apply);
    Ok(())
}

/// A Windows-only user switch required by the native CUA surface. It is never
/// inferred from a screenshot failure and is changed only for the selected CUA
/// repair. The original raw registry value is restorable.
pub(super) fn surface_environment(home: &Path, apply: bool, result: &mut Outcome) -> Result<()> {
    #[cfg(windows)]
    {
        use winreg::{
            RegKey, RegValue,
            enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ},
        };
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
            "Environment",
            if apply {
                KEY_READ | KEY_WRITE
            } else {
                KEY_READ
            },
        )?;
        let name = "CODEX_ELECTRON_ENABLE_WINDOWS_COMPUTER_USE";
        let before = match key.get_raw_value(name) {
            Ok(v) => Some(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let expected = RegValue {
            vtype: REG_SZ,
            bytes: "1\0".encode_utf16().flat_map(u16::to_le_bytes).collect(),
        };
        if before.as_ref() == Some(&expected) {
            return Ok(());
        }
        result.needs_repair = true;
        result
            .message
            .push_str("\n\nWindows 电脑操控的本机开关尚未启用；修复时会保存原值并启用。");
        if apply {
            let directory = result.backup.clone().unwrap_or_else(|| {
                backup_root(home).join(format!(
                    "{}-{}",
                    crate::types::now_ms(),
                    uuid::Uuid::new_v4().simple()
                ))
            });
            let snapshot = json!({"schemaVersion":1,"name":name,"before":before.as_ref().map(|v|json!({"bytes":v.bytes,"type":v.vtype.clone() as u32})),"after":{"bytes":expected.bytes,"type":1}});
            private_write(
                &directory.join("native-environment.json"),
                &serde_json::to_vec(&snapshot)?,
            )?;
            let current = key.get_raw_value(name).ok();
            ensure!(
                current == before,
                "Windows 电脑操控开关在修复期间变化，未覆盖"
            );
            key.set_raw_value(name, &expected)?;
            broadcast_environment();
            result.backup = Some(directory);
            result.applied = true;
            result.can_restore = true;
        }
    }
    #[cfg(not(windows))]
    let _ = (home, apply, result);
    Ok(())
}
#[cfg(windows)]
pub(super) fn broadcast_environment() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    let name = "Environment\0".encode_utf16().collect::<Vec<_>>();
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            name.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            1000,
            std::ptr::null_mut(),
        );
    }
}
pub(super) fn environment_restore(backup: &Path, apply: bool) -> Result<()> {
    #[cfg(windows)]
    {
        use winreg::{
            RegKey, RegValue,
            enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_EXPAND_SZ, REG_SZ},
        };
        let file = backup.join("native-environment.json");
        if !file.is_file() {
            return Ok(());
        }
        let mut value: Value = serde_json::from_slice(&fs::read(&file)?)?;
        ensure!(
            value["schemaVersion"] == 1
                && value["name"] == "CODEX_ELECTRON_ENABLE_WINDOWS_COMPUTER_USE",
            "环境开关备份不属于本工具，未覆盖"
        );
        let decode = |v: &Value| -> Result<Option<RegValue>> {
            if v.is_null() {
                return Ok(None);
            }
            Ok(Some(RegValue {
                bytes: serde_json::from_value(v["bytes"].clone())?,
                vtype: match v["type"].as_u64() {
                    Some(1) => REG_SZ,
                    Some(2) => REG_EXPAND_SZ,
                    _ => bail!("环境备份类型不支持"),
                },
            }))
        };
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
            "Environment",
            if apply {
                KEY_READ | KEY_WRITE
            } else {
                KEY_READ
            },
        )?;
        let name = "CODEX_ELECTRON_ENABLE_WINDOWS_COMPUTER_USE";
        let current = key.get_raw_value(name).ok();
        let before = decode(&value["before"])?;
        let after = decode(&value["after"])?;
        ensure!(
            current == before || current == after,
            "Windows 电脑操控开关已被后续修改，未覆盖"
        );
        if apply && current != before {
            if let Some(before) = before {
                key.set_raw_value(name, &before)?;
            } else {
                key.delete_value(name)?;
            }
            broadcast_environment();
        }
        if apply {
            value["restored"] = json!(true);
            private_write(&file, &serde_json::to_vec(&value)?)?;
        }
    }
    #[cfg(not(windows))]
    let _ = (backup, apply);
    Ok(())
}

pub(super) fn apply(
    toolkit: &Path,
    plan: &Value,
    progress: &mut impl FnMut(&str),
) -> Result<Outcome> {
    let backup = PathBuf::from(
        plan["backup"]
            .as_str()
            .context("客户端文件修复缺少备份目录")?,
    );
    #[cfg(windows)]
    {
        return windows(toolkit, &backup, Some(plan), progress);
    }
    #[cfg(target_os = "macos")]
    {
        return mac::apply(&backup, plan, progress);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (toolkit, backup, progress);
        bail!("此修复仅支持 Windows 和 macOS")
    }
}
pub(super) fn restore(
    toolkit: &Path,
    backup: &Path,
    progress: &mut impl FnMut(&str),
) -> Result<Outcome> {
    if !verify_restore(backup)? {
        return Ok(Outcome {
            message: "客户端安装文件无需再次恢复，继续恢复关联配置。".into(),
            backup: Some(backup.into()),
            applied: false,
            can_restore: false,
            needs_repair: false,
        });
    }
    #[cfg(windows)]
    {
        return windows(toolkit, backup, None, progress);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = toolkit;
        return mac::restore(backup, progress);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (toolkit, backup, progress);
        bail!("此修复仅支持 Windows 和 macOS")
    }
}
pub(super) fn verify_restore(backup: &Path) -> Result<bool> {
    #[cfg(windows)]
    {
        let file = backup.join("package-journal.json");
        if !file.is_file() {
            return Ok(false);
        }
        let journal: Value = serde_json::from_slice(&fs::read(file)?)?;
        ensure!(journal["schemaVersion"] == 1, "客户端文件备份格式未知");
        let stage = journal["stage"].as_str().unwrap_or("");
        if ["restored", "prepared"].contains(&stage) {
            return Ok(false);
        }
        ensure!(
            ["applied", "installing", "failed"].contains(&stage),
            "客户端文件备份状态未知"
        );
        let current: Value = serde_json::from_slice(&system(
            Path::new("powershell.exe"),
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                r#"[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$p=Get-AppxPackage -Name OpenAI.Codex|Sort-Object Version -Descending|Select-Object -First 1;@{name=$p.PackageFullName;root=$p.InstallLocation}|ConvertTo-Json -Compress"#,
            ],
            None,
            Duration::from_secs(20),
        )?)?;
        let asar = PathBuf::from(
            current["root"]
                .as_str()
                .context("当前用户的客户端安装目录不可用")?,
        )
        .join("app/resources/app.asar");
        let current_sha = digest(&asar)?;
        if stage == "failed" && Some(current_sha.as_str()) == journal["originalAsarSha256"].as_str()
        {
            return Ok(false);
        }
        ensure!(
            current["name"] == journal["installedPackage"]
                && Some(current_sha.as_str()) == journal["patchedAsarSha256"].as_str(),
            "客户端已在修复后更新或修改，未覆盖；请保留备份进一步检查"
        );
        let recovery = PathBuf::from(
            journal["recoveryPackage"]
                .as_str()
                .context("恢复包路径缺失")?,
        );
        ensure!(
            recovery.parent() == Some(backup)
                && Some(digest(&recovery)?.to_uppercase())
                    == journal["recoverySha256"].as_str().map(str::to_uppercase),
            "恢复包路径或完整性校验失败"
        );
        return Ok(true);
    }
    #[cfg(target_os = "macos")]
    {
        return mac::verify_restore(backup);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = backup;
        bail!("此修复仅支持 Windows 和 macOS");
    }
}
#[cfg(windows)]
fn windows(
    toolkit: &Path,
    backup: &Path,
    plan: Option<&Value>,
    progress: &mut impl FnMut(&str),
) -> Result<Outcome> {
    let tools = backup.join("tools");
    assets::extract(&tools)?;
    let query = r#"[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$p=Get-AppxPackage -Name OpenAI.Codex|Sort-Object Version -Descending|Select-Object -First 1;@{userSid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;sessionId=(Get-Process -Id $PID).SessionId;packageName=$p.Name;packageFullName=$p.PackageFullName}|ConvertTo-Json -Compress"#;
    let mut request: Value = serde_json::from_slice(&system(
        Path::new("powershell.exe"),
        &["-NoProfile", "-NonInteractive", "-Command", query],
        None,
        Duration::from_secs(20),
    )?)?;
    request["action"] = json!(if plan.is_some() { "repair" } else { "restore" });
    if let Some(plan) = plan {
        request["plan"] = plan.clone();
    }
    let file = backup.join("package-request.json");
    private_write(&file, &serde_json::to_vec_pretty(&request)?)?;
    let launcher = tools.join("hub-elevate.ps1");
    progress(
        "正在准备系统修复执行器。若出现系统权限请求，请核对后确认；Hub 保持运行，原程序不会预先卸载。此步骤可能需要数分钟…",
    );
    let _ = toolkit;
    let out = system(
        Path::new("powershell.exe"),
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            launcher.to_str().context("执行器路径无效")?,
            "-RequestPath",
            file.to_str().context("修复路径无效")?,
        ],
        None,
        Duration::from_secs(1800),
    )?;
    let value: Value = serde_json::from_slice(&out)
        .context("修复执行器未返回结果。恢复包已保留，请在修复记录中重新检查。")?;
    ensure!(
        value["success"] == true,
        "{}\n备份目录：{}",
        value["message"]
            .as_str()
            .unwrap_or("客户端文件修复未完成，原程序未预先卸载"),
        backup.display()
    );
    Ok(Outcome {
        message: value["message"]
            .as_str()
            .unwrap_or("客户端修复已结束")
            .into(),
        backup: Some(backup.into()),
        applied: value["applied"] == true,
        can_restore: value["canRestore"] == true,
        needs_repair: false,
    })
}

#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod mac {
    use super::*;
    use std::collections::BTreeMap;
    fn tree(root: &Path) -> Result<BTreeMap<String, String>> {
        fn visit(root: &Path, p: &Path, out: &mut BTreeMap<String, String>) -> Result<()> {
            for e in fs::read_dir(p)? {
                let e = e?;
                let p = e.path();
                let kind = e.file_type()?;
                let key = p.strip_prefix(root)?.to_string_lossy().into_owned();
                if kind.is_symlink() {
                    out.insert(key, format!("link:{}", fs::read_link(&p)?.display()));
                } else if kind.is_dir() {
                    visit(root, &p, out)?;
                } else if kind.is_file() {
                    out.insert(key, digest(&p)?);
                } else {
                    bail!("应用中含不受支持的特殊文件");
                }
            }
            Ok(())
        }
        let mut out = BTreeMap::new();
        visit(root, root, &mut out)?;
        Ok(out)
    }
    fn run(program: &str, args: &[&str]) -> Result<Vec<u8>> {
        let out = rt()?.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(300),
                tokio::process::Command::new(program)
                    .args(args)
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .context("macOS 文件或签名操作超时，请保留备份后重新检查")?
            .context("macOS 文件或签名操作未执行")
        })?;
        ensure!(
            out.status.success(),
            "应用备份、完整性或系统签名检查失败，原应用已保留"
        );
        Ok(out.stdout)
    }
    fn copy(from: &Path, to: &Path) -> Result<()> {
        run(
            "/usr/bin/ditto",
            &[
                "--rsrc",
                "--extattr",
                from.to_str().context("应用路径无效")?,
                to.to_str().context("备份路径无效")?,
            ],
        )?;
        Ok(())
    }
    fn authorized(script: &Path, args: &[&str], parent: &Path) -> Result<()> {
        if tempfile::NamedTempFile::new_in(parent).is_ok() {
            let mut values = vec![script.to_str().context("macOS 执行器路径无效")?];
            values.extend_from_slice(args);
            run("/bin/sh", &values)?;
        } else {
            let quote = |v: &str| format!("'{}'", v.replace('\'', "'\\''"));
            let command = std::iter::once("/bin/sh")
                .chain(std::iter::once(
                    script.to_str().context("macOS 执行器路径无效")?,
                ))
                .chain(args.iter().copied())
                .map(quote)
                .collect::<Vec<_>>()
                .join(" ");
            let source = format!(
                "do shell script {} with administrator privileges",
                serde_json::to_string(&command)?
            );
            run("/usr/bin/osascript", &["-e", &source])
                .context("未取得 macOS 系统授权或替换未完成，请保留备份后重新检查")?;
        }
        Ok(())
    }
    fn swap(
        app: &Path,
        candidate: &Path,
        expected: &BTreeMap<String, String>,
        backup: &Path,
    ) -> Result<()> {
        ensure!(&tree(app)? == expected, "应用在检查后发生变化，未覆盖");
        startup::require_stopped()?;
        let parent = app.parent().context("应用目录无效")?;
        let suffix = uuid::Uuid::new_v4().simple();
        let staged = parent.join(format!(".hub-repair-{suffix}.app"));
        let old = parent.join(format!(".hub-before-{suffix}.app"));
        let script = backup.join("hub-package-macos.sh");
        private_write(
            &script,
            include_bytes!("../../assets/codex-repair/hub-package-macos.sh"),
        )?;
        let previous = digest(&app.join("Contents/Resources/app.asar"))?;
        let next = digest(&candidate.join("Contents/Resources/app.asar"))?;
        private_write(
            &backup.join("mac-swap.json"),
            &serde_json::to_vec(
                &json!({"app":app,"staged":staged,"previous":old,"expectedSha256":previous}),
            )?,
        )?;
        authorized(
            &script,
            &[
                app.to_str().context("应用路径无效")?,
                candidate.to_str().context("候选路径无效")?,
                staged.to_str().unwrap(),
                old.to_str().unwrap(),
                &previous,
                &next,
            ],
            parent,
        )?;
        ensure!(
            tree(app)? == tree(candidate)?,
            "替换后的应用与候选不一致，完整原应用备份已保留"
        );
        // Only this task's verified, same-parent old bundle is removed; the
        // original complete application remains in the private repair backup.
        ensure!(
            old.parent() == Some(parent)
                && old
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".hub-before-"),
            "旧应用临时路径异常"
        );
        let _ = fs::remove_dir_all(old);
        Ok(())
    }
    pub(super) fn apply(
        backup: &Path,
        plan: &Value,
        progress: &mut impl FnMut(&str),
    ) -> Result<Outcome> {
        let source = PathBuf::from(plan["source"].as_str().context("客户端路径缺失")?);
        let prepared = PathBuf::from(plan["prepared"].as_str().context("候选文件缺失")?);
        ensure!(
            digest(&source)? == plan["sourceSha256"].as_str().unwrap_or("")
                && digest(&prepared)? == plan["preparedSha256"].as_str().unwrap_or(""),
            "来源或候选文件已变化，未替换"
        );
        let app = source
            .ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .context("无法定位所属 macOS 应用")?;
        let before = tree(app)?;
        let original = backup.join("original.app");
        let staged = backup.join("prepared.app");
        copy(app, &original)?;
        ensure!(tree(&original)? == before, "应用备份校验失败");
        copy(&original, &staged)?;
        let target = staged.join(source.strip_prefix(app)?);
        fs::copy(&prepared, &target)?;
        let plist = staged.join("Contents/Info.plist");
        let raw = run(
            "/usr/bin/plutil",
            &[
                "-convert",
                "json",
                "-o",
                "-",
                plist.to_str().context("应用清单路径无效")?,
            ],
        )?;
        let mut doc: Value = serde_json::from_slice(&raw)?;
        if let Some(entries) = doc.get_mut("ElectronAsarIntegrity") {
            let entries = entries.as_object_mut().context("应用完整性表结构未知")?;
            let entry = entries
                .get_mut("Resources/app.asar")
                .context("应用完整性表路径未知")?;
            ensure!(entry["algorithm"] == "SHA256", "应用完整性算法未知");
            let raw = fs::read(&target)?;
            ensure!(raw.len() > 16, "ASAR 头无效");
            let n = u32::from_le_bytes(raw[12..16].try_into()?) as usize;
            ensure!(16 + n <= raw.len(), "ASAR 头越界");
            entry["hash"] = json!(hex::encode(Sha256::digest(&raw[16..16 + n])));
        }
        private_write(&plist, &serde_json::to_vec(&doc)?)?;
        run(
            "/usr/bin/plutil",
            &["-convert", "binary1", plist.to_str().unwrap()],
        )?;
        progress("正在验证 macOS 候选应用签名；原应用与完整备份均已保留…");
        run(
            "/usr/bin/codesign",
            &[
                "--force",
                "--deep",
                "--sign",
                "-",
                "--preserve-metadata=identifier,entitlements,requirements,runtime",
                staged.to_str().unwrap(),
            ],
        )?;
        run(
            "/usr/bin/codesign",
            &["--verify", "--deep", "--strict", staged.to_str().unwrap()],
        )?;
        let after = tree(&staged)?;
        let file = backup.join("mac-package.json");
        let mut journal =
            json!({"schemaVersion":1,"stage":"prepared","app":app,"before":before,"after":after});
        private_write(&file, &serde_json::to_vec(&journal)?)?;
        swap(app, &staged, &before, backup)?;
        journal["stage"] = json!("applied");
        private_write(&file, &serde_json::to_vec(&journal)?)?;
        Ok(Outcome{message:"macOS 客户端文件修复、完整性与本地签名验证完成。原应用已完整备份；系统辅助功能与屏幕录制权限仍由系统设置管理。请使用增强启动后测试。".into(),backup:Some(backup.into()),applied:true,can_restore:true,needs_repair:false})
    }
    pub(super) fn verify_restore(backup: &Path) -> Result<bool> {
        let file = backup.join("mac-package.json");
        if !file.is_file() {
            return Ok(false);
        }
        let journal: Value = serde_json::from_slice(&fs::read(file)?)?;
        ensure!(journal["schemaVersion"] == 1, "macOS 应用备份格式未知");
        if journal["stage"] == "restored" {
            return Ok(false);
        }
        ensure!(
            ["applied", "prepared"].contains(&journal["stage"].as_str().unwrap_or("")),
            "应用备份状态不支持恢复"
        );
        let app = PathBuf::from(journal["app"].as_str().context("应用备份缺少路径")?);
        let before: BTreeMap<String, String> = serde_json::from_value(journal["before"].clone())?;
        let after: BTreeMap<String, String> = serde_json::from_value(journal["after"].clone())?;
        ensure!(
            tree(&backup.join("original.app"))? == before,
            "原应用备份校验失败"
        );
        if app.exists() {
            let current = tree(&app)?;
            ensure!(
                current == before || current == after,
                "应用在修复后被修改，未覆盖"
            );
        } else {
            let swap: Value = serde_json::from_slice(&fs::read(backup.join("mac-swap.json"))?)?;
            let previous = PathBuf::from(
                swap["previous"]
                    .as_str()
                    .context("中断记录缺少原应用路径")?,
            );
            ensure!(
                swap["app"].as_str() == app.to_str()
                    && previous.parent() == app.parent()
                    && previous
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with(".hub-before-")),
                "中断时原应用路径未通过校验"
            );
            let current = tree(&previous)?;
            ensure!(
                current == before || current == after,
                "中断时原应用位置未通过校验"
            );
        }
        Ok(true)
    }
    pub(super) fn restore(backup: &Path, _: &mut impl FnMut(&str)) -> Result<Outcome> {
        verify_restore(backup)?;
        let file = backup.join("mac-package.json");
        let mut journal: Value = serde_json::from_slice(&fs::read(&file)?)?;
        ensure!(
            journal["schemaVersion"] == 1
                && ["applied", "prepared"].contains(&journal["stage"].as_str().unwrap_or("")),
            "应用备份状态不支持恢复"
        );
        let app = PathBuf::from(journal["app"].as_str().context("应用备份缺少路径")?);
        let after: BTreeMap<String, String> = serde_json::from_value(journal["after"].clone())?;
        let before: BTreeMap<String, String> = serde_json::from_value(journal["before"].clone())?;
        let original = backup.join("original.app");
        ensure!(tree(&original)? == before, "原应用备份校验失败");
        if !app.exists() {
            let s: Value = serde_json::from_slice(&fs::read(backup.join("mac-swap.json"))?)?;
            authorized(
                &backup.join("hub-package-macos.sh"),
                &[
                    app.to_str().unwrap(),
                    original.to_str().unwrap(),
                    s["staged"].as_str().context("缺少临时路径")?,
                    s["previous"].as_str().unwrap(),
                    s["expectedSha256"].as_str().context("缺少中断校验值")?,
                    "unused",
                    "recover",
                ],
                app.parent().context("应用路径无效")?,
            )?;
            let current = tree(&app)?;
            ensure!(
                current == before || current == after,
                "中断应用回位后未通过校验"
            );
        }
        if tree(&app)? != before {
            swap(&app, &original, &after, backup)?;
        }
        journal["stage"] = json!("restored");
        private_write(&file, &serde_json::to_vec(&journal)?)?;
        Ok(Outcome {
            message: "已恢复完整的原 macOS 应用，用户配置和会话保持原样。".into(),
            backup: Some(backup.into()),
            applied: false,
            can_restore: false,
            needs_repair: false,
        })
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn mac_restore_rejects_later_changes_to_application_or_backup() {
            let root = tempfile::tempdir().unwrap();
            let app = root.path().join("Fixture.app");
            let backup = root.path().join("backup");
            fs::create_dir_all(&app).unwrap();
            fs::create_dir_all(backup.join("original.app")).unwrap();
            fs::write(app.join("file"), "after").unwrap();
            fs::write(backup.join("original.app/file"), "before").unwrap();
            let before = tree(&backup.join("original.app")).unwrap();
            let after = tree(&app).unwrap();
            fs::write(backup.join("mac-package.json"),serde_json::to_vec(&json!({"schemaVersion":1,"stage":"applied","app":app,"before":before,"after":after})).unwrap()).unwrap();
            assert!(verify_restore(&backup).unwrap());
            fs::write(app.join("file"), "later user change").unwrap();
            assert!(verify_restore(&backup).is_err());
            fs::write(app.join("file"), "after").unwrap();
            fs::write(backup.join("original.app/file"), "corrupt backup").unwrap();
            assert!(verify_restore(&backup).is_err());
        }
        #[test]
        fn mac_restore_accepts_interrupted_apply_or_restore_swap() {
            let root = tempfile::tempdir().unwrap();
            let app = root.path().join("Fixture.app");
            let previous = root.path().join(".hub-before-fixture.app");
            let backup = root.path().join("backup");
            fs::create_dir_all(backup.join("original.app")).unwrap();
            fs::create_dir_all(&previous).unwrap();
            fs::write(backup.join("original.app/file"), "before").unwrap();
            fs::write(previous.join("file"), "after").unwrap();
            let before = tree(&backup.join("original.app")).unwrap();
            let after = tree(&previous).unwrap();
            fs::write(
                backup.join("mac-package.json"),
                serde_json::to_vec(&json!({
                    "schemaVersion":1,"stage":"applied","app":app,
                    "before":before,"after":after
                }))
                .unwrap(),
            )
            .unwrap();
            fs::write(
                backup.join("mac-swap.json"),
                serde_json::to_vec(&json!({"app":app,"previous":previous})).unwrap(),
            )
            .unwrap();
            assert!(!app.exists());
            // Applying a repair moves the original bundle aside; restoring a
            // repair moves the patched bundle aside. Both interruption points
            // must pass preflight before the privileged swap can resume.
            assert!(verify_restore(&backup).unwrap());
            fs::write(previous.join("file"), "before").unwrap();
            assert!(verify_restore(&backup).unwrap());
            fs::write(previous.join("file"), "later user change").unwrap();
            assert!(verify_restore(&backup).is_err());
        }
    }
}
