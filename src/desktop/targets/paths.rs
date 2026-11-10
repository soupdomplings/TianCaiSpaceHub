//! Path and profile rules ported from Magpie e6cdeac9 (MIT).
//! Copyright (c) 2026 yetone. MIT license in THIRD_PARTY_NOTICES.md.
//! See THIRD_PARTY_NOTICES.md. Discovery reads metadata and never runs a launcher.
use super::{ClientTarget, McpFormat, target};
use crate::desktop::{clients::ClientId, formats, transaction};
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Platform {
    Windows,
    Mac,
}
impl Platform {
    pub(super) fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Mac
        }
    }
}

fn absolute(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let path = PathBuf::from(value);
    (!value.is_empty()
        && path.is_absolute()
        && !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)))
    .then_some(path)
}

// Program links may be followed for discovery; configuration writes still use
// transaction::reject_links. Normalize Windows extended paths before identity.
fn program_path(path: &Path) -> PathBuf {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(value) = value.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{value}"));
        }
        if let Some(value) = value.strip_prefix(r"\\?\") {
            return PathBuf::from(value);
        }
    }
    path
}

fn metadata(path: &Path) -> Option<Value> {
    transaction::read(path)
        .ok()
        .flatten()
        .and_then(|raw| formats::json(&raw, Value::Null).ok())
}

fn scoop_launcher(path: &Path) -> Option<PathBuf> {
    let raw = transaction::read(&path.with_extension("shim"))
        .ok()
        .flatten()?;
    let text = std::str::from_utf8(&raw).ok()?;
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "path")
            .then(|| absolute(value.trim().trim_matches('"')))
            .flatten()
    })
}

fn app_roots(launcher: &Path, platform: Platform) -> Option<(PathBuf, PathBuf)> {
    let launcher = program_path(&scoop_launcher(launcher).unwrap_or_else(|| launcher.into()));
    if platform == Platform::Mac {
        if launcher.extension().is_some_and(|s| s == "app") {
            return Some((launcher.join("Contents/Resources/app"), launcher));
        }
        for bundle in launcher.ancestors() {
            if bundle.extension().is_some_and(|s| s == "app") {
                return Some((bundle.join("Contents/Resources/app"), bundle.into()));
            }
        }
        return None;
    }
    let parent = launcher.parent()?;
    let install = if parent
        .file_name()
        .is_some_and(|s| s.eq_ignore_ascii_case("bin"))
    {
        parent.parent()?
    } else if launcher
        .file_stem()
        .is_some_and(|s| s.eq_ignore_ascii_case("Code"))
    {
        parent
    } else {
        return None;
    };
    let mut app = install.join("resources/app");
    if !app.join("product.json").is_file() {
        let mut versions = std::fs::read_dir(install)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path().join("resources/app"))
            .filter(|p| p.join("product.json").is_file())
            .collect::<Vec<_>>();
        versions.sort();
        if let Some(last) = versions.last() {
            app = last.clone();
        }
    }
    Some((app, install.into()))
}

