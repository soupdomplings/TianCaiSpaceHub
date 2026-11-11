use super::{
    formats, mcp,
    models::{Binding, base},
    targets::ClientTarget,
    transaction::{self, FileChange},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf};

fn visit_model_rows(
    rows: &mut Value,
    visit: &mut impl FnMut(&mut Value) -> Result<()>,
) -> Result<bool> {
    let mut found = false;
    if let Some(items) = rows.as_array_mut() {
        for row in items {
            if row.get("insert").is_some() {
                found |= visit_model_rows(&mut row["insert"], visit)?;
            } else if row["id"] == "llm-pi-ai" {
                visit(row)?;
                found = true;
            }
            if row["group"] == true {
                found |= visit_model_rows(&mut row["config"], visit)?;
            }
        }
    }
    Ok(found)
}

pub(super) fn owned_providers(raw: &[u8]) -> Result<Value> {
    let mut selected: Vec<Value> = Vec::new();
    for block in mcp::patch_blocks(raw)? {
        if !block.contains("llm-pi-ai") {
            continue;
        }
        let mut rows: Value = serde_yaml_ng::from_str(&block)?;
        visit_model_rows(&mut rows, &mut |row| {
            selected.push(
                row["config"]["providers"]
                    .as_object()
                    .map(|entries| {
                        entries
                            .iter()
                            .filter(|(key, _)| key.starts_with("tiancai-hub-"))
                            .map(|(key, value)| (key.clone(), value.clone()))
                            .collect::<serde_json::Map<_, _>>()
                    })
                    .unwrap_or_default()
                    .into(),
            );
            Ok(())
        })?;
    }
    Ok(selected.into())
}

pub(super) fn same_profile_snapshot(current: &Value, expected: &Value) -> bool {
    // ConfigEditor may append an id-targeted override when importing settings
    // into an inserted plugin. Compare the effective last config, not the
    // number of patch nodes. Older snapshots also omitted empty blocks.
    let effective = |value: &Value| {
        value
            .as_array()
            .and_then(|rows| rows.last())
            .filter(|row| row.as_object().is_some_and(|map| !map.is_empty()))
            .cloned()
    };
    effective(current) == effective(expected)
}

