//! Coordinate Codex maintenance between the GUI and Hub daemon without stopping Hub.
use std::{
    fs::{self, File, OpenOptions},
    path::PathBuf,
};

use anyhow::{Context, Result};
use fs2::FileExt;

pub(crate) struct Guard {
    _file: File,
}

fn lock_path() -> Result<PathBuf> {
    // Deliberately independent of CODEXHUB_HOME: all Hub instances for this OS
    // user must cooperate even when their Hub profiles or Codex homes differ.
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from)
        .context("无法确定本机用户数据目录，已停止 Codex 操作。")?;
    #[cfg(not(windows))]
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("无法确定本机用户目录，已停止 Codex 操作。")?
        .join("Library/Application Support");
    Ok(root.join("CodexHub").join("codex-startup-access.lock"))
}

fn acquire(path: &std::path::Path, exclusive: bool) -> Result<Guard> {
    fs::create_dir_all(path.parent().context("修复锁路径无效。")?)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .context("无法协调 Codex 操作，已停止。")?;
    let result = if exclusive {
        FileExt::try_lock_exclusive(&file)
    } else {
        FileExt::try_lock_shared(&file)
    };
    result.context(if exclusive {
        "Hub 正在处理其他 Codex 操作或已有修复任务，请稍后点击“重新检查”。Hub 无需退出。"
    } else {
        "Codex 启动修复正在进行，请等待修复结束后重试。"
    })?;
    Ok(Guard { _file: file })
}

pub(crate) fn operation() -> Result<Guard> {
    acquire(&lock_path()?, false)
}
pub(crate) fn repair() -> Result<Guard> {
    acquire(&lock_path()?, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_repair_excludes_operations_and_releases_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("access.lock");
        let first = acquire(&path, false).unwrap();
        let second = acquire(&path, false).unwrap();
        assert!(acquire(&path, true).is_err());
        drop(first);
        assert!(acquire(&path, true).is_err());
        drop(second);
        let repair = acquire(&path, true).unwrap();
        assert!(acquire(&path, false).is_err());
        assert!(acquire(&path, true).is_err());
        drop(repair);
        assert!(acquire(&path, false).is_ok());
    }
}
