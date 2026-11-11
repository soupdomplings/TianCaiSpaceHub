//! Read installed metadata and prepare our small preload; never edit the install.
use super::*;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    process::{Command, Stdio},
};

const ANCHOR: &str = "let ye=createSkillVlmService({authenticationManager:e.authenticationManager,productManager:e.productManager,appVersion:r.appVersion,fetch:s,logger:O});";
const BOOTSTRAP: &[u8] = include_bytes!("host-bridge.cjs");

pub(super) struct Prepared {
    executable: PathBuf,
    bootstrap: PathBuf,
    server: PathBuf,
}

pub(super) enum Outcome {
    NeedsRestart,
    Ready(Prepared),
}

pub(super) fn prepare(config_path: &Path) -> Result<Outcome> {
    ensure!(
        cfg!(windows),
        "WorkBuddy 内置模型分享当前仅适配 Windows；macOS 待单独验证"
    );
    let (running, discovered) = crate::web::workbuddy_launch::discover()?;
    if running {
        return Ok(Outcome::NeedsRestart);
    }
    let executable = if let Some(configured) = std::env::var_os("WORKBUDDY_DESKTOP_PATH") {
        crate::web::workbuddy_launch::resolve_executable(&PathBuf::from(configured))
    } else {
        discovered
    }
    .context("未找到 WorkBuddy，请先安装，或设置 WORKBUDDY_DESKTOP_PATH 指定桌面程序")?;
    let resources = executable
        .parent()
        .context("WorkBuddy 安装路径无效")?
        .join("resources");
    verify_archive(&resources.join("app.asar"))?;
    verify_fuses(&executable)?;
    let root = super::super::state_root(config_path).join("workbuddy-host");
    fs::create_dir_all(&root).context("无法准备 WorkBuddy 本机分享适配器")?;
    let bootstrap = root.join("host-bridge.cjs");
    if fs::read(&bootstrap).ok().as_deref() != Some(BOOTSTRAP) {
        super::super::transaction::atomic_write(&bootstrap, BOOTSTRAP)?;
    }
    Ok(Outcome::Ready(Prepared {
        executable,
        bootstrap,
        server: resources.join("app.asar/main/server.js"),
    }))
}

// The two exact entries are read without executing installation JavaScript.
fn archive_entry(file: &mut fs::File, tree: &Value, data: u64, key: &str) -> Result<Vec<u8>> {
    let mut entry = tree;
    for component in key.split('/') {
        entry = &entry["files"][component];
    }
    ensure!(
        entry["unpacked"] != true && entry.get("link").is_none(),
        "WorkBuddy 安装资源布局已变化，暂未启用分享"
    );
    let size = entry["size"]
        .as_u64()
        .filter(|size| *size <= 8 * 1024 * 1024)
        .context("WorkBuddy 安装资源不兼容")?;
    let offset = entry["offset"]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .context("WorkBuddy 安装资源位置无效")?;
    let start = data
        .checked_add(offset)
        .context("WorkBuddy 安装资源位置无效")?;
    ensure!(
        start
            .checked_add(size)
            .is_some_and(|end| end <= file.metadata().map(|m| m.len()).unwrap_or(0)),
        "WorkBuddy 安装资源不完整"
    );
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; size as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn verify_archive(path: &Path) -> Result<()> {
    let mut file = fs::File::open(path).context("无法读取 WorkBuddy 安装资源")?;
    let mut header = [0; 16];
    file.read_exact(&mut header)?;
    let value = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap()) as u64;
    let size = value(12);
    ensure!(
        value(0) == 4 && (1..=16 * 1024 * 1024).contains(&size) && value(4) >= size + 8,
        "WorkBuddy ASAR 目录格式不兼容"
    );
    let data = 8 + value(4);
    let mut bytes = vec![0; size as usize];
    file.read_exact(&mut bytes)?;
    let tree: Value = serde_json::from_slice(&bytes).context("WorkBuddy ASAR 目录无效")?;
    let package: Value =
        serde_json::from_slice(&archive_entry(&mut file, &tree, data, "package.json")?)?;
    ensure!(
        package["version"] == "5.9.3",
        "当前 WorkBuddy 版本尚未适配内置模型分享；本轮支持 Windows 5.9.3，请保留普通启动"
    );
    let server = archive_entry(&mut file, &tree, data, "main/server.js")?;
    let source = std::str::from_utf8(&server).context("WorkBuddy 宿主脚本格式不兼容")?;
    ensure!(
        source.matches(ANCHOR).count() == 1 && source.contains("coordinator:"),
        "WorkBuddy 宿主接线位置已变化，暂未启用分享"
    );
    Ok(())
}

