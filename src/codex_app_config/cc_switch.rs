//! Handoff of CC Switch's static model catalog to the active Hub gateway.
//! Its JSON file is owned by CC Switch. We only manage the root TOML reference.
use super::{
    AI_GATEWAY_PROVIDER_NAME, ManagedCodexAppBackupManifest, default_codex_home,
    managed_provider_names_in_config, parse_config_toml_document,
};
use anyhow::{Result, anyhow};
use std::{fs, io, path::Path};
use toml_edit::DocumentMut;

const KEY: &str = "model_catalog_json";
const FILENAME: &str = "cc-switch-model-catalog.json";

fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        let mut right = right.components();
        left.components().all(|a| {
            right
                .next()
                .is_some_and(|b| a.as_os_str().eq_ignore_ascii_case(b.as_os_str()))
        }) && right.next().is_none()
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

pub(super) fn reference<'a>(home: &Path, doc: &'a DocumentMut) -> Option<&'a str> {
    let raw = doc.get(KEY)?.as_str()?;
    // Compare paths, not basenames. CC Switch writes this file directly in the
    // active Codex home. No canonicalization / existence requirement: a missing
    // generated file must not leave the client pinned to an unusable catalog.
    same_path(&home.join(raw), &home.join(FILENAME)).then_some(raw)
}

/// Read-only check for model refresh, returning neither provider data nor TOML
/// snippets (parse errors can include credentials on the offending line).
pub(crate) fn active_hub_has_cc_switch_catalog(
    codex_home: Option<&Path>,
    backend_url: &str,
) -> Result<bool> {
    let home = codex_home
        .map(Path::to_path_buf)
        .unwrap_or_else(default_codex_home);
    let path = home.join("config.toml");
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(anyhow!("无法读取 Codex 的 config.toml。")),
    };
    let doc = parse_config_toml_document(&raw, &path)
        .map_err(|_| anyhow!("Codex 的 config.toml 语法无效。"))?;
    Ok(reference(&home, &doc).is_some()
        && active_gateway(&doc)
        && managed_provider_names_in_config(&doc, backend_url, false)
            .contains(AI_GATEWAY_PROVIDER_NAME))
}

fn active_gateway(doc: &DocumentMut) -> bool {
    doc.get("model_provider").and_then(toml_edit::Item::as_str) == Some(AI_GATEWAY_PROVIDER_NAME)
}

/// Called only when explicitly activating the default Hub provider. Return
/// whether the receipt changed, so repeated setup needn't rewrite it.
pub(super) fn hand_off(
    home: &Path,
    doc: &mut DocumentMut,
    manifest: &mut ManagedCodexAppBackupManifest,
) -> Result<bool> {
    let owned = reference(home, doc);
    if !same_path(&manifest.codex_home, home) {
        return if owned.is_some() {
            Err(anyhow!(
                "Codex 接管备份不属于当前目录，已保留 CC Switch 配置。"
            ))
        } else {
            Ok(false)
        };
    }
    if let Some(raw) = owned {
        let changed = manifest.removed_cc_switch_catalog.as_deref() != Some(raw);
        if changed {
            manifest.removed_cc_switch_catalog = Some(raw.to_owned());
        }
        doc.remove(KEY);
        Ok(changed)
    } else if doc.contains_key(KEY) || !active_gateway(doc) {
        // A replacement catalog, a new provider or an independently rebuilt
        // config supersedes the previous release's restore point.
        Ok(manifest.removed_cc_switch_catalog.take().is_some())
    } else {
        Ok(false)
    }
}