// Some(None) is an identified installed build, which must stop candidate
// searching. None is an unrelated launcher/fork and may be skipped.
fn portable_at(launcher: &Path, platform: Platform) -> Option<Option<PathBuf>> {
    let (app, install) = app_roots(launcher, platform)?;
    let product = metadata(&app.join("product.json"));
    if let Some(name) = product
        .as_ref()
        .and_then(|p| p["applicationName"].as_str())
        .filter(|s| !s.is_empty())
    {
        if name != "code" {
            return None;
        }
    }
    if let Some(short) = product.as_ref().and_then(|p| p["nameShort"].as_str()) {
        let short = short.to_ascii_lowercase();
        if product
            .as_ref()
            .and_then(|p| p["applicationName"].as_str())
            .is_none()
            && short != "code"
            && short != "visual studio code"
        {
            return None;
        }
    }
    if product.as_ref().is_some_and(|p| p.get("target").is_some()) {
        return Some(None);
    }
    let data = if platform == Platform::Mac {
        let product = product?;
        if product["applicationName"] != "code" {
            return None;
        }
        let folder = product["portable"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("code-portable-data");
        if Path::new(folder).components().count() != 1 {
            return None;
        }
        install.parent()?.join(folder)
    } else {
        install.join("data")
    };
    Some(data.is_dir().then_some(data))
}

pub(super) fn vscode_user_dir(
    appdata: &Path,
    platform: Platform,
    portable: Option<&Path>,
    launchers: &[PathBuf],
    bundles: &[PathBuf],
) -> PathBuf {
    if let Some(data) = portable.filter(|p| p.is_absolute()) {
        return data.join("user-data/User");
    }
    for launcher in launchers.iter().filter(|p| p.is_absolute() && p.exists()) {
        if let Some(data) = portable_at(launcher, platform) {
            return data
                .map(|p| p.join("user-data/User"))
                .unwrap_or_else(|| appdata.join("Code/User"));
        }
    }
    // On macOS an app is used without a PATH launcher. Only agree on a
    // portable location when every identified bundle refers to that location.
    if platform == Platform::Mac {
        let mut found = None;
        for bundle in bundles.iter().filter(|p| p.is_dir()) {
            if let Some(data) = portable_at(bundle, platform) {
                let Some(data) = data else {
                    return appdata.join("Code/User");
                };
                if found.as_ref().is_some_and(|p| p != &data) {
                    return appdata.join("Code/User");
                }
                found = Some(data);
            }
        }
        if let Some(data) = found {
            return data.join("user-data/User");
        }
    }
    appdata.join("Code/User")
}

pub(super) fn vscode_targets(
    root: &Path,
    skills: &Path,
    platform: Platform,
) -> Result<Vec<ClientTarget>> {
    let mut result = vec![target(
        ClientId::VsCode,
        "默认",
        root.into(),
        skills.into(),
        root.join("mcp.json"),
        McpFormat::Servers,
        "chat",
    )];
    let Some(storage) = metadata(&root.join("globalStorage/storage.json")) else {
        return Ok(result);
    };
    for profile in storage["userDataProfiles"]
        .as_array()
        .into_iter()
        .flatten()
        .take(128)
    {
        let location = &profile["location"];
        let path = if let Some(name) = location.as_str().filter(|s| !s.is_empty()) {
            if name.starts_with("file:") {
                url::Url::parse(name)
                    .ok()
                    .and_then(|u| u.to_file_path().ok())
            } else {
                Some(root.join("profiles").join(name))
            }
        } else {
            location["fsPath"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| location["path"].as_str())
                .map(|s| {
                    let s = if platform == Platform::Windows
                        && s.as_bytes().get(2) == Some(&b':')
                        && s.starts_with('/')
                    {
                        &s[1..]
                    } else {
                        s
                    };
                    PathBuf::from(s)
                })
        };
        let Some(path) = path.filter(|p| p.is_absolute() && p.is_dir()) else {
            continue;
        };
        transaction::reject_links(&path).context("VS Code Profile 目录无法安全读取")?;
        if result.iter().any(|t| t.root == path) {
            continue;
        }
        let label = profile["name"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(|s| format!("Profile {s}"))
            .unwrap_or_else(|| {
                format!(
                    "Profile {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )
            });
        let mut item = target(
            ClientId::VsCode,
            &label,
            path.clone(),
            skills.into(),
            path.join("mcp.json"),
            McpFormat::Servers,
            "chat",
        );
        item.models_inherited = profile["useDefaultFlags"]["languageModels"] == true;
        if item.models_inherited {
            item.model_root = Some(root.into());
        }
        result.push(item);
    }
    Ok(result)
}

pub(super) fn zcode_locations(
    user: &Path,
    home_override: Option<&Path>,
    root_override: Option<&Path>,
    platform: Platform,
) -> Result<(PathBuf, PathBuf)> {
    let home = if platform == Platform::Windows {
        home_override.filter(|p| p.is_absolute()).unwrap_or(user)
    } else {
        user
    };
    let root = root_override
        .filter(|p| p.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".zcode"));
    let raw = transaction::read(&root.join("v2/setting.json"))
        .context("无法核对 ZCode 数据迁移设置，请检查配置位置与读取权限；未改写模型配置")?;
    let moved = raw
        .as_deref()
        .and_then(|raw| serde_json::from_slice::<Value>(raw).ok())
        .and_then(|value| {
            value["dataBaseDir"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        });
    let models = moved
        .map(|p| p.join(".zcode"))
        .unwrap_or_else(|| root.clone());
    transaction::reject_links(&models)
        .context("ZCode 模型目录包含链接或非规范路径，请核对客户端实际数据目录；未改写模型配置")?;
    Ok((root, models))
}

pub(super) fn harness_targets(root: &Path) -> Result<Vec<ClientTarget>> {
    let mut profiles = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("profiles")) {
        for entry in entries.take(128) {
            let entry = entry?;
            let path = entry.path().join("cordis.patch.yml");
            if path.is_file() {
                transaction::reject_links(&path)?;
                profiles.push(path);
            }
        }
    }
    profiles.sort_by_key(|p| {
        (
            p.parent()
                .and_then(Path::file_name)
                .is_none_or(|n| n != "web"),
            p.clone(),
        )
    });
    if profiles.is_empty() {
        // Preserve legacy config.yaml rather than create a patch the installed
        // generation never reads. New modern homes use a real desktop profile.
        profiles.push(if root.join("config.yaml").is_file() {
            root.join("config.yaml")
        } else {
            root.join("profiles/desktop/cordis.patch.yml")
        });
    }
    let mut result = vec![target(
        ClientId::DeepSeekHarness,
        "默认",
        root.into(),
        root.join("skills"),
        profiles[0].clone(),
        McpFormat::Harness,
        "harness",
    )];
    for file in profiles.into_iter().skip(1) {
        let profile = file.parent().context("Harness Profile 目录无效")?;
        let label = format!(
            "Profile {}",
            profile.file_name().unwrap_or_default().to_string_lossy()
        );
        let mut item = target(
            ClientId::DeepSeekHarness,
            &label,
            profile.into(),
            root.join("skills"),
            file,
            McpFormat::Harness,
            "harness",
        );
        item.models_inherited = true; // the default target manages the model catalog across profiles
        item.model_root = Some(root.into());
        result.push(item);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