fn verify_fuses(path: &Path) -> Result<()> {
    const SENTINEL: &[u8] = b"dL7pKGdnNz796PbbjQWNKmHXBZaB9tsX";
    let mut file = fs::File::open(path)?;
    let mut tail = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..count]);
        if let Some(index) = tail
            .windows(SENTINEL.len())
            .position(|value| value == SENTINEL)
        {
            let wire = &tail[index + SENTINEL.len()..];
            if wire.len() >= 10 {
                ensure!(
                    wire[0] == 1 && wire[1] >= 8 && wire[2] == b'1' && wire[4] == b'1',
                    "当前 WorkBuddy 不允许本机分享适配启动；保留原程序，暂未启用分享"
                );
                return Ok(());
            }
        }
        let keep = tail.len().saturating_sub(64);
        tail.drain(..keep);
    }
    anyhow::bail!("当前 WorkBuddy 启动格式不兼容，暂未启用内置模型分享")
}

impl Prepared {
    pub(super) fn launch(self, callback: &str, lease: &str, bridge_id: &str) -> Result<()> {
        let preload = self.bootstrap.to_string_lossy();
        ensure!(
            !preload.contains(['"', '\r', '\n']) && !preload.contains("  "),
            "Hub 配置目录包含暂不支持的连续空白，请选择普通路径后启用 WorkBuddy 分享"
        );
        let original = std::env::var("NODE_OPTIONS").unwrap_or_default();
        let options = format!("{original} --require \"{preload}\"");
        let mut command = Command::new(&self.executable);
        command
            .current_dir(self.executable.parent().context("WorkBuddy 安装目录无效")?)
            .env_remove("ELECTRON_RUN_AS_NODE")
            .env("NODE_OPTIONS", options)
            .env("TIANCAI_WORKBUDDY_ORIGINAL_NODE_OPTIONS", original)
            .env("TIANCAI_WORKBUDDY_CALLBACK", callback)
            .env("TIANCAI_WORKBUDDY_LEASE", lease)
            .env("TIANCAI_WORKBUDDY_BRIDGE_ID", bridge_id)
            .env("TIANCAI_WORKBUDDY_SERVER", self.server)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .spawn()
            .context("无法启动 WorkBuddy 分享，请检查程序权限")?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn asar_anchor_and_version_are_checked_without_install_writes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app.asar");
        let write = |version: &str, source: &str| {
            let package = serde_json::to_vec(&json!({"version":version})).unwrap();
            let tree = serde_json::to_vec(&json!({"files":{"package.json":{"size":package.len(),"offset":"0"},"main":{"files":{"server.js":{"size":source.len(),"offset":package.len().to_string()}}}}})).unwrap();
            let padded = (tree.len() + 3) & !3;
            let mut bytes = Vec::new();
            for word in [
                4_u32,
                (padded + 8) as u32,
                (padded + 4) as u32,
                tree.len() as u32,
            ] {
                bytes.extend(word.to_le_bytes());
            }
            bytes.extend(tree);
            bytes.resize(16 + padded, 0);
            bytes.extend(package);
            bytes.extend(source.as_bytes());
            fs::write(&file, bytes).unwrap();
        };
        write("5.9.3", &format!("{ANCHOR} coordinator:"));
        verify_archive(&file).unwrap();
        write("5.9.4", &format!("{ANCHOR} coordinator:"));
        assert!(verify_archive(&file).is_err());
        write("5.9.3", &format!("{ANCHOR}{ANCHOR} coordinator:"));
        assert!(verify_archive(&file).is_err());
    }
}
