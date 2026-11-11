//! Discover optional program sources without executing or changing them.
use crate::desktop::transaction;
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

fn check(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.is_absolute() && path.is_file(),
        "天才搭档 Server 的已有来源已不存在，请自动查找或重新选择官方运行程序"
    );
    transaction::reject_links(path)?;
    ensure!(
        path.file_name().is_some_and(|n| if cfg!(windows) {
            n.eq_ignore_ascii_case("codex.exe")
        } else {
            n == "codex"
        }),
        "请选择官方运行程序 codex.exe / codex，供天才搭档 Server 准备使用"
    );
    // The Windows desktop shell has the same basename as the CLI.
    ensure!(
        !path
            .parent()
            .is_some_and(|p| p.join("resources/codex.exe").is_file()),
        "这是桌面窗口程序，请选择其内置的 codex.exe 或自动查找天才搭档 Server 来源"
    );
    path.canonicalize()
        .context("无法核对天才搭档 Server 的已有来源位置")
}

pub(super) fn resolve(selected: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = selected {
        return check(path);
    }
    available()
        .into_iter()
        .next()
        .context("未找到可复用的运行程序；请在设置中确认准备天才搭档 Server")
}
pub(super) fn available() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path).filter(|p| p.is_absolute()) {
            #[cfg(windows)]
            candidates.extend(windows_candidates(&dir));
            #[cfg(not(windows))]
            candidates.push(dir.join("codex"));
        }
    }
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.extend(windows_candidates(
            &PathBuf::from(&local).join("Programs/OpenAI/Codex"),
        ));
        let root = PathBuf::from(local).join("OpenAI/Codex/bin");
        if let Ok(entries) = std::fs::read_dir(root) {
            let mut entries: Vec<_> = entries.take(128).flatten().collect();
            entries
                .sort_by_key(|e| std::cmp::Reverse(e.metadata().and_then(|m| m.modified()).ok()));
            candidates.extend(entries.into_iter().map(|e| e.path().join("codex.exe")));
        }
    }
    #[cfg(target_os = "macos")]
    for path in [
        "/Applications/Codex.app/Contents/Resources/codex",
        "/Applications/ChatGPT.app/Contents/Resources/codex",
    ] {
        candidates.push(path.into());
    }
    #[cfg(windows)]
    {
        let mut packages = registered_packages();
        // Registration follows Store upgrades and non-default install volumes.
        // Fall back to the known package directory only when no registration is available.
        if packages.is_empty()
            && let Some(root) = std::env::var_os("ProgramFiles")
            && let Ok(entries) = std::fs::read_dir(PathBuf::from(root).join("WindowsApps"))
        {
            packages.extend(entries.take(8192).flatten().filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                desktop_package(&name).then(|| (name, entry.path()))
            }));
        }
        candidates.extend(package_candidates(packages));
    }
    candidates
        .into_iter()
        .filter(|p| p.is_file())
        .filter_map(|p| check(&p).ok())
        .collect()
}
#[cfg(windows)]
fn windows_candidates(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = [
        "codex.exe",
        "bin/codex.exe",
        "app/resources/codex.exe",
        "resources/codex.exe",
        "app/resources/codex-cli/bin/codex.exe",
        "app/resources/codex-cli/codex.exe",
        "resources/codex-cli/bin/codex.exe",
        "resources/codex-cli/codex.exe",
    ]
    .into_iter()
    .map(|relative| root.join(relative))
    .collect();
    for (package, target) in [
        ("codex-win32-x64", "x86_64-pc-windows-msvc"),
        ("codex-win32-arm64", "aarch64-pc-windows-msvc"),
    ] {
        for vendor in [
            format!("node_modules/@openai/codex/node_modules/@openai/{package}/vendor/{target}"),
            format!("node_modules/@openai/codex/vendor/{target}"),
        ] {
            for directory in ["bin", "codex"] {
                paths.push(root.join(&vendor).join(directory).join("codex.exe"));
            }
        }
    }
    paths
}
#[cfg(windows)]
fn desktop_package(name: &str) -> bool {
    name.starts_with("OpenAI.Codex_") || name.starts_with("OpenAI.ChatGPT_")
}
#[cfg(windows)]
fn registered_packages() -> Vec<(String, PathBuf)> {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, KEY_READ},
    };
    let Ok(repository) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages",
        KEY_READ,
    ) else {
        return Vec::new();
    };
    repository
        .enum_keys()
        .flatten()
        .filter_map(|name| {
            if !desktop_package(&name) {
                return None;
            }
            let package = repository.open_subkey_with_flags(&name, KEY_READ).ok()?;
            let path = PathBuf::from(package.get_value::<String, _>("PackageRootFolder").ok()?);
            (path.is_absolute() && path.is_dir()).then_some((name, path))
        })
        .collect()
}
#[cfg(windows)]
fn package_candidates(mut packages: Vec<(String, PathBuf)>) -> Vec<PathBuf> {
    packages.sort_by_key(|(name, _)| std::cmp::Reverse(package_version(name)));
    packages
        .into_iter()
        .flat_map(|(_, root)| windows_candidates(&root))
        .collect()
}
#[cfg(windows)]
fn package_version(name: &str) -> Vec<u64> {
    name.split('_')
        .nth(1)
        .unwrap_or("")
        .split('.')
        .map(|v| v.parse().unwrap_or(0))
        .collect()
}

