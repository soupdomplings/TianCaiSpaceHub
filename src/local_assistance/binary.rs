//! Locate native CLI programs without executing them. Store-package binaries
//! are staged under this feature's own root before CreateProcess, never patched.
use crate::desktop::transaction;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

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
        .find_map(|p| check(&p).ok())
        .context("未找到 Codex CLI，请安装 Codex 或在本机协助设置中选择程序位置")
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

pub(super) fn packaged(path: &Path) -> bool {
    cfg!(windows)
        && path
            .components()
            .any(|c| c.as_os_str().eq_ignore_ascii_case("WindowsApps"))
}
const MAX_BINARY: u64 = 512 * 1024 * 1024;
fn digest(path: &Path) -> Result<String> {
    transaction::reject_links(path)?;
    let mut input =
        std::fs::File::open(path).context("无法读取已安装的 Codex CLI，请重新选择程序")?;
    ensure!(
        input.metadata()?.len() <= MAX_BINARY,
        "Codex CLI 文件大小异常，未复制或执行"
    );
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let n = input.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        ensure!(total <= MAX_BINARY, "Codex CLI 文件大小异常，未复制或执行");
        hash.update(&bytes[..n]);
    }
    ensure!(total > 0, "Codex CLI 文件为空，未执行");
    Ok(hex::encode(hash.finalize()))
}
fn stage_file(source: &Path, destination: &Path, expected: &str) -> Result<()> {
    transaction::reject_links(destination)?;
    if destination.is_file() && digest(destination)? == expected {
        return Ok(());
    }
    let parent = destination.parent().context("后台程序缓存位置无效")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let input = std::fs::File::open(source)?;
    let size = std::io::copy(&mut input.take(MAX_BINARY + 1), &mut temp)?;
    ensure!(size <= MAX_BINARY, "后台程序复制超过限制，未执行");
    temp.flush()?;
    temp.as_file().sync_all()?;
    ensure!(
        digest(temp.path())? == expected,
        "Codex CLI 在复制期间变化，请稍后重试"
    );
    temp.persist(destination)
        .map_err(|e| e.error)
        .context("无法准备 Codex CLI 缓存，请检查 Hub 数据目录权限")?;
    Ok(())
}
pub(super) fn prepare_at(root: &Path, source: &Path) -> Result<PathBuf> {
    let source_id = digest(source)?;
    let folder = root.join("runtime").join(&source_id);
    transaction::reject_links(&folder)?;
    std::fs::create_dir_all(&folder)?;
    crate::desktop::permissions::owner_only(&folder)?;
    let source_dir = source.parent().context("Codex CLI 程序位置无效")?;
    // The CLI uses these siblings for Windows sandbox / Code Mode. Copy only
    // this known family, with the same bounded hash/readback protection.
    let mut total = 0u64;
    for name in [
        "codex.exe",
        "codex-command-runner.exe",
        "codex-windows-sandbox-setup.exe",
        "codex-windows-sandbox-service.exe",
        "codex-code-mode-host.exe",
    ] {
        let from = source_dir.join(name);
        if !from.is_file() {
            continue;
        }
        total = total.saturating_add(std::fs::metadata(&from)?.len());
        ensure!(
            total <= 768 * 1024 * 1024,
            "Codex CLI 组件超过容量限制，未执行"
        );
        let expected = if name == "codex.exe" {
            source_id.clone()
        } else {
            digest(&from)?
        };
        stage_file(&from, &folder.join(name), &expected)?;
    }
    let program = folder.join("codex.exe");
    ensure!(
        program.is_file(),
        "Codex CLI 缓存程序不存在，未执行"
    );
    Ok(program)
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
    fn staging_preserves_installed_files_and_repairs_only_its_own_cache() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("install");
        std::fs::create_dir(&install).unwrap();
        let cli = install.join("codex.exe");
        std::fs::write(&cli, b"synthetic CLI; never launched").unwrap();
        std::fs::write(
            install.join("codex-command-runner.exe"),
            b"synthetic helper",
        )
        .unwrap();
        std::fs::write(install.join("unrelated.exe"), b"do not copy").unwrap();
        let root = dir.path().join("feature");
        let cached = prepare_at(&root, &cli).unwrap();
        assert!(!cached.parent().unwrap().join("unrelated.exe").exists());
        assert!(
            cached
                .parent()
                .unwrap()
                .join("codex-command-runner.exe")
                .is_file()
        );
        std::fs::write(&cached, b"changed cache").unwrap();
        assert_eq!(prepare_at(&root, &cli).unwrap(), cached);
        assert_eq!(
            std::fs::read(&cached).unwrap(),
            std::fs::read(&cli).unwrap()
        );
        std::fs::write(&cli, b"new synthetic version").unwrap();
        assert_ne!(prepare_at(&root, &cli).unwrap(), cached);
        assert!(cached.is_file());
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
