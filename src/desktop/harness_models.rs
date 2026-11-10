use super::{
    formats, mcp,
    models::{Binding, base},
    targets::ClientTarget,
    transaction::{self, FileChange},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf};

pub fn plan(target: &ClientTarget, bindings: &[&Binding], port: u16) -> Result<Vec<FileChange>> {
    let has_profile = std::fs::read_dir(target.root.join("profiles"))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.path().join("cordis.patch.yml").is_file());
    ensure!(
        bindings.is_empty() || has_profile || !target.root.join("config.yaml").is_file(),
        "此 Harness 目录仅有旧版模型配置。请先更新或打开 Harness 以建立实际 Profile，再回 Hub 添加模型；原配置未修改"
    );
    let mut changes = Vec::new();
    let mut providers = serde_json::Map::new();
    let credentials_path = target.root.join(".credentials.yaml");
    let raw = transaction::read(&credentials_path)?;
    let mut credentials: Value = if raw.as_ref().is_none_or(|r| r.is_empty()) {
        json!({"version":1,"refs":{}})
    } else {
        serde_yaml_ng::from_slice(raw.as_ref().unwrap()).context("Harness 凭据配置无效")?
    };
    ensure!(
        credentials["version"] == 1,
        "Harness 凭据版本不兼容，未覆盖"
    );
    let refs = formats::object_at_mut(&mut credentials, &["refs"])?;
    refs.retain(|k, _| !k.starts_with("TIANCAI_HUB_"));
    for binding in bindings {
        let key = format!(
            "TIANCAI_HUB_{}",
            binding.id.replace('-', "_").to_uppercase()
        );
        refs.insert(key.clone(), binding.local_token.clone().into());
        let mut model = json!({"id":binding.model,"name":binding.label});
        for field in [
            "contextWindow",
            "maxTokens",
            "input",
            "reasoningEfforts",
            "compat",
        ] {
            if let Some(value) = binding.capabilities.get(field) {
                model[field] = value.clone();
            }
        }
        providers.insert(format!("tiancai-hub-{}",binding.id),json!({"displayName":binding.label,"apiKeyEnv":key,"api":if binding.wire_protocol.is_empty(){"openai-completions"}else{&binding.wire_protocol},"baseURL":base(binding,port),"models":[model],"streamIdleTimeoutMs":600000}));
    }
    changes.push(FileChange::plan(
        credentials_path,
        raw.as_deref(),
        Some(serde_yaml_ng::to_string(&credentials)?.into_bytes()),
    ));
    let mut files = BTreeSet::<PathBuf>::new();
    if let Ok(entries) = std::fs::read_dir(target.root.join("profiles")) {
        for entry in entries.take(128) {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                transaction::reject_links(&entry.path())?;
                let path = entry.path().join("cordis.patch.yml");
                if path.is_file() {
                    files.insert(path);
                }
            }
        }
    }
    if files.is_empty() {
        for profile in ["desktop", "web"] {
            files.insert(
                target
                    .root
                    .join("profiles")
                    .join(profile)
                    .join("cordis.patch.yml"),
            );
        }
    }
    let home = target.root.join("cordis.patch.yml");
    if home.is_file() {
        files.insert(home);
    }
    for path in files {
        let raw = transaction::read(&path)?;
        let mut blocks = mcp::patch_blocks(raw.as_deref().unwrap_or_default())?;
        let mut found = false;
        for block in &mut blocks {
            if !block.contains("llm-pi-ai") {
                continue;
            }
            let mut rows: Value =
                serde_yaml_ng::from_str(block).context("Harness 的模型补丁无法安全读取")?;
            if let Some(items) = rows.as_array_mut() {
                for row in items {
                    if row["id"] != "llm-pi-ai" {
                        continue;
                    }
                    let entries = formats::object_at_mut(row, &["config", "providers"])?;
                    entries.retain(|k, _| !k.starts_with("tiancai-hub-"));
                    entries.extend(providers.clone());
                    found = true;
                }
            }
            if found {
                *block = serde_yaml_ng::to_string(&rows)?;
                break;
            }
        }
        if !found && !providers.is_empty() {
            blocks.push(serde_yaml_ng::to_string(&json!([{"id":"llm-pi-ai","name":"@deepseek-ai/dsh-llm-pi-ai","config":{"providers":providers}}]))?);
        }
        changes.push(FileChange::plan(
            path,
            raw.as_deref(),
            Some(blocks.join("\n").into_bytes()),
        ));
    }
    // 0.2's ACP composition reads settings.yaml independently of profiles.
    let path = target.root.join("settings.yaml");
    let raw = transaction::read(&path)?;
    if raw.is_some() || !bindings.is_empty() {
        let mut doc: Value = if raw.as_ref().is_none_or(|b| b.is_empty()) {
            json!({})
        } else {
            serde_yaml_ng::from_slice(raw.as_ref().unwrap())
                .context("Harness settings.yaml 无法读取，未修改")?
        };
        let entries = formats::object_at_mut(&mut doc, &["llm-pi-ai", "providers"])?;
        entries.retain(|k, _| !k.starts_with("tiancai-hub-"));
        entries.extend(providers);
        changes.push(FileChange::plan(
            path,
            raw.as_deref(),
            Some(formats::write_yaml_section(
                raw.as_deref().unwrap_or_default(),
                "llm-pi-ai",
                &doc["llm-pi-ai"],
            )?),
        ));
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ai_gateway::config::ProviderConfig,
        desktop::{clients::ClientId, models, targets},
    };
    #[test]
    fn legacy_only_harness_does_not_receive_modern_model_files_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("config.yaml");
        std::fs::write(
            &legacy,
            "- id: llm-deepseek\n  config:\n    baseURL: https://example.invalid\n",
        )
        .unwrap();
        let before = std::fs::read(&legacy).unwrap();
        let target = targets::target(
            ClientId::DeepSeekHarness,
            "fixture",
            dir.path().into(),
            dir.path().join("skills"),
            legacy.clone(),
            targets::McpFormat::Harness,
            "harness",
        );
        let source = ProviderConfig {
            name: "source".into(),
            models: vec!["model".into()],
            ..Default::default()
        };
        let binding = models::copy(&source, &target, "model", "")
            .unwrap()
            .desktop_binding
            .unwrap();
        assert!(
            plan(&target, &[&binding], 3210)
                .err()
                .unwrap()
                .to_string()
                .contains("旧版模型配置")
        );
        assert_eq!(std::fs::read(&legacy).unwrap(), before);
        assert!(!dir.path().join("profiles").exists());
        assert!(!dir.path().join(".credentials.yaml").exists());
    }

    #[test]
    fn modern_harness_merges_providers_and_credentials_without_taking_over_official_models() {
        let dir = tempfile::tempdir().unwrap();
        let patch = dir.path().join("profiles/desktop/cordis.patch.yml");
        std::fs::create_dir_all(patch.parent().unwrap()).unwrap();
        std::fs::write(&patch,"- id: llm-pi-ai\n  config:\n    future: true\n    providers:\n      external:\n        api: openai-completions\n- id: llm-deepseek\n  config:\n    keep: 7\n").unwrap();
        std::fs::write(
            dir.path().join(".credentials.yaml"),
            "version: 1\nrefs:\n  EXTERNAL_KEY: fixture-external\n",
        )
        .unwrap();
        let target = targets::target(
            ClientId::DeepSeekHarness,
            "fixture",
            dir.path().into(),
            dir.path().join("skills"),
            patch.clone(),
            targets::McpFormat::Harness,
            "harness",
        );
        let source = ProviderConfig {
            name: "source".into(),
            models: vec!["model".into()],
            ..Default::default()
        };
        let binding = models::copy(&source, &target, "model", "")
            .unwrap()
            .desktop_binding
            .unwrap();
        let changes = plan(&target, &[&binding], 3210).unwrap();
        let changed = changes.iter().find(|c| c.path == patch).unwrap();
        let value: Value = serde_yaml_ng::from_slice(changed.after.as_ref().unwrap()).unwrap();
        assert_eq!(value[0]["config"]["future"], true);
        assert_eq!(
            value[0]["config"]["providers"]["external"]["api"],
            "openai-completions"
        );
        assert_eq!(value[1]["config"]["keep"], 7);
        let key = format!("tiancai-hub-{}", binding.id);
        assert!(
            value[0]["config"]["providers"][&key]["baseURL"]
                .as_str()
                .unwrap()
                .contains("/clients/deepseek-harness/")
        );
        let credentials = changes
            .iter()
            .find(|c| c.path.file_name().is_some_and(|n| n == ".credentials.yaml"))
            .unwrap();
        let value: Value = serde_yaml_ng::from_slice(credentials.after.as_ref().unwrap()).unwrap();
        assert_eq!(value["refs"]["EXTERNAL_KEY"], "fixture-external");
        assert!(!dir.path().join("cordis.patch.yml").exists());
    }
}
