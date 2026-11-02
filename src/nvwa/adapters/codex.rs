use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Result, ensure};
use toml_edit::{DocumentMut, Item, Table, value};

use super::Snapshot;

pub(super) fn default_path() -> PathBuf {
    crate::codex_app_config::default_codex_home().join("config.toml")
}

fn document(raw: &[u8]) -> Result<DocumentMut> {
    let text = std::str::from_utf8(raw).map_err(|_| anyhow::anyhow!("Codex MCP 配置不是 UTF-8"))?;
    text.parse()
        .map_err(|_| anyhow::anyhow!("Codex MCP 配置 TOML 无效，本次未修改"))
}

pub(super) fn read(raw: &[u8], name: &str) -> Result<Option<Snapshot>> {
    let doc = document(raw)?;
    let Some(servers) = doc.get("mcp_servers") else {
        return Ok(None);
    };
    let servers = servers
        .as_table_like()
        .ok_or_else(|| anyhow::anyhow!("Codex mcp_servers 格式无效"))?;
    let Some(item) = servers.get(name) else {
        return Ok(None);
    };
    ensure!(
        item.is_table() || item.is_inline_table(),
        "Codex 同名 MCP 条目格式无效"
    );
    let mut single = DocumentMut::new();
    single["mcp_servers"] = Item::Table(Table::new());
    single["mcp_servers"][name] = item.clone();
    Ok(Some(Snapshot::Toml(single.to_string())))
}

pub(super) fn desired(name: &str, endpoint: &str, bearer: &str) -> Snapshot {
    let mut doc = DocumentMut::new();
    doc["mcp_servers"] = Item::Table(Table::new());
    let mut server = Table::new();
    server["url"] = value(endpoint);
    server["startup_timeout_sec"] = value(30);
    server["tool_timeout_sec"] = value(60);
    server["enabled"] = value(true);
    let mut headers = Table::new();
    headers["Authorization"] = value(format!("Bearer {bearer}"));
    server["http_headers"] = Item::Table(headers);
    doc["mcp_servers"][name] = Item::Table(server);
    Snapshot::Toml(doc.to_string())
}

pub(super) fn merge(current: &Snapshot, old_name: &str, desired: &Snapshot) -> Result<Snapshot> {
    let (Snapshot::Toml(current), Snapshot::Toml(desired)) = (current, desired) else {
        anyhow::bail!("Codex 修复设置类型无效");
    };
    let mut doc = document(current.as_bytes())?;
    let new_doc = document(desired.as_bytes())?;
    let (new_name, new_entry) = new_doc["mcp_servers"]
        .as_table_like()
        .and_then(|servers| servers.iter().next())
        .ok_or_else(|| anyhow::anyhow!("Codex 修复缺少目标设置"))?;
    let servers = doc["mcp_servers"]
        .as_table_like_mut()
        .ok_or_else(|| anyhow::anyhow!("Codex 修复缺少受管设置"))?;
    let mut entry = servers
        .remove(old_name)
        .ok_or_else(|| anyhow::anyhow!("Codex 修复缺少受管条目"))?;
    let table = entry
        .as_table_like_mut()
        .ok_or_else(|| anyhow::anyhow!("Codex 修复条目格式无效"))?;
    for key in [
        "command",
        "args",
        "env",
        "env_vars",
        "cwd",
        "bearer_token_env_var",
        "type",
        "transport",
    ] {
        table.remove(key);
    }
    for key in ["url", "enabled"] {
        table.insert(key, new_entry[key].clone());
    }
    for key in ["startup_timeout_sec", "tool_timeout_sec"] {
        if table.get(key).is_none() {
            table.insert(key, new_entry[key].clone());
        }
    }
    if table
        .get("http_headers")
        .and_then(Item::as_table_like)
        .is_none()
    {
        table.insert("http_headers", Item::Table(Table::new()));
    }
    if table
        .get("env_http_headers")
        .is_some_and(|item| item.as_table_like().is_none())
    {
        table.remove("env_http_headers");
    }
    for key in ["http_headers", "env_http_headers"] {
        if let Some(headers) = table.get_mut(key).and_then(Item::as_table_like_mut) {
            let auth_names: Vec<_> = headers
                .iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .map(|(name, _)| name.to_owned())
                .collect();
            for name in auth_names {
                headers.remove(&name);
            }
            if key == "http_headers" {
                headers.insert(
                    "Authorization",
                    new_entry["http_headers"]["Authorization"].clone(),
                );
            }
        }
    }
    servers.insert(new_name, entry);
    Ok(Snapshot::Toml(doc.to_string()))
}

pub(super) fn replace(raw: &[u8], name: &str, entry: Option<&Snapshot>) -> Result<Vec<u8>> {
    replace_many(raw, &BTreeMap::from([(name.to_owned(), entry.cloned())]))
}

/// Produce a single document for an explicitly verified set of MCP targets.
pub(super) fn replace_many(
    raw: &[u8],
    entries: &BTreeMap<String, Option<Snapshot>>,
) -> Result<Vec<u8>> {
    let mut doc = document(raw)?;
    if doc.get("mcp_servers").is_none() {
        if entries.values().all(Option::is_none) {
            return Ok(raw.to_vec());
        }
        doc["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = doc["mcp_servers"]
        .as_table_like_mut()
        .ok_or_else(|| anyhow::anyhow!("Codex mcp_servers 格式无效"))?;
    for (name, entry) in entries {
        match entry {
            Some(Snapshot::Toml(text)) => {
                let single = document(text.as_bytes())?;
                let item = single
                    .get("mcp_servers")
                    .and_then(Item::as_table_like)
                    .and_then(|t| t.get(name.as_str()))
                    .ok_or_else(|| anyhow::anyhow!("Codex 受保护备份缺少目标条目"))?;
                servers.insert(name, item.clone());
            }
            Some(_) => anyhow::bail!("Codex 受保护备份类型无效"),
            None => {
                servers.remove(name);
            }
        }
    }
    Ok(doc.to_string().into_bytes())
}

pub(super) fn enabled(snapshot: &Snapshot, name: &str) -> Option<bool> {
    let Snapshot::Toml(text) = snapshot else {
        return None;
    };
    document(text.as_bytes())
        .ok()?
        .get("mcp_servers")?
        .as_table_like()?
        .get(name)?
        .as_table_like()?
        .get("enabled")
        .and_then(Item::as_bool)
        .or(Some(true))
}
