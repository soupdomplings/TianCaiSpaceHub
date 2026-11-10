//! Recoverable model operation joining the native files and gateway copies.
use super::{
    models::{self, ManagedFile},
    targets::ClientTarget,
    transaction::{self, TransactionStore},
};
use crate::{ai_gateway::config::ProviderConfig, config::AppConfig};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
struct Operation {
    version: u32,
    id: String,
    target: String,
    before: Vec<ProviderConfig>,
    after: Vec<ProviderConfig>,
    #[serde(default)]
    native: Option<super::webstorm_native::NativeUndo>,
}

fn copies(config: &AppConfig, target: &str) -> Vec<ProviderConfig> {
    config
        .ai_gateway
        .providers
        .iter()
        .filter(|p| {
            p.desktop_binding
                .as_ref()
                .is_some_and(|b| b.target_id == target)
        })
        .cloned()
        .collect()
}
fn same(a: &[ProviderConfig], b: &[ProviderConfig]) -> Result<bool> {
    Ok(serde_json::to_value(a)? == serde_json::to_value(b)?)
}
fn index_path(path: &Path) -> std::path::PathBuf {
    super::state_root(path).join("model-operations.json")
}
fn state(path: &Path, id: &str, target: &str, phase: &str) -> Result<()> {
    let file = index_path(path);
    let before = transaction::read(&file)?;
    let mut list: Vec<serde_json::Value> = before
        .as_ref()
        .map(|b| serde_json::from_slice(b))
        .transpose()?
        .unwrap_or_default();
    list.retain(|v| v["id"] != id);
    list.push(serde_json::json!({"id":id,"targetId":target,"phase":phase,"at":crate::ai_gateway::request_log::now_ms()}));
    let store = TransactionStore::new(super::state_root(path))?;
    store.apply(vec![transaction::FileChange::plan(
        file,
        before.as_deref(),
        Some(serde_json::to_vec(&list)?),
    )])?;
    Ok(())
}
pub fn pending(path: &Path) -> Result<Vec<serde_json::Value>> {
    Ok(transaction::read(&index_path(path))?
        .map(|v| serde_json::from_slice::<Vec<serde_json::Value>>(&v))
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .filter(|v| v["phase"] != "restored")
        .collect())
}

