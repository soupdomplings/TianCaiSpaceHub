//! Authenticated native TianGong registration, with protected recovery records.
use super::{
    clients::ClientId,
    resources::*,
    skills,
    targets::ClientTarget,
    transaction::{self, FileChange},
};
use crate::{gmclaw_desktop::DesktopClient, gmclaw_im::GmClawBridgeConfig};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

static NATIVE_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub async fn client(config: &GmClawBridgeConfig) -> Result<DesktopClient> {
    let status = crate::gmclaw_runtime::status(config).await;
    ensure!(
        status.state == crate::gmclaw_runtime::GmClawConnectionState::Connected,
        "请先在天工 Work 的接入设置中连接本机客户端"
    );
    DesktopClient::with_authorization(
        &config.endpoint,
        crate::gmclaw_runtime::connection_authorization(config).await?,
    )
}
fn static_value(kind: ResourceKind, value: Option<Value>) -> Option<Value> {
    value.map(|value| {
        let fields: &[&str] = if kind == ResourceKind::Skill {
            &[
                "name",
                "display_name",
                "version",
                "type",
                "entry",
                "description",
                "source_dir",
                "enabled",
                "skill_source",
            ]
        } else {
            &[
                "server_id",
                "description",
                "server_url",
                "connect_type",
                "timeout_ms",
                "token_encrypted",
                "header_config",
                "config_param",
                "status",
            ]
        };
        let mut object = serde_json::Map::new();
        for key in fields {
            if let Some(v) = value.get(*key) {
                let v = if matches!(*key, "header_config" | "config_param") {
                    v.as_str()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(v.clone())
                } else {
                    v.clone()
                };
                object.insert((*key).into(), v);
            }
        }
        object.into()
    })
}
async fn read(client: &DesktopClient, resource: &Resource) -> Result<Option<Value>> {
    let value = if resource.kind == ResourceKind::Skill {
        client
            .skills()
            .await?
            .into_iter()
            .find(|r| r["name"] == resource.name)
    } else {
        client.mcp_connection(&resource.name).await?
    };
    Ok(static_value(resource.kind, value))
}
async fn replace(
    client: &DesktopClient,
    resource: &Resource,
    value: Option<Value>,
    exists: bool,
) -> Result<()> {
    if resource.kind == ResourceKind::Skill {
        return client.skill_replace(&resource.name, value, exists).await;
    }
    match value {
        Some(mut v) => {
            if exists {
                client.mcp_update_connection(&resource.name, v).await
            } else {
                v["server_name"] = resource.name.clone().into();
                client.mcp_create_connection(v).await
            }
        }
        None => client.mcp_delete_connection(&resource.name).await,
    }
}
fn desired(lib: &Library, resource: &Resource, target: &ClientTarget, port: u16) -> Result<Value> {
    if resource.kind == ResourceKind::Skill {
        let package = skills::from_directory(&lib.root.join("skills").join(&resource.id))?;
        return Ok(
            json!({"name":resource.name,"display_name":resource.name,"version":"1.0.0","type":"skill","entry":"SKILL.md","description":package.metadata.description,"source_dir":target.skills.join(resource.skill_folder(target)?),"enabled":1,"skill_source":"custom"}),
        );
    }
    let server = lib.mcp_config(resource)?;
    let config = &server.config;
    let (url, headers, transport) = if server.transport() == "stdio" {
        (
            format!(
                "http://127.0.0.1:{port}/api/desktop/mcp-bridge/{}",
                resource.id
            ),
            json!({"Authorization":format!("Bearer {}",lib.bridge_token(&resource.id)?)}),
            "streamable",
        )
    } else {
        (
            config["url"].as_str().context("MCP 地址无效")?.into(),
            config
                .get("headers")
                .or_else(|| config.get("http_headers"))
                .cloned()
                .unwrap_or(json!({})),
            if server.transport() == "sse" {
                "sse"
            } else {
                "streamable"
            },
        )
    };
    Ok(
        json!({"server_id":resource.name,"description":resource.description,"server_url":url,"connect_type":1,"timeout_ms":30000,"token_encrypted":"","header_config":headers,"config_param":{"transport":transport,"requestTimeout":30,"maximumTotalTimeout":600},"status":"active"}),
    )
}

