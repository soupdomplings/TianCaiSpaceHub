use super::{clients::ClientId, transaction};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
};
mod paths;

#[cfg(windows)]
pub(super) fn vscode_program(launcher: &Path) -> Option<PathBuf> {
    paths::vscode_program(launcher)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientTarget {
    pub id: String,
    pub client: ClientId,
    pub label: String,
    pub root: PathBuf,
    pub skills: PathBuf,
    pub mcp: PathBuf,
    pub mcp_format: McpFormat,
    pub present: bool,
    pub model_mode: String,
    /// Separate model storage, e.g. ZCode's migrated data directory. Resource
    /// roots and target identity remain unchanged when only models are moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_root: Option<PathBuf>,
    /// This profile inherits the default model catalog; keep it available for
    /// Skill/MCP, but never create an unused languageModels file there.
    #[serde(default)]
    pub models_inherited: bool,
}
impl ClientTarget {
    pub fn models_root(&self) -> &Path {
        self.model_root.as_deref().unwrap_or(&self.root)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum McpFormat {
    Codex,
    Servers,
    McpServers,
    ZCode,
    Harness,
    Tiangong,
    JetBrains,
    OpenCode,
}

pub fn home() -> Result<PathBuf> {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .context("无法确定用户目录，请在高级设置选择客户端配置目录")
}

pub fn discover() -> Result<Vec<ClientTarget>> {
    discover_with_launcher(None)
}

pub(super) fn discover_for_library(library_root: &Path) -> Result<Vec<ClientTarget>> {
    let saved = transaction::read(&library_root.join("launchers.json"))?
        .map(|raw| serde_json::from_slice::<serde_json::Value>(&raw))
        .transpose()?
        .and_then(|value| value["vscode"].as_str().map(PathBuf::from));
    discover_with_launcher(saved.as_deref())
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(|s| PathBuf::from(s.to_string_lossy().trim()))
        .filter(|p| p.is_absolute())
}

fn discover_with_launcher(saved_vscode: Option<&Path>) -> Result<Vec<ClientTarget>> {
    let user = home()?;
    #[cfg(windows)]
    let appdata = env_path("APPDATA").unwrap_or_else(|| user.join("AppData/Roaming"));
    #[cfg(not(windows))]
    let appdata = user.join("Library/Application Support");
    let codex = crate::codex_app_config::default_codex_home();
    let wb = crate::workbuddy_config::config_path()
        .parent()
        .context("WorkBuddy 配置路径无效")?
        .to_path_buf();
    let tg = crate::gmclaw_config::config_path()
        .parent()
        .context("天工配置路径无效")?
        .to_path_buf();
    let dsh = env::var_os("DSH_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| user.join(".dsh"));
    let platform = paths::Platform::current();
    let (zcode, zcode_models) = paths::zcode_locations(
        &user,
        env_path("HOME").as_deref(),
        env_path("ZCODE_HOME").as_deref(),
        platform,
    )?;
    let mut result = vec![
        target(
            ClientId::Codex,
            "默认",
            codex.clone(),
            codex.join("skills"),
            codex.join("config.toml"),
            McpFormat::Codex,
            "codex",
        ),
        target(
            ClientId::WorkBuddy,
            "默认",
            wb.clone(),
            wb.join("skills"),
            wb.join("mcp.json"),
            McpFormat::McpServers,
            "workbuddy",
        ),
        target(
            ClientId::Tiangong,
            "默认",
            tg.clone(),
            tg.join("skill"),
            tg.join("electron-data.db"),
            McpFormat::Tiangong,
            "tiangong",
        ),
        target(
            ClientId::ZCode,
            "默认",
            zcode.clone(),
            zcode.join("skills"),
            zcode.join("cli/config.json"),
            McpFormat::ZCode,
            "zcode",
        ),
        target(
            ClientId::WebStorm,
            "AI Assistant",
            user.join(".ai"),
            user.join(".agents/skills"),
            user.join(".ai/mcp/mcp.json"),
            McpFormat::JetBrains,
            "native",
        ),
        target(
            ClientId::WebStorm,
            "Air / ACP",
            user.join(".jetbrains"),
            user.join(".config/opencode/skills"),
            user.join(".jetbrains/tiancai-opencode.json"),
            McpFormat::OpenCode,
            "acp",
        ),
    ];
    if let Some(item) = result.iter_mut().find(|t| t.client == ClientId::ZCode) {
        if zcode_models != zcode {
            item.model_root = Some(zcode_models);
        }
    }
    result.extend(paths::harness_targets(&dsh)?);
    let mut launchers = saved_vscode
        .map(Path::to_path_buf)
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(path) = env::var_os("PATH") {
        for dir in env::split_paths(&path) {
            for name in if cfg!(windows) {
                &["code.cmd", "code.exe", "code"][..]
            } else {
                &["code"][..]
            } {
                launchers.push(dir.join(name));
            }
        }
    }
    let bundles = [
        PathBuf::from("/Applications/Visual Studio Code.app"),
        user.join("Applications/Visual Studio Code.app"),
    ];
    let vscode = paths::vscode_user_dir(
        &appdata,
        platform,
        env_path("VSCODE_PORTABLE").as_deref(),
        &launchers,
        &bundles,
    );
    let skills = user.join(".copilot/skills");
    result.extend(paths::vscode_targets(&vscode, &skills, platform)?);
    Ok(result)
}

pub fn target(
    client: ClientId,
    label: &str,
    root: PathBuf,
    skills: PathBuf,
    mcp: PathBuf,
    format: McpFormat,
    mode: &str,
) -> ClientTarget {
    let identity = format!(
        "{}\0{}\0{}",
        client.key(),
        if cfg!(windows) {
            root.to_string_lossy().to_lowercase()
        } else {
            root.to_string_lossy().into_owned()
        },
        mode
    );
    let id = transaction::fingerprint(Some(identity.as_bytes()));
    ClientTarget {
        id,
        client,
        label: label.into(),
        present: root.is_dir(),
        root,
        skills,
        mcp,
        mcp_format: format,
        model_mode: mode.into(),
        model_root: None,
        models_inherited: false,
    }
}

pub fn validate_override(path: &Path) -> Result<()> {
    transaction::reject_links(path)
}
