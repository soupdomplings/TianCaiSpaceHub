use anyhow::{Result, ensure};
use serde_json::{Value, json};

use crate::gmclaw_desktop::DesktopClient;

use super::Snapshot;

const FIELDS: &[&str] = &[
    "server_id",
    "description",
    "server_url",
    "connect_type",
    "timeout_ms",
    "token_encrypted",
    "header_config",
    "config_param",
    "tools_json",
    "status",
    "is_connected",
    "conn_last_error",
    "retry_count",
];

pub(super) async fn read(client: &DesktopClient, name: &str) -> Result<Option<Snapshot>> {
    let Some(row) = client.mcp_connection(name).await? else {
        return Ok(None);
    };
    Ok(Some(snapshot(&row)?))
}

pub(super) fn snapshot(row: &Value) -> Result<Snapshot> {
    let row = row
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("天工 MCP 目标响应无效"))?;
    let mut entry = serde_json::Map::new();
    for key in FIELDS {
        if let Some(value) = row.get(*key) {
            let normalized = if *key == "token_encrypted" {
                Value::String(value.as_str().unwrap_or_default().into())
            } else if *key == "conn_last_error" && value.as_str().is_none_or(str::is_empty) {
                Value::Null
            } else if matches!(*key, "header_config" | "config_param" | "tools_json") {
                match value.as_str() {
                    Some(text) => serde_json::from_str(text).unwrap_or_else(|_| value.clone()),
                    None => value.clone(),
                }
            } else {
                value.clone()
            };
            entry.insert((*key).into(), normalized);
        }
    }
    Ok(Snapshot::Json(Value::Object(entry)))
}

pub(super) fn desired(name: &str, endpoint: &str, bearer: &str) -> Snapshot {
    Snapshot::Json(json!({
        "server_id": name, "description": "TianCaiSpaceHub NVWA MCP",
        "server_url": endpoint, "connect_type": 1, "timeout_ms": 30000,
        "token_encrypted": "", "header_config": {"Authorization": format!("Bearer {bearer}")},
        "config_param": {"transport":"streamable", "requestTimeout":30, "maximumTotalTimeout":60},
        "tools_json": [], "status": "active", "is_connected": 0,
        "conn_last_error": null, "retry_count": 0,
    }))
}

pub(super) async fn replace(
    client: &DesktopClient,
    name: &str,
    entry: Option<&Snapshot>,
    exists: bool,
) -> Result<()> {
    match entry {
        Some(Snapshot::Json(value)) => {
            ensure!(value.is_object(), "天工 MCP 受保护备份类型无效");
            if exists {
                client.mcp_update_connection(name, value.clone()).await
            } else {
                let mut value = value.clone();
                value["server_name"] = name.into();
                client.mcp_create_connection(value).await
            }
        }
        Some(_) => anyhow::bail!("天工 MCP 受保护备份类型无效"),
        None => client.mcp_delete_connection(name).await,
    }
}

pub(super) fn static_snapshot(snapshot: &Snapshot) -> Snapshot {
    let Snapshot::Json(value) = snapshot else {
        return snapshot.clone();
    };
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        for key in [
            "is_connected",
            "tools_json",
            "conn_last_error",
            "retry_count",
        ] {
            object.remove(key);
        }
    }
    Snapshot::Json(value)
}

pub(super) fn enabled(snapshot: &Snapshot) -> Option<bool> {
    let Snapshot::Json(value) = snapshot else {
        return None;
    };
    Some(value.get("status").and_then(Value::as_str) == Some("active"))
}

pub(super) fn connected(snapshot: &Snapshot) -> Option<bool> {
    let Snapshot::Json(value) = snapshot else {
        return None;
    };
    value
        .get("is_connected")
        .and_then(Value::as_i64)
        .map(|flag| flag == 1)
}
