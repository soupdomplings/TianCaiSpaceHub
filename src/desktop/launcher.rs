//! User-triggered opening of additional clients. Discovery never executes files.
use super::{
    clients::ClientId,
    transaction::{self, FileChange, TransactionStore},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
#[cfg(windows)]
mod windows;

fn supported(client: ClientId) -> Result<()> {
    ensure!(
        !matches!(
            client,
            ClientId::Codex | ClientId::WorkBuddy | ClientId::Tiangong
        ),
        "此客户端使用原有启动入口"
    );
    Ok(())
}
pub fn locate(config: &Path, client: ClientId) -> Result<Option<PathBuf>> {
    supported(client)?;
    let path = super::state_root(config).join("launchers.json");
    if let Some(bytes) = transaction::read(&path)? {
        let saved: Value = serde_json::from_slice(&bytes)?;
        if let Some(path) = saved[client.key()].as_str() {
            let path = PathBuf::from(path);
            ensure!(
                path.exists(),
                "保存的程序位置已不存在，请在接入设置重新选择"
            );
            return Ok(Some(path));
        }
    }
    #[cfg(not(windows))]
    let stem = match client {
        ClientId::VsCode => "Code",
        ClientId::DeepSeekHarness => "dsh",
        ClientId::ZCode => "ZCode",
        ClientId::WebStorm => "webstorm64",
        _ => unreachable!(),
    };
    let mut paths = Vec::new();
    #[cfg(target_os = "macos")]
    {
        let app = match client {
            ClientId::VsCode => "Visual Studio Code",
            ClientId::DeepSeekHarness => "DeepSeek Harness",
            ClientId::ZCode => "ZCode",
            ClientId::WebStorm => "WebStorm",
            _ => unreachable!(),
        };
        for root in [
            PathBuf::from("/Applications"),
            super::targets::home()?.join("Applications"),
        ] {
            paths.push(root.join(format!("{app}.app")));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for root in std::env::split_paths(&path) {
            #[cfg(windows)]
            {
                if client == ClientId::VsCode {
                    paths.push(root.join("code.cmd"));
                }
                paths.extend(
                    windows::program_names(client)
                        .iter()
                        .map(|name| root.join(name)),
                );
                if client == ClientId::VsCode {
                    paths.push(root.join("code"));
                }
            }
            #[cfg(not(windows))]
            paths.push(root.join(stem.to_ascii_lowercase()));
        }
    }
    #[cfg(windows)]
    {
        // Match the VS Code configuration discovery order, so a portable
        // PATH installation opens the same instance that received its models.
        let from_path = || paths.iter().find_map(|path| windows::resolve(client, path));
        return Ok(if client == ClientId::VsCode {
            from_path().or_else(|| windows::locate(client))
        } else {
            windows::locate(client).or_else(from_path)
        });
    }
    #[cfg(not(windows))]
    Ok(paths.into_iter().find(|p| p.exists()))
}
pub fn save(config: &Path, client: ClientId, path: &Path) -> Result<()> {
    supported(client)?;
    ensure!(
        path.is_absolute() && path.exists(),
        "请选择已安装的客户端程序"
    );
    #[cfg(windows)]
    ensure!(
        path.is_file()
            && path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("exe")),
        "请选择 .exe 程序文件"
    );
    transaction::reject_links(path)?;
    #[cfg(target_os = "macos")]
    ensure!(
        path.is_file() || path.is_dir() && path.extension().is_some_and(|s| s == "app"),
        "请选择 .app 应用程序或可执行文件"
    );
    let root = super::state_root(config);
    let file = root.join("launchers.json");
    let before = transaction::read(&file)?;
    let mut value: Value = before
        .as_ref()
        .map(|b| serde_json::from_slice(b))
        .transpose()?
        .unwrap_or(json!({}));
    value[client.key()] = path.to_string_lossy().to_string().into();
    TransactionStore::new(root.join("launcher-changes"))?.apply(vec![FileChange::plan(
        file,
        before.as_deref(),
        Some(serde_json::to_vec_pretty(&value)?),
    )])?;
    Ok(())
}
pub fn open(config: &Path, client: ClientId) -> Result<Value> {
    static LAST: std::sync::Mutex<Option<(ClientId, std::time::Instant)>> =
        std::sync::Mutex::new(None);
    let mut last = LAST
        .lock()
        .map_err(|_| anyhow::anyhow!("程序启动状态暂不可用"))?;
    ensure!(
        !last
            .as_ref()
            .is_some_and(|(c, t)| *c == client && t.elapsed() < std::time::Duration::from_secs(3)),
        "已发起打开，请稍后查看客户端"
    );
    let path = locate(config, client)?.context("未找到客户端程序，请到接入设置选择程序位置")?;
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("/usr/bin/open");
        if path.is_dir() {
            c.arg("-a");
        }
        c.arg(&path);
        c
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = std::process::Command::new(&path);
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("无法打开客户端，请重新选择程序位置")?;
    *last = Some((client, std::time::Instant::now()));
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(json!({"opened":true,"message":"已发起打开；配置与模型连接状态会自动读取"}))
}