pub(super) fn restore(
    home: &Path,
    doc: &mut DocumentMut,
    manifest: &ManagedCodexAppBackupManifest,
) {
    if !doc.contains_key(KEY) && active_gateway(doc) && same_path(&manifest.codex_home, home) {
        if let Some(original) = manifest.removed_cc_switch_catalog.as_ref() {
            doc[KEY] = toml_edit::value(original);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_app_config::{
        ensure_managed_backup, managed_backup_paths, managed_gateway_config_is_current,
        parse_existing_config_toml, tests::test_configure_options, uninstall_with_managed_state,
        write_auth_json, write_config_toml,
    };
    use tempfile::tempdir;

    fn seed(home: &Path, catalog: &str) {
        let mut doc: DocumentMut = r#"
model_provider = "custom"
model = "keep-model"
[model_providers.custom]
name = "My provider"
[profiles.personal]
model_catalog_json = "cc-switch-model-catalog.json"
[projects.'project-one']
trust_level = "untrusted"
"#
        .parse()
        .unwrap();
        doc[KEY] = toml_edit::value(catalog);
        doc["sqlite_home"] = toml_edit::value(home.join("history store").to_str().unwrap());
        fs::write(home.join("config.toml"), doc.to_string()).unwrap();
        ensure_managed_backup(home, &home.join("config.toml"), &home.join("auth.json")).unwrap();
    }

    fn uninstall(home: &Path, backend: &str) -> DocumentMut {
        uninstall_with_managed_state(
            &managed_backup_paths(home),
            &home.join("config.toml"),
            &home.join("auth.json"),
            backend,
        )
        .unwrap();
        parse_existing_config_toml(&home.join("config.toml")).unwrap()
    }

    #[test]
    fn released_catalog_round_trips_without_touching_history_or_catalog_file() {
        for absolute in [true, false] {
            let tmp = tempdir().unwrap();
            let home = tmp.path();
            let catalog = home.join(FILENAME);
            let raw = if absolute {
                catalog.to_str().unwrap()
            } else {
                "./cc-switch-model-catalog.json"
            };
            // The absolute case deliberately has no JSON file.
            if !absolute {
                fs::write(&catalog, b"keep contents").unwrap();
            }
            seed(home, raw);
            let opts = test_configure_options(home.to_owned());
            let config = home.join("config.toml");
            let auth = home.join("auth.json");
            assert!(!active_hub_has_cc_switch_catalog(Some(home), &opts.backend_url).unwrap());
            write_config_toml(&config, &opts).unwrap();
            let doc = parse_existing_config_toml(&config).unwrap();
            assert!(doc.get(KEY).is_none());
            assert_eq!(doc["model"].as_str(), Some("keep-model"));
            assert_eq!(
                doc["model_provider"].as_str(),
                Some(AI_GATEWAY_PROVIDER_NAME)
            );
            assert_eq!(
                doc["model_providers"]["custom"]["name"].as_str(),
                Some("My provider")
            );
            assert_eq!(
                doc["sqlite_home"].as_str(),
                home.join("history store").to_str()
            );
            assert_eq!(
                doc["projects"]["project-one"]["trust_level"].as_str(),
                Some("untrusted")
            );
            assert_eq!(doc["profiles"]["personal"][KEY].as_str(), Some(FILENAME));
            write_auth_json(&auth, &opts).unwrap();
            assert!(managed_gateway_config_is_current(
                &config,
                &auth,
                &opts.backend_url
            ));
            // Simulate CC Switch taking the catalog back while the gateway stays active.
            let mut doc = doc;
            doc[KEY] = toml_edit::value(raw);
            // Use the existing duplicate-key repair parser, even for a read-only check.
            let rewritten = format!("model_provider = 'ai-gateway'\n{doc}");
            fs::write(&config, &rewritten).unwrap();
            assert!(active_hub_has_cc_switch_catalog(Some(home), &opts.backend_url).unwrap());
            assert_eq!(fs::read_to_string(&config).unwrap(), rewritten);
            assert!(!managed_gateway_config_is_current(
                &config,
                &auth,
                &opts.backend_url
            ));
            write_config_toml(&config, &opts).unwrap();
            write_config_toml(&config, &opts).unwrap();
            let restored = uninstall(home, &opts.backend_url);
            assert_eq!(restored[KEY].as_str(), Some(raw));
            assert_eq!(restored["model_provider"].as_str(), Some("custom"));
            if absolute {
                assert!(!catalog.exists());
            } else {
                assert_eq!(fs::read(&catalog).unwrap(), b"keep contents");
            }
        }
    }

    #[test]
    fn scoped_to_default_activation_and_exact_catalog_path() {
        let tmp = tempdir().unwrap();
        let home = tmp.path();
        let config = home.join("config.toml");
        let external = home.with_file_name("another-home").join(FILENAME);
        for raw in [
            "user-catalog.json",
            "custom/cc-switch-model-catalog.json",
            "../cc-switch-model-catalog.json",
            external.to_str().unwrap(),
        ] {
            seed(home, raw);
            let opts = test_configure_options(home.to_owned());
            write_config_toml(&config, &opts).unwrap();
            assert!(!active_hub_has_cc_switch_catalog(Some(home), &opts.backend_url).unwrap());
            assert_eq!(
                parse_existing_config_toml(&config).unwrap()[KEY].as_str(),
                Some(raw)
            );
            assert_eq!(uninstall(home, &opts.backend_url)[KEY].as_str(), Some(raw));
        }
        for activate in [true, false] {
            seed(home, FILENAME);
            let mut opts = test_configure_options(home.to_owned());
            if activate {
                opts.provider_name = Some("custom".into());
            }
            opts.activate_provider = activate;
            write_config_toml(&config, &opts).unwrap();
            assert_eq!(
                parse_existing_config_toml(&config).unwrap()[KEY].as_str(),
                Some(FILENAME)
            );
            assert_eq!(
                uninstall(home, &opts.backend_url)[KEY].as_str(),
                Some(FILENAME)
            );
        }
    }

    #[test]
    fn uninstall_respects_catalog_and_provider_changes_after_handoff() {
        for (later_provider, later_catalog) in
            [("ai-gateway", Some("my-models.json")), ("custom", None)]
        {
            let tmp = tempdir().unwrap();
            let home = tmp.path();
            seed(home, FILENAME);
            let opts = test_configure_options(home.to_owned());
            let config = home.join("config.toml");
            write_config_toml(&config, &opts).unwrap();
            let mut doc = parse_existing_config_toml(&config).unwrap();
            doc["model_provider"] = toml_edit::value(later_provider);
            if let Some(raw) = later_catalog {
                doc[KEY] = toml_edit::value(raw);
            }
            fs::write(config, doc.to_string()).unwrap();
            let restored = uninstall(home, &opts.backend_url);
            assert_eq!(
                restored.get(KEY).and_then(toml_edit::Item::as_str),
                later_catalog
            );
        }
    }

    #[test]
    fn preflight_does_not_expose_invalid_config_or_create_missing_files() {
        let tmp = tempdir().unwrap();
        let config = tmp.path().join("config.toml");
        assert!(!active_hub_has_cc_switch_catalog(Some(tmp.path()), "local").unwrap());
        assert!(!config.exists());
        fs::write(config, "sample_secret = 'not-a-real-key' invalid-suffix").unwrap();
        let err = active_hub_has_cc_switch_catalog(Some(tmp.path()), "local").unwrap_err();
        assert!(!format!("{err:#}").contains("not-a-real-key"));
    }
}