pub(super) fn spawn_error(path: &Path, error: &std::io::Error) -> String {
    let action = match error.raw_os_error() {
        Some(2 | 3) => "程序或运行目录已变化，请在天才搭档或天工本机协助设置中重新准备共享 Server",
        Some(5 | 577 | 1260 | 225 | 226) => {
            "系统阻止了天才搭档 Server 运行，请核对文件权限和系统拦截记录，再重新准备共享运行包"
        }
        Some(193 | 216) => "天才搭档 Server 程序与本机格式或架构不兼容，请重新准备匹配的共享运行包",
        Some(740) => "天才搭档 Server 程序要求提权，请在共享准备入口选择可由当前用户运行的来源",
        _ => "请在天才搭档或天工本机协助设置中检查共享 Server，修正后重试新任务",
    };
    format!(
        "无法启动天才搭档 Server（系统错误 {}：{}）。{}。程序：{}",
        error
            .raw_os_error()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "未知".into()),
        error,
        action,
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn store_package_versions_are_compared_numerically() {
        #[cfg(windows)]
        assert!(
            package_version("OpenAI.Codex_26.1013.1.0_x64")
                > package_version("OpenAI.Codex_26.928.1.0_x64")
        );
    }
    #[test]
    fn desktop_shell_is_not_accepted_as_a_cli() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("resources")).unwrap();
        std::fs::write(dir.path().join("resources/codex.exe"), b"fixture cli").unwrap();
        let shell = dir
            .path()
            .join(if cfg!(windows) { "codex.exe" } else { "codex" });
        std::fs::write(&shell, b"fixture desktop").unwrap();
        assert!(check(&shell).unwrap_err().to_string().contains("桌面窗口"));
    }
    #[cfg(windows)]
    #[test]
    fn npm_native_discovery_accepts_current_bin_and_previous_codex_layouts() {
        let temp = tempfile::tempdir().unwrap();
        for relative in [
            "node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe",
            "node_modules/@openai/codex/node_modules/@openai/codex-win32-arm64/vendor/aarch64-pc-windows-msvc/bin/codex.exe",
            "node_modules/@openai/codex/vendor/x86_64-pc-windows-msvc/codex/codex.exe",
        ] {
            let program = temp.path().join(relative);
            std::fs::create_dir_all(program.parent().unwrap()).unwrap();
            std::fs::write(&program, b"native program fixture; never executed").unwrap();
            assert!(windows_candidates(temp.path()).contains(&program));
            assert_eq!(check(&program).unwrap(), program.canonicalize().unwrap());
        }
        let wrapper = temp.path().join("codex.cmd");
        std::fs::write(&wrapper, b"fixture wrapper; never executed").unwrap();
        assert!(check(&wrapper).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn registered_store_candidates_follow_current_version_and_custom_location() {
        let temp = tempfile::tempdir().unwrap();
        let previous = temp.path().join("custom-volume/old-package");
        let current = temp.path().join("custom-volume/current-package");
        for root in [&previous, &current] {
            std::fs::create_dir_all(root.join("app/resources")).unwrap();
            std::fs::write(root.join("app/resources/codex.exe"), b"fixture").unwrap();
        }
        let candidates = package_candidates(vec![
            (
                "OpenAI.Codex_26.928.1.0_x64__fixture".into(),
                previous.clone(),
            ),
            (
                "OpenAI.Codex_26.1103.7773.0_x64__fixture".into(),
                current.clone(),
            ),
        ]);
        let found = candidates.into_iter().find_map(|path| check(&path).ok());
        assert_eq!(
            found,
            Some(
                current
                    .join("app/resources/codex.exe")
                    .canonicalize()
                    .unwrap()
            )
        );
        assert!(desktop_package("OpenAI.ChatGPT_26.1103.1.0_x64__fixture"));
        assert!(!desktop_package("Unrelated.Codex_26.1103.1.0_x64__fixture"));
        assert!(
            windows_candidates(&previous)
                .contains(&previous.join("app/resources/codex-cli/bin/codex.exe"))
        );
    }
    #[test]
    fn system_failure_keeps_code_and_recovery_instead_of_blames() {
        for code in [2, 5, 193, 740, 1260] {
            let text = spawn_error(
                Path::new("C:/fixture/codex.exe"),
                &std::io::Error::from_raw_os_error(code),
            );
            assert!(text.contains(&code.to_string()));
            assert!(text.contains("codex.exe"));
            assert!(!text.contains("安全软件"));
        }
    }
}
