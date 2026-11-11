//! Discover optional program sources without executing or changing them.
use crate::desktop::transaction;
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

fn check(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.is_absolute() && path.is_file(),
        "Codex CLI 程序不存在，请在本机协助设置中重新选择程序或自动查找"
    );
    transaction::reject_links(path)?;
    ensure!(
        path.file_name().is_some_and(|n| if cfg!(windows) {
            n.eq_ignore_ascii_case("codex.exe")
        } else {
            n == "codex"
        }),
        "请选择 Codex CLI 的 codex.exe / codex"
    );
    // The Windows desktop shell has the same basename as the CLI.
    ensure!(
        !path
            .parent()
            .is_some_and(|p| p.join("resources/codex.exe").is_file()),
        "这是 Codex 桌面窗口程序，请选择其 resources 文件夹中的 codex.exe 或使用自动查找"
    );
    path.canonicalize().context("无法核对 Codex CLI 程序位置")
}

pub(super) fn resolve(selected: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = selected {
        return check(path);
    }
    available()
        .into_iter()
        .next()
        .context("未找到 Codex CLI；请在设置中确认准备共享天才搭档 Server")
}
pub(super) fn available() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path).filter(|p| p.is_absolute()) {
            candidates.push(dir.join(if cfg!(windows) { "codex.exe" } else { "codex" }));
            #[cfg(windows)]
            for rel in [
                "node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/codex/codex.exe",
                "node_modules/@openai/codex/vendor/x86_64-pc-windows-msvc/codex/codex.exe",
                "node_modules/@openai/codex/node_modules/@openai/codex-win32-arm64/vendor/aarch64-pc-windows-msvc/codex/codex.exe",
            ] {
                candidates.push(dir.join(rel));
            }
        }
    }
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(&local).join("Programs/OpenAI/Codex/bin/codex.exe"));
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
    if let Some(root) = std::env::var_os("ProgramFiles") {
        if let Ok(entries) = std::fs::read_dir(PathBuf::from(root).join("WindowsApps")) {
            let mut packages: Vec<_> = entries
                .take(8192)
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("OpenAI.Codex_"))
                .collect();
            packages.sort_by_key(|e| {
                std::cmp::Reverse(package_version(&e.file_name().to_string_lossy()))
            });
            candidates.extend(
                packages
                    .into_iter()
                    .map(|e| e.path().join("app/resources/codex.exe")),
            );
        }
    }
    candidates
        .into_iter()
        .filter(|p| p.is_file())
        .filter_map(|p| check(&p).ok())
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
        Some(2 | 3) => "程序或运行目录已变化，请在本机协助设置中点击自动查找，或重新选择 Codex CLI",
        Some(5 | 577 | 1260 | 225 | 226) => {
            "系统阻止了程序运行，请核对该 Codex CLI 的文件权限和系统拦截记录，再在本机协助设置中重新选择可运行的程序"
        }
        Some(193 | 216) => "所选文件不是本机可运行的 Codex CLI，请选择与系统架构匹配的 codex.exe",
        Some(740) => "所选程序要求提权，Hub 不会自动提权；请选择可由当前用户运行的 Codex CLI",
        _ => "请在天工 Work → 接入设置 → 本机协助中核对 Codex CLI 程序位置，修正后重试新任务",
    };
    format!(
        "无法启动 Codex CLI 协助后台（系统错误 {}：{}）。{}。程序：{}",
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