pub fn commit(
    path: &Path,
    before: &AppConfig,
    next: &mut AppConfig,
    target: &ClientTarget,
    replace: bool,
) -> Result<String> {
    let store = TransactionStore::new(super::state_root(path))?;
    let port = next.local_listen_port().context("本机服务端口无效")?;
    let bindings = next
        .ai_gateway
        .providers
        .iter()
        .filter(|p| p.enabled)
        .filter_map(|p| p.desktop_binding.clone())
        .collect::<Vec<_>>();
    let native =
        if target.client == super::clients::ClientId::WebStorm && target.model_mode == "native" {
            Some(super::webstorm_native::prepare(
                target, port, &bindings, replace,
            )?)
        } else {
            None
        };
    let changes = if native.is_some() {
        vec![]
    } else {
        models::plan(target, &bindings, port, true)?
    };
    let managed = changes
        .iter()
        .map(|c| {
            Ok(ManagedFile {
                path: c.path.clone(),
                fingerprint: transaction::fingerprint(c.after.as_deref()),
                owned: Some(models::owned_section(target, &c.path, c.after.as_deref())?),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    for provider in &mut next.ai_gateway.providers {
        if let Some(binding) = &mut provider.desktop_binding {
            if binding.target_id == target.id {
                binding.native_files = managed.clone();
            }
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    store.put_record(
        &id,
        &Operation {
            version: 1,
            id: id.clone(),
            target: target.id.clone(),
            before: copies(before, &target.id),
            after: copies(next, &target.id),
            native: native.clone(),
        },
    )?;
    state(path, &id, &target.id, "prepared")?;
    let applied = if let Some(native) = &native {
        super::webstorm_native::apply(native, false).map(|_| id.clone())
    } else {
        store.apply_as(&id, changes)
    };
    if let Err(error) = applied {
        let _ = state(path, &id, &target.id, "native-error");
        return Err(error);
    }
    if let Err(error) = next.save(&path.to_path_buf()) {
        let restored = if let Some(native) = &native {
            super::webstorm_native::apply(native, true)
        } else {
            store.restore(&id)
        };
        if restored.is_ok() {
            let _ = state(path, &id, &target.id, "restored");
            return Err(error);
        }
        let _ = state(path, &id, &target.id, "needs-recovery");
        anyhow::bail!("{error}；部分客户端配置已后改，未覆盖。可在客户端页恢复操作 {id}");
    }
    // The native files and AppConfig are already committed. A journal-index
    // failure must not invite another click that creates a duplicate copy.
    if state(path, &id, &target.id, "applied").is_err() {
        tracing::warn!("模型已保存，操作索引待重新核对");
    }
    Ok(id)
}

pub fn restore(path: &Path, id: &str) -> Result<AppConfig> {
    let store = TransactionStore::new(super::state_root(path))?;
    let operation: Operation = store.get_record(id)?;
    ensure!(
        operation.version == 1 && operation.id == id,
        "模型恢复记录不兼容"
    );
    let mut config = AppConfig::load_or_default(&path.to_path_buf())?;
    let current = copies(&config, &operation.target);
    ensure!(
        same(&current, &operation.after)? || same(&current, &operation.before)?,
        "此客户端的 Hub 模型后来已改变，未覆盖；请先核对当前条目"
    );
    // Preflight every file before changing the gateway. Restore is idempotent.
    if let Some(native) = &operation.native {
        super::webstorm_native::apply(native, true)?;
        config.ai_gateway.providers.retain(|p| {
            !p.desktop_binding
                .as_ref()
                .is_some_and(|b| b.target_id == operation.target)
        });
        config.ai_gateway.providers.extend(operation.before.clone());
        if let Err(error) = config.save(&path.to_path_buf()) {
            let _ = super::webstorm_native::apply(native, false);
            return Err(error);
        }
        let _ = state(path, id, &operation.target, "restored");
        return Ok(config);
    }
    let native = store.restore_plan(id)?;
    let previous = config.clone();
    config.ai_gateway.providers.retain(|p| {
        !p.desktop_binding
            .as_ref()
            .is_some_and(|b| b.target_id == operation.target)
    });
    config.ai_gateway.providers.extend(operation.before.clone());
    config.save(&path.to_path_buf())?;
    if let Err(error) = store.apply(native) {
        let mut rollback = AppConfig::load_or_default(&path.to_path_buf())?;
        if same(&copies(&rollback, &operation.target), &operation.before)? {
            rollback.ai_gateway.providers.retain(|p| {
                !p.desktop_binding
                    .as_ref()
                    .is_some_and(|b| b.target_id == operation.target)
            });
            rollback
                .ai_gateway
                .providers
                .extend(copies(&previous, &operation.target));
            let _ = rollback.save(&path.to_path_buf());
        }
        let _ = state(path, id, &operation.target, "needs-recovery");
        return Err(error);
    }
    if state(path, id, &operation.target, "restored").is_err() {
        tracing::warn!("模型已恢复，操作索引待重新核对");
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{
        clients::ClientId,
        targets::{self, McpFormat},
    };
    fn target(root: &Path, label: &str) -> ClientTarget {
        let root = root.join(label);
        targets::target(
            ClientId::VsCode,
            label,
            root.clone(),
            root.join("skills"),
            root.join("mcp.json"),
            McpFormat::Servers,
            "chat",
        )
    }
    #[test]
    fn model_commit_and_undo_keep_other_clients_and_later_gateway_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hub.json");
        let a = target(dir.path(), "a");
        let b = target(dir.path(), "b");
        let source = ProviderConfig {
            name: "synthetic".into(),
            base_url: "https://example.invalid/v1".into(),
            api_key: "synthetic".into(),
            models: vec!["same".into()],
            ..Default::default()
        };
        let mut config = AppConfig::default();
        config.ai_gateway.providers.push(source.clone());
        config.save(&path).unwrap();
        let mut next = config.clone();
        next.ai_gateway
            .providers
            .push(models::copy(&source, &a, "same", "").unwrap());
        let first = commit(&path, &config, &mut next, &a, false).unwrap();
        assert!(
            models::using(
                next.ai_gateway
                    .providers
                    .last()
                    .unwrap()
                    .desktop_binding
                    .as_ref()
                    .unwrap(),
                &a
            )
            .unwrap()
        );
        let mut other = next.clone();
        other
            .ai_gateway
            .providers
            .push(models::copy(&source, &b, "same", "").unwrap());
        commit(&path, &next, &mut other, &b, false).unwrap();
        other.ai_gateway.providers[0].api_key = "synthetic-later".into();
        other.save(&path).unwrap();
        let restored = restore(&path, &first).unwrap();
        assert_eq!(restored.ai_gateway.providers[0].api_key, "synthetic-later");
        assert!(copies(&restored, &a.id).is_empty());
        assert_eq!(copies(&restored, &b.id).len(), 1);
        assert!(
            models::using(
                copies(&restored, &b.id)[0]
                    .desktop_binding
                    .as_ref()
                    .unwrap(),
                &b
            )
            .unwrap()
        );
    }
    #[test]
    fn model_undo_refuses_external_file_changes_before_altering_gateway() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hub.json");
        let a = target(dir.path(), "a");
        let source = ProviderConfig {
            name: "synthetic".into(),
            models: vec!["same".into()],
            base_url: "https://example.invalid/v1".into(),
            api_key: "synthetic".into(),
            ..Default::default()
        };
        let mut config = AppConfig::default();
        config.save(&path).unwrap();
        let mut next = config.clone();
        next.ai_gateway
            .providers
            .push(models::copy(&source, &a, "same", "").unwrap());
        let id = commit(&path, &config, &mut next, &a, false).unwrap();
        let file = a.root.join("chatLanguageModels.json");
        std::fs::write(&file, b"[{\"name\":\"independently changed\"}]").unwrap();
        let gateway = std::fs::read(&path).unwrap();
        assert!(restore(&path, &id).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), gateway);
        assert_eq!(
            std::fs::read(&file).unwrap(),
            b"[{\"name\":\"independently changed\"}]"
        );
    }
}
