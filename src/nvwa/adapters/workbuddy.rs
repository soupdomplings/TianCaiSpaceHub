use std::{env, path::PathBuf};

use anyhow::{Result, ensure};
use serde_json::{Value, json};

use super::Snapshot;

pub(super) fn default_path() -> Result<PathBuf> {
    for key in ["WORKBUDDY_CONFIG_DIR", "CODEBUDDY_CONFIG_DIR"] {
        if let Some(path) = env::var_os(key).filter(|p| !p.is_empty()) {
            return Ok(PathBuf::from(path).join("mcp.json"));
        }
    }
    let home = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or_else(|| anyhow::anyhow!("无法确定 WorkBuddy 用户目录，请显式选择 MCP 配置路径"))?;
    let mut directory = String::from(".workbuddy");
    if let Ok(instance) = env::var("WORKBUDDY_INSTANCE_NUMBER") {
        let instance = instance.trim();
        ensure!(
            instance.bytes().all(|c| c.is_ascii_digit()),
            "WorkBuddy 实例目录无效，请显式选择 MCP 配置路径"
        );
        if !instance.is_empty() {
            directory.push('-');
            directory.push_str(instance);
        }
    }
    Ok(PathBuf::from(home).join(directory).join("mcp.json"))
}

fn document(raw: &[u8]) -> Result<Value> {
    let doc = if raw.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(raw)
            .map_err(|_| anyhow::anyhow!("WorkBuddy MCP JSON 无效，本次未修改"))?
    };
    ensure!(doc.is_object(), "WorkBuddy MCP 根配置必须是对象");
    if let Some(servers) = doc.get("mcpServers") {
        ensure!(servers.is_object(), "WorkBuddy mcpServers 格式无效");
    }
    Ok(doc)
}

pub(super) fn read(raw: &[u8], name: &str) -> Result<Option<Snapshot>> {
    let doc = document(raw)?;
    let entry = doc
        .get("mcpServers")
        .and_then(|servers| servers.get(name))
        .cloned();
    if let Some(value) = &entry {
        ensure!(value.is_object(), "WorkBuddy 同名 MCP 条目格式无效");
    }
    Ok(entry.map(Snapshot::Json))
}

pub(super) fn desired(endpoint: &str, bearer: &str) -> Snapshot {
    Snapshot::Json(json!({
        "type": "streamableHttp", "url": endpoint,
        "headers": {"Authorization": format!("Bearer {bearer}")},
        "timeout": 30000,
    }))
}

pub(super) fn replace(raw: &[u8], name: &str, entry: Option<&Snapshot>) -> Result<Vec<u8>> {
    let mut doc = document(raw)?;
    if doc.get("mcpServers").is_none() {
        if entry.is_none() {
            return Ok(raw.to_vec());
        }
        doc["mcpServers"] = json!({});
    }
    let servers = doc["mcpServers"]
        .as_object_mut()
        .expect("validated MCP object");
    match entry {
        Some(Snapshot::Json(value)) => {
            ensure!(value.is_object(), "WorkBuddy 受保护备份类型无效");
            servers.insert(name.into(), value.clone());
        }
        Some(_) => anyhow::bail!("WorkBuddy 受保护备份类型无效"),
        None => {
            servers.remove(name);
        }
    }
    serde_json::to_vec_pretty(&doc).map_err(|_| anyhow::anyhow!("无法编码 WorkBuddy MCP 配置"))
}

pub(super) fn enabled(snapshot: &Snapshot) -> Option<bool> {
    let Snapshot::Json(value) = snapshot else {
        return None;
    };
    Some(
        !value
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    )
}