#[derive(Serialize, Deserialize)]
struct NativeUndo {
    kind: String,
    resource: Resource,
    target: ClientTarget,
    before: Option<Value>,
    after: Option<Value>,
    file_undo: Option<String>,
    #[serde(default)]
    previous_use: Option<UseRecord>,
}

pub async fn apply(
    lib: &Library,
    client: &DesktopClient,
    resource_id: &str,
    target_id: &str,
    using: bool,
    replace_conflict: bool,
    port: u16,
) -> Result<UseOutcome> {
    let _gate = NATIVE_GATE.lock().await;
    let mut catalog = lib.load()?;
    let resource = catalog
        .resources
        .iter()
        .find(|r| r.id == resource_id)
        .cloned()
        .context("所选资源不存在")?;
    let target = lib
        .targets(&catalog)?
        .into_iter()
        .find(|t| t.id == target_id && t.client == ClientId::Tiangong)
        .context("天工配置位置无效")?;
    let before = read(client, &resource).await?;
    let after = if using {
        let mut value = desired(lib, &resource, &target, port)?;
        if resource.kind == ResourceKind::Mcp {
            if let Some(old) = &before {
                for key in ["timeout_ms", "config_param"] {
                    if let Some(existing) = old.get(key) {
                        if key == "config_param" {
                            let transport = value[key]["transport"].clone();
                            value[key] = existing.clone();
                            value[key]["transport"] = transport;
                        } else {
                            value[key] = existing.clone();
                        }
                    }
                }
            }
        }
        Some(value)
    } else {
        None
    };
    let prior = catalog
        .uses
        .iter()
        .find(|u| u.resource_id == resource_id && u.target_id == target_id);
    let fp = |v: &Option<Value>| -> Result<String> {
        Ok(transaction::fingerprint(
            v.as_ref().map(serde_json::to_vec).transpose()?.as_deref(),
        ))
    };
    if let Some(prior) = prior {
        ensure!(
            replace_conflict || prior.entry_fingerprint.as_deref() == Some(&fp(&before)?),
            "天工已独立修改此配置，请核对后选择替换"
        );
    } else {
        ensure!(
            before.is_none() || before == after || replace_conflict,
            "天工已有同名配置，请核对后选择替换"
        );
    }
    let mut changes = Vec::new();
    let mut files = Vec::new();
    if resource.kind == ResourceKind::Skill {
        let location = target.skills.join(resource.skill_folder(&target)?);
        let existing = if location.join("SKILL.md").is_file() {
            Some(skills::from_directory(&location)?)
        } else {
            None
        };
        if let Some(existing) = &existing {
            ensure!(
                existing.fingerprint == resource.fingerprint || replace_conflict,
                "天工 Skill 内容已独立修改，未覆盖"
            );
        }
        let mut paths = std::collections::BTreeMap::new();
        if let Some(existing) = existing {
            for p in existing.files.keys() {
                paths.insert(location.join(p), None);
            }
        }
        if using {
            for (p, bytes) in
                skills::from_directory(&lib.root.join("skills").join(&resource.id))?.files
            {
                paths.insert(location.join(p), Some(bytes));
            }
        }
        for (path, after) in paths {
            let before = transaction::read(&path)?;
            if let Some(after) = &after {
                files.push(OwnedFile {
                    path: path.clone(),
                    fingerprint: transaction::fingerprint(Some(after)),
                });
            }
            changes.push(FileChange::plan(path, before.as_deref(), after));
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let file_undo = if changes.is_empty() {
        None
    } else {
        Some(uuid::Uuid::new_v4().to_string())
    };
    lib.transactions.put_record(
        &id,
        &NativeUndo {
            kind: "tiangong-resource-v1".into(),
            resource: resource.clone(),
            target: target.clone(),
            before: before.clone(),
            after: after.clone(),
            file_undo: file_undo.clone(),
            previous_use: prior.cloned(),
        },
    )?;
    // Recovery is reachable even if the process stops between native/file writes.
    let marker = lib.root.join("native-recovery").join(format!("{id}.json"));
    lib.transactions.apply(vec![FileChange::plan(marker,None,Some(serde_json::to_vec(&json!({"id":id,"resourceId":resource_id,"targetId":target_id,"name":resource.name,"client":target.client,"using":using,"at":crate::ai_gateway::request_log::now_ms()}))?))])?;
    if let Some(file_id) = &file_undo {
        lib.transactions.apply_as(file_id, changes)?;
    }
    ensure!(
        read(client, &resource).await? == before,
        "天工刚刚修改了配置，已保留恢复记录，请重新读取"
    );
    let write = replace(client, &resource, after.clone(), before.is_some()).await;
    let actual = read(client, &resource).await;
    let confirmed = actual.as_ref().is_ok_and(|actual| {
        actual == &after
            || resource.kind == ResourceKind::Mcp
                && actual
                    .as_ref()
                    .zip(after.as_ref())
                    .is_some_and(|(a, b)| mcp_matches(a, b) && a["status"] == b["status"])
    });
    if !confirmed {
        if actual.as_ref().ok() == Some(&before) {
            if let Some(file_id) = &file_undo {
                let _ = lib.transactions.restore(file_id);
            }
        }
        write?;
        anyhow::bail!("天工保存结果未确认，请重新读取；可撤销操作 {id}");
    }
    let actual = actual?;
    lib.transactions.put_record(
        &id,
        &NativeUndo {
            kind: "tiangong-resource-v1".into(),
            resource: resource.clone(),
            target: target.clone(),
            before: before.clone(),
            after: actual.clone(),
            file_undo: file_undo.clone(),
            previous_use: prior.cloned(),
        },
    )?;
    catalog
        .uses
        .retain(|u| !(u.resource_id == resource_id && u.target_id == target_id));
    if using {
        catalog.uses.push(UseRecord {
            resource_id: resource_id.into(),
            target_id: target_id.into(),
            location: if resource.kind == ResourceKind::Skill {
                target.skills.join(resource.skill_folder(&target)?)
            } else {
                target.mcp.clone()
            },
            files,
            entry_fingerprint: Some(fp(&actual)?),
            undo: id.clone(),
            pending: false,
        });
    }
    lib.transactions
        .apply(vec![lib.catalog_change(&catalog)?])?;
    let reload = client.reload_configuration().await;
    Ok(UseOutcome {
        target_id: target_id.into(),
        success: true,
        using: Some(using),
        message: reload.err().map(|e| e.to_string()).unwrap_or_else(|| {
            if using {
                "天工已使用"
            } else {
                "天工已停用"
            }
            .into()
        }),
        undo: Some(id),
    })
}
pub async fn restore(lib: &Library, client: &DesktopClient, id: &str) -> Result<()> {
    let _gate = NATIVE_GATE.lock().await;
    let record: NativeUndo = lib.transactions.get_record(id)?;
    ensure!(record.kind == "tiangong-resource-v1", "恢复记录类型不兼容");
    let current = read(client, &record.resource).await?;
    ensure!(
        current == record.after || current == record.before,
        "天工在保存后已被独立修改，未覆盖"
    );
    let plan = record
        .file_undo
        .as_ref()
        .map(|id| lib.transactions.restore_plan(id))
        .transpose()?;
    replace(
        client,
        &record.resource,
        record.before.clone(),
        current.is_some(),
    )
    .await?;
    ensure!(
        read(client, &record.resource).await? == record.before,
        "天工恢复结果未确认，请重新读取"
    );
    if let Some(plan) = plan {
        lib.transactions.apply(plan)?;
    }
    let mut catalog = lib.load()?;
    catalog
        .uses
        .retain(|u| !(u.resource_id == record.resource.id && u.target_id == record.target.id));
    if let Some(previous) = record.previous_use {
        catalog.uses.push(previous);
    }
    lib.transactions
        .apply(vec![lib.catalog_change(&catalog)?])?;
    lib.finish_restore(id)?;
    let _ = client.reload_configuration().await;
    Ok(())
}
pub fn native_undo(lib: &Library, id: &str) -> bool {
    lib.root
        .join("native-recovery")
        .join(format!("{id}.json"))
        .is_file()
}

pub async fn enrich(
    lib: &Library,
    view: &mut LibraryView,
    client: Result<DesktopClient>,
    port: u16,
) {
    let (skill_rows, mcp_rows) = match &client {
        Ok(client) => tokio::join!(client.skills(), client.mcp_connections()),
        Err(error) => {
            for row in &mut view.resources {
                for state in &mut row.targets {
                    if state.client == ClientId::Tiangong {
                        state.using = None;
                        state.error = Some(error.to_string());
                    }
                }
            }
            return;
        }
    };
    let mcp_rows = mcp_rows.and_then(|rows| {
        ensure!(rows.iter().all(|v|["server_id","server_name"].into_iter().any(|key|v[key].as_str().is_some_and(|n|!n.is_empty()))),
            "天工 MCP 列表含无法识别的条目，请重新读取；保留现有配置和使用记录");
        Ok(rows)
    });
    // Do not treat a failed native read as an empty list. A full successful list
    // can retire stale use records after deletion in TianGong itself.
    if let Ok(rows) = &mcp_rows {
        let names = rows.iter().flat_map(|v|["server_id","server_name"].into_iter().filter_map(move |key|v[key].as_str()).filter(|n|!n.is_empty()).map(str::to_string)).collect::<std::collections::BTreeSet<_>>();
        for target in view.targets.iter().filter(|t|t.client == ClientId::Tiangong) {
                if lib.reconcile_native_mcp_names(&target.id,&names,&view.revision).is_ok() {
                    if let Ok(current) = lib.load() {
                        view.revision = current.revision.clone();
                        for row in &mut view.resources {
                            for state in &mut row.targets {
                                if state.client == ClientId::Tiangong {
                                    state.managed = current.uses.iter().any(|u|u.resource_id == row.resource.id && u.target_id == state.target_id);
                                }
                            }
                        }
                    }
                }
        }
    }
    for row in &mut view.resources {
        for state in &mut row.targets {
            if state.client != ClientId::Tiangong {
                continue;
            }
            let result: Result<bool> = (|| {
                let target = view
                    .targets
                    .iter()
                    .find(|t| t.id == state.target_id)
                    .context("天工配置位置不存在")?;
                let rows = if row.resource.kind == ResourceKind::Skill {
                    &skill_rows
                } else {
                    &mcp_rows
                };
                let rows = rows.as_ref().map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let actual = rows.iter().find(|v| {
                    if row.resource.kind == ResourceKind::Skill {
                        v["name"] == row.resource.name
                    } else {
                        v["server_id"] == row.resource.name || v["server_name"] == row.resource.name
                    }
                });
                let Some(actual) = static_value(row.resource.kind, actual.cloned()) else {
                    return Ok(false);
                };
                if row.resource.kind == ResourceKind::Skill {
                    if actual["enabled"] != 1 && actual["enabled"] != true {
                        return Ok(false);
                    }
                    let expected = target.skills.join(row.resource.skill_folder(target)?);
                    ensure!(
                        actual["source_dir"]
                            .as_str()
                            .is_some_and(|v| std::path::Path::new(v) == expected),
                        "天工同名 Skill 使用了其他目录，请核对后选择替换"
                    );
                    let package = skills::from_directory(&expected)?;
                    ensure!(
                        package.fingerprint == row.resource.fingerprint,
                        "天工 Skill 内容已独立修改，请核对"
                    );
                    Ok(true)
                } else {
                    if actual["status"] != "active" {
                        return Ok(false);
                    }
                    ensure!(
                        mcp_matches(&actual, &desired(lib, &row.resource, target, port)?),
                        "天工同名 MCP 已独立调整，请核对后选择保留或替换"
                    );
                    Ok(true)
                }
            })();
            match result {
                Ok(using) => {
                    state.using = Some(using);
                    state.error = None;
                }
                Err(e) => {
                    state.using = None;
                    state.error = Some(e.to_string());
                }
            }
        }
    }
}

/// Import native definitions without changing the desktop or starting tools.
pub async fn discover(lib: &Library, client: &DesktopClient) -> Result<Vec<String>> {
    let catalog = lib.load()?;
    let Some(target) = lib
        .targets(&catalog)?
        .into_iter()
        .find(|t| t.client == ClientId::Tiangong)
    else {
        return Ok(vec![]);
    };
    let (skills, mcp) = tokio::join!(client.skills(), client.mcp_connections());
    let mut warnings = Vec::new();
    match skills {
        Ok(rows) => {
            for row in rows {
                if row["skill_source"] != "custom" {
                    continue;
                }
                let result = (|| -> Result<()> {
                    let path = std::path::PathBuf::from(
                        row["source_dir"].as_str().context("Skill 未提供源目录")?,
                    );
                    ensure!(
                        path.parent() == Some(target.skills.as_path()),
                        "Skill 目录不在客户端自定义目录中，未导入"
                    );
                    let resource = lib.import_skill(
                        skills::from_directory(&path)?,
                        "天工 Work".into(),
                        &lib.load()?.revision,
                    )?;
                    let mut catalog = lib.load()?;
                    let resource = catalog
                        .resources
                        .iter_mut()
                        .find(|r| r.id == resource.id)
                        .context("Skill 索引已变化")?;
                    resource.name = row["name"].as_str().context("Skill 缺少名称")?.into();
                    let key = if cfg!(windows) {
                        target.skills.to_string_lossy().to_lowercase()
                    } else {
                        target.skills.to_string_lossy().into_owned()
                    };
                    resource.skill_folders.insert(
                        key,
                        path.file_name()
                            .context("Skill 目录无效")?
                            .to_string_lossy()
                            .into_owned(),
                    );
                    let resource = resource.clone();
                    lib.transactions
                        .apply(vec![lib.catalog_change(&catalog)?])?;
                    let baseline = static_value(ResourceKind::Skill, Some(row.clone()));
                    lib.adopt(&resource, &target, baseline.as_ref())?;
                    Ok(())
                })();
                if let Err(e) = result {
                    warnings.push(format!("天工 Skill：{e}"));
                }
            }
        }
        Err(e) => warnings.push(e.to_string()),
    }
    match mcp {
        Ok(rows) => {
            for row in rows {
                let result = (|| -> Result<()> {
                    let name = row["server_id"]
                        .as_str()
                        .or_else(|| row["server_name"].as_str())
                        .context("MCP 缺少名称")?;
                    if name == crate::local_assistance::settings::MCP_NAME { return Ok(()); }
                    let url = row["server_url"].as_str().context("MCP 缺少地址")?;
                    // Managed STDIO bridges are already in the library. Never import
                    // the local capability token as a different remote resource.
                    if url.contains("/api/desktop/mcp-bridge/") {
                        return Ok(());
                    }
                    let headers = row.get("header_config").cloned().unwrap_or(json!({}));
                    let headers = headers
                        .as_str()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(headers);
                    let param = row.get("config_param").cloned().unwrap_or(json!({}));
                    let param = param
                        .as_str()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(param);
                    let server = super::mcp::McpServer {
                        name: name.into(),
                        config: json!({"url":url,"headers":headers,"type":if param["transport"]=="sse"{"sse"}else{"http"}}),
                    };
                    if lib
                        .load()?
                        .resources
                        .iter()
                        .any(|r| r.kind == ResourceKind::Mcp && r.name == name)
                    {
                        return Ok(());
                    }
                    let resource = lib.save_mcp(
                        None,
                        server,
                        row["description"].as_str().unwrap_or("").into(),
                        &lib.load()?.revision,
                    )?;
                    let baseline = static_value(ResourceKind::Mcp, Some(row.clone()));
                    lib.adopt(&resource, &target, baseline.as_ref())?;
                    Ok(())
                })();
                if let Err(e) = result {
                    warnings.push(format!("天工 MCP：{e}"));
                }
            }
        }
        Err(e) => warnings.push(e.to_string()),
    }
    Ok(warnings)
}

fn mcp_matches(actual: &Value, wanted: &Value) -> bool {
    let headers = |v: &Value| v.get("header_config").cloned().unwrap_or(json!({}));
    let transport = |v: &Value| {
        v["config_param"]["transport"]
            .as_str()
            .unwrap_or("streamable")
            .to_string()
    };
    actual["server_url"] == wanted["server_url"]
        && headers(actual) == headers(wanted)
        && transport(actual) == transport(wanted)
        && actual["connect_type"].as_i64().unwrap_or(1) == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn native_deletion_requires_a_complete_identifiable_mcp_list() {
        use axum::{Json, Router, http::StatusCode, response::IntoResponse};
        use std::sync::{Arc, Mutex};
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::new(&dir.path().join("hub.toml")).unwrap();
        let root = dir.path().join("client");
        let target = super::super::targets::target(ClientId::Tiangong,"fixture",root.clone(),root.join("skills"),root.join("fixture.db"),super::super::targets::McpFormat::Tiangong,"tiangong");
        lib.save_targets(lib.load().unwrap(),vec![target.clone()]).unwrap();
        let resource = lib.save_mcp(None,super::super::mcp::McpServer{name:"synthetic".into(),config:json!({"type":"http","url":"https://fixture.invalid/mcp"})},String::new(),&lib.load().unwrap().revision).unwrap();
        lib.adopt(&resource,&target,None).unwrap();
        let response = Arc::new(Mutex::new(None::<Value>));
        let read = response.clone();
        let app = Router::new()
            .route("/data/skills",axum::routing::get(||async{Json(json!({"skills":[]}))}))
            .route("/data/mcp/connections",axum::routing::get(move ||{let read=read.clone();async move{
                read.lock().unwrap().clone().map(Json).map(IntoResponse::into_response).unwrap_or_else(||StatusCode::SERVICE_UNAVAILABLE.into_response())
            }}));
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client=DesktopClient::fixture(&format!("http://{}",listener.local_addr().unwrap()));
        let task=tokio::spawn(async move{axum::serve(listener,app).await.unwrap();});
        for reply in [None,Some(json!({"connections":[{"status":"active"}]})),Some(json!({"connections":[{"server_id":"fixture-id","server_name":"synthetic","status":"active"}]})),Some(json!({"connections":[]}))] {
            let confirmed_absent=reply.as_ref().is_some_and(|v|v["connections"]==json!([]));
            *response.lock().unwrap()=reply;
            let mut view=lib.view().unwrap();
            enrich(&lib,&mut view,Ok(client.clone()),3210).await;
            let state=view.resources.iter().find(|r|r.resource.id==resource.id).unwrap().targets.iter().find(|t|t.target_id==target.id).unwrap();
            assert_eq!(state.managed,!confirmed_absent);
            assert_eq!(state.using,if confirmed_absent {Some(false)} else {None});
            assert_eq!(lib.load().unwrap().uses.iter().any(|u|u.target_id==target.id),!confirmed_absent);
        }
        assert!(lib.load().unwrap().resources.iter().any(|r|r.id==resource.id));
        task.abort();
    }
    #[test]
    fn volatile_connection_fields_do_not_claim_user_changes() {
        let a = json!({"server_id":"test","server_url":"https://example.invalid/mcp","is_connected":1,"tools_json":[1],"config_param":"{\"transport\":\"sse\"}"});
        let b = json!({"server_id":"test","server_url":"https://example.invalid/mcp","is_connected":0,"config_param":{"transport":"sse"}});
        assert_eq!(
            static_value(ResourceKind::Mcp, Some(a)),
            static_value(ResourceKind::Mcp, Some(b))
        );
    }
    #[tokio::test]
    async fn synthetic_native_registration_conflicts_and_undo_keep_skills_and_mcp_independent() {
        use axum::{Json, Router, http::StatusCode, response::IntoResponse};
        use std::sync::{Arc, Mutex};
        let saved: Arc<Mutex<std::collections::BTreeMap<String, Value>>> = Default::default();
        let store = saved.clone();
        let app=Router::new().fallback(move |r:axum::extract::Request| {
            let store=store.clone();async move {
                assert_eq!(r.headers()["authorization"],"Bearer fixture");
                let path=r.uri().path().to_string();let method=r.method().clone();
                if path=="/ctrl/config/reload"{return Json(json!({"ok":true})).into_response();}
                if path=="/data/skills" && method==axum::http::Method::GET {
                    return Json(json!({"skills":store.lock().unwrap().iter().filter(|(k,_)|k.starts_with("skill/")).map(|(_,v)|v.clone()).collect::<Vec<_>>()})).into_response();
                }
                let skill=path.starts_with("/data/skills");
                let bytes=axum::body::to_bytes(r.into_body(),1024*1024).await.unwrap();
                let mut body=serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
                let name=if method==axum::http::Method::POST {
                    body[if skill{"name"}else{"server_name"}].as_str().unwrap().to_string()
                } else {path.rsplit('/').next().unwrap().to_string()};
                let key=format!("{}/{name}",if skill{"skill"}else{"mcp"});
                let mut data=store.lock().unwrap();
                if method==axum::http::Method::GET {
                    return data.get(&key).cloned().map(Json).map(IntoResponse::into_response).unwrap_or_else(||StatusCode::NOT_FOUND.into_response());
                }
                if method==axum::http::Method::DELETE{data.remove(&key);}else{
                    if !skill {body.as_object_mut().unwrap().remove("server_name");body["server_id"]=name.into();body["is_connected"]=true.into();}
                    data.insert(key,body);
                }
                Json(json!({"ok":true})).into_response()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = DesktopClient::fixture(&format!("http://{}", listener.local_addr().unwrap()));
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::new(&dir.path().join("hub.toml")).unwrap();
        let root = dir.path().join("client");
        let target = super::super::targets::target(
            ClientId::Tiangong,
            "fixture",
            root.clone(),
            root.join("skills"),
            root.join("fixture.db"),
            super::super::targets::McpFormat::Tiangong,
            "tiangong",
        );
        lib.save_targets(lib.load().unwrap(), vec![target.clone()])
            .unwrap();
        let mcp=lib.save_mcp(None,super::super::mcp::McpServer{name:"synthetic".into(),config:json!({"type":"http","url":"https://example.invalid/mcp","headers":{"X-Synthetic":"test"}})},String::new(),&lib.load().unwrap().revision).unwrap();
        let first = apply(&lib, &client, &mcp.id, &target.id, true, false, 3210)
            .await
            .unwrap();
        assert!(first.success);
        assert_eq!(
            saved.lock().unwrap()["mcp/synthetic"]["header_config"]["X-Synthetic"],
            "test"
        );
        saved.lock().unwrap().get_mut("mcp/synthetic").unwrap()["server_url"] =
            "https://later.example.invalid/mcp".into();
        assert!(
            restore(&lib, &client, first.undo.as_deref().unwrap())
                .await
                .is_err()
        );
        assert!(
            apply(&lib, &client, &mcp.id, &target.id, false, false, 3210)
                .await
                .is_err()
        );
        let second = apply(&lib, &client, &mcp.id, &target.id, false, true, 3210)
            .await
            .unwrap();
        assert!(!saved.lock().unwrap().contains_key("mcp/synthetic"));
        restore(&lib, &client, second.undo.as_deref().unwrap())
            .await
            .unwrap();
        assert_eq!(
            saved.lock().unwrap()["mcp/synthetic"]["server_url"],
            "https://later.example.invalid/mcp"
        );
        let package = dir.path().join("package");
        std::fs::create_dir(&package).unwrap();
        std::fs::write(
            package.join("SKILL.md"),
            "---\nname: synthetic-skill\ndescription: Synthetic test\n---\nTest only",
        )
        .unwrap();
        std::fs::write(package.join("reference.txt"), "kept").unwrap();
        let skill = lib
            .import_skill(
                skills::from_directory(&package).unwrap(),
                "synthetic".into(),
                &lib.load().unwrap().revision,
            )
            .unwrap();
        let result = apply(&lib, &client, &skill.id, &target.id, true, false, 3210)
            .await
            .unwrap();
        assert!(saved.lock().unwrap().contains_key("skill/synthetic-skill"));
        assert_eq!(
            std::fs::read_to_string(
                target
                    .skills
                    .join(skill.skill_folder(&target).unwrap())
                    .join("reference.txt")
            )
            .unwrap(),
            "kept"
        );
        restore(&lib, &client, result.undo.as_deref().unwrap())
            .await
            .unwrap();
        assert!(!saved.lock().unwrap().contains_key("skill/synthetic-skill"));
        assert!(saved.lock().unwrap().contains_key("mcp/synthetic"));
        task.abort();
    }
}