pub(super) fn settings_imported(target: &ClientTarget, expected: &Value) -> Result<bool> {
    let path = target.root.join("profiles/desktop/cordis.patch.yml");
    let Some(raw) = transaction::read(&path)? else {
        return Ok(false);
    };
    // Only the active profile is evidence; .imported is an archive, not a
    // configuration source. The last targeted config wins in Cordis.
    Ok(owned_providers(&raw)?
        .as_array()
        .and_then(|rows| rows.last())
        == Some(expected))
}

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
        for field in ["contextWindow", "maxTokens", "input", "compat"] {
            if let Some(value) = binding.capabilities.get(field) {
                model[field] = value.clone();
            }
        }
        if let Some(efforts) = binding.capabilities.get("reasoningEfforts") {
            if let Some(levels) = efforts.as_array() {
                // Harness accepts a level-to-wire dictionary, not our shared
                // capability list. An empty list leaves its defaults intact.
                let mut offered = serde_json::Map::new();
                for level in levels {
                    let wire = level.as_str().context("Harness 思考强度需要为名称")?;
                    let name = if wire == "none" { "off" } else { wire };
                    ensure!(
                        ["off", "minimal", "low", "medium", "high", "xhigh", "max"].contains(&name),
                        "Harness 不支持思考强度 {wire}；可选 off、minimal、low、medium、high、xhigh、max"
                    );
                    offered.insert(
                        name.into(),
                        if wire == "off" {
                            Value::Null
                        } else {
                            wire.into()
                        },
                    );
                }
                if !offered.is_empty() {
                    model["reasoningEfforts"] = if offered.keys().all(|level| level == "off") {
                        Value::Bool(false)
                    } else {
                        offered.into()
                    };
                }
            } else {
                model["reasoningEfforts"] = efforts.clone();
            }
        }
        if let Some(images) = binding
            .capabilities
            .get("supportsImage")
            .and_then(Value::as_bool)
        {
            model["input"] = if images {
                json!(["text", "image"])
            } else {
                json!(["text"])
            };
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
    // Desktop always boots this profile, even when only a web profile existed
    // when Hub first imported the model.
    if !bindings.is_empty() {
        files.insert(target.root.join("profiles/desktop/cordis.patch.yml"));
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
            let changed = visit_model_rows(&mut rows, &mut |row| {
                let entries = formats::object_at_mut(row, &["config", "providers"])?;
                entries.retain(|k, _| !k.starts_with("tiancai-hub-"));
                entries.extend(providers.clone());
                Ok(())
            })?;
            found |= changed;
            if changed {
                *block = serde_yaml_ng::to_string(&rows)?;
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
    // Desktop 0.2 imports and renames this legacy document. Do not recreate it
    // on every save; if one still exists, keep its owned routes consistent.
    let path = target.root.join("settings.yaml");
    let raw = transaction::read(&path)?;
    if raw.is_some() {
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
        desktop::{clients::ClientId, models, models::ManagedFile, targets},
    };

    fn fixture(root: &std::path::Path) -> (ClientTarget, Binding) {
        let target = targets::target(
            ClientId::DeepSeekHarness,
            "fixture",
            root.into(),
            root.join("skills"),
            root.join("profiles/web/cordis.patch.yml"),
            targets::McpFormat::Harness,
            "harness",
        );
        let source = ProviderConfig {
            name: "source".into(),
            models: vec!["gpt-6.1-sol".into()],
            ..Default::default()
        };
        let binding = models::copy(&source, &target, "gpt-6.1-sol", "fixture model")
            .unwrap()
            .desktop_binding
            .unwrap();
        (target, binding)
    }

    fn save_fixture(target: &ClientTarget, binding: &mut Binding, changes: &[FileChange]) {
        binding.native_files = changes
            .iter()
            .map(|change| {
                std::fs::create_dir_all(change.path.parent().unwrap()).unwrap();
                std::fs::write(&change.path, change.after.as_ref().unwrap()).unwrap();
                ManagedFile {
                    path: change.path.clone(),
                    fingerprint: transaction::fingerprint(change.after.as_deref()),
                    owned: Some(
                        models::owned_section(target, &change.path, change.after.as_deref())
                            .unwrap(),
                    ),
                }
            })
            .collect();
    }

    #[test]
    fn web_only_home_also_configures_desktop_without_creating_removed_settings_document() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (target, mut binding) = fixture(&root);
        std::fs::create_dir_all(target.mcp.parent().unwrap()).unwrap();
        std::fs::write(&target.mcp, "[]\n").unwrap();
        // Builtin-source capabilities include an empty reasoning list.
        binding.capabilities = json!({"contextWindow":65536,"maxTokens":8192,"reasoningEfforts":[],"supportsImage":true});
        let changes = plan(&target, &[&binding], 3210).unwrap();
        let path = root.join("profiles/desktop/cordis.patch.yml");
        let desktop = changes.iter().find(|change| change.path == path).unwrap();
        let value: Value = serde_yaml_ng::from_slice(desktop.after.as_ref().unwrap()).unwrap();
        let model =
            &value[0]["config"]["providers"][format!("tiancai-hub-{}", binding.id)]["models"][0];
        assert_eq!(model["id"], "gpt-6.1-sol");
        assert_eq!(model["input"], json!(["text", "image"]));
        assert!(model.get("reasoningEfforts").is_none());
        assert!(
            !changes
                .iter()
                .any(|change| change.path.ends_with("settings.yaml"))
        );
        save_fixture(&target, &mut binding, &changes);
        assert!(models::using(&binding, &target).unwrap());
    }

    #[test]
    fn reasoning_levels_use_harness_native_dictionary_and_reject_unknown_levels_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let (target, mut binding) = fixture(dir.path());
        binding.capabilities = json!({"reasoningEfforts":["none","low","high","xhigh","max"]});
        let changes = plan(&target, &[&binding], 3210).unwrap();
        let patch = changes
            .iter()
            .find(|change| change.path.ends_with("desktop/cordis.patch.yml"))
            .unwrap();
        let value: Value = serde_yaml_ng::from_slice(patch.after.as_ref().unwrap()).unwrap();
        assert_eq!(
            value[0]["config"]["providers"][format!("tiancai-hub-{}", binding.id)]["models"][0]["reasoningEfforts"],
            json!({"off":"none","low":"low","high":"high","xhigh":"xhigh","max":"max"})
        );
        binding.capabilities = json!({"reasoningEfforts":["ultra"]});
        assert!(
            plan(&target, &[&binding], 3210)
                .err()
                .unwrap()
                .to_string()
                .contains("不支持思考强度 ultra")
        );
        assert!(!target.root.join(".credentials.yaml").exists());
    }

    #[test]
    fn inserted_plugin_and_later_override_both_keep_owned_models_visible() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (target, mut binding) = fixture(&root);
        let patch = root.join("profiles/desktop/cordis.patch.yml");
        std::fs::create_dir_all(patch.parent().unwrap()).unwrap();
        std::fs::write(&patch, "- insert:\n  - id: llm-pi-ai\n    name: '@deepseek-ai/dsh-llm-pi-ai'\n    config:\n      providers:\n        manual:\n          displayName: Manual\n- id: llm-pi-ai\n  config:\n    providers:\n      later:\n        displayName: Later\n").unwrap();
        let changes = plan(&target, &[&binding], 3210).unwrap();
        let changed = changes.iter().find(|change| change.path == patch).unwrap();
        let value: Value = serde_yaml_ng::from_slice(changed.after.as_ref().unwrap()).unwrap();
        let key = format!("tiancai-hub-{}", binding.id);
        assert_eq!(value.as_array().unwrap().len(), 2);
        assert_eq!(
            value[0]["insert"][0]["config"]["providers"]["manual"]["displayName"],
            "Manual"
        );
        assert_eq!(
            value[1]["config"]["providers"]["later"]["displayName"],
            "Later"
        );
        assert_eq!(
            value[0]["insert"][0]["config"]["providers"][&key],
            value[1]["config"]["providers"][&key]
        );
        save_fixture(&target, &mut binding, &changes);
        assert!(models::using(&binding, &target).unwrap());
        let mut later = value;
        later[1]["config"]["providers"]
            .as_object_mut()
            .unwrap()
            .remove(&key);
        std::fs::write(&patch, serde_yaml_ng::to_string(&later).unwrap()).unwrap();
        assert!(models::using(&binding, &target).is_err());
    }

    #[test]
    fn legacy_settings_import_requires_matching_active_profile_and_never_reads_imported_archive() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (target, mut binding) = fixture(&root);
        let legacy = root.join("settings.yaml");
        let desktop = root.join("profiles/desktop/cordis.patch.yml");
        std::fs::create_dir_all(desktop.parent().unwrap()).unwrap();
        std::fs::write(
            &desktop,
            "- insert:\n  - id: llm-pi-ai\n    name: '@deepseek-ai/dsh-llm-pi-ai'\n",
        )
        .unwrap();
        std::fs::write(&legacy, "unrelated:\n  keep: 7\nllm-pi-ai:\n  providers:\n    external:\n      displayName: External\n      api: openai-completions\n      baseURL: https://example.invalid/v1\n      models:\n      - id: external-model\n").unwrap();
        let changes = plan(&target, &[&binding], 3210).unwrap();
        save_fixture(&target, &mut binding, &changes);
        let value: Value = serde_yaml_ng::from_slice(&std::fs::read(&legacy).unwrap()).unwrap();
        assert_eq!(value["unrelated"]["keep"], 7);
        assert_eq!(
            value["llm-pi-ai"]["providers"]["external"]["displayName"],
            "External"
        );
        std::fs::rename(&legacy, root.join("settings.yaml.imported")).unwrap();
        // Real SettingsForms merges the legacy section; ConfigEditor appends
        // this id-targeted override when the plugin exists only in an insert.
        let mut profile: Value =
            serde_yaml_ng::from_slice(&std::fs::read(&desktop).unwrap()).unwrap();
        profile.as_array_mut().unwrap().push(json!({"id":"llm-pi-ai","name":"@deepseek-ai/dsh-llm-pi-ai","config":value["llm-pi-ai"]}));
        std::fs::write(&desktop, serde_yaml_ng::to_string(&profile).unwrap()).unwrap();
        assert!(models::using(&binding, &target).unwrap());
        assert!(
            !plan(&target, &[&binding], 3210)
                .unwrap()
                .iter()
                .any(|change| change.path == legacy)
        );
        // A retained archive cannot make an absent/modified live profile usable.
        std::fs::write(&desktop, "[]\n").unwrap();
        assert!(models::using(&binding, &target).is_err());
    }

    #[test]
    fn old_snapshots_ignore_empty_layers_but_require_the_last_active_owned_config() {
        let owned = json!({"tiancai-hub-fixture":{"displayName":"Fixture"}});
        assert!(same_profile_snapshot(&json!([{}]), &json!([])));
        assert!(same_profile_snapshot(
            &json!([{}, owned.clone()]),
            &json!([owned.clone()])
        ));
        assert!(same_profile_snapshot(
            &json!([owned.clone(), owned.clone()]),
            &json!([owned.clone()])
        ));
        assert!(!same_profile_snapshot(
            &json!([owned.clone(), {}]),
            &json!([owned])
        ));
    }
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
