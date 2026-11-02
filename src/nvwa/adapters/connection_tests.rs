//! Offline transactions use temporary config and DPAPI backup files only.
use super::*;
use serde_json::json;

fn endpoint(client: ClientKind) -> &'static str {
    if client == ClientKind::Codex {
        "http://127.0.0.1:3849/mcp/fixture-profile/codex"
    } else {
        "http://127.0.0.1:3849/mcp/fixture-profile/workbuddy"
    }
}
const KEY: &str = "fixture-local-key-never-used-for-http";

fn fixture(client: ClientKind) -> (tempfile::TempDir, AdapterContext, AdapterTarget) {
    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("hub.toml");
    let secrets = SecretStore::new(ConfigStore::new(&config_path).unwrap().root().into()).unwrap();
    let target = AdapterTarget {
        profile_id: "fixture-profile".into(),
        client,
        server_name: "MCP196".into(),
        override_path: Some(directory.path().join(if client == ClientKind::Codex {
            "config.toml"
        } else {
            "mcp.json"
        })),
    };
    let raw = if client == ClientKind::Codex {
        "# unrelated settings\nmodel='fixture-model'\n[mcp_servers.other]\ncommand='fixture-other'\n"
    } else {
        r#"{"theme":"fixture","mcpServers":{"other":{"command":"fixture-other"}}}"#
    };
    fs::write(target.override_path.as_ref().unwrap(), raw).unwrap();
    (
        directory,
        AdapterContext {
            config_path,
            secrets,
            desktop: None,
        },
        target,
    )
}

fn snapshot(target: &AdapterTarget) -> Snapshot {
    let raw = fs::read(target.override_path.as_ref().unwrap()).unwrap();
    read_file(target, &raw).unwrap().unwrap()
}

fn write_snapshot(target: &AdapterTarget, snapshot: &Snapshot) {
    let path = target.override_path.as_ref().unwrap();
    let raw = fs::read(path).unwrap();
    let updated = match target.client {
        ClientKind::Codex => codex::replace(&raw, &target.server_name, Some(snapshot)),
        ClientKind::Workbuddy => workbuddy::replace(&raw, &target.server_name, Some(snapshot)),
        _ => unreachable!(),
    }
    .unwrap();
    fs::write(path, updated).unwrap();
}

fn customize(target: &AdapterTarget, stale_bridge: bool) {
    let current = snapshot(target);
    let custom = match current {
        Snapshot::Toml(text) => {
            let mut doc: toml_edit::DocumentMut = text.parse().unwrap();
            let entry = &mut doc["mcp_servers"][&target.server_name];
            entry["tool_timeout_sec"] = toml_edit::value(125);
            entry["enabled_tools"] =
                toml_edit::value(toml_edit::Array::from_iter(["fixture.tool"]));
            entry["http_headers"]["X-Fixture"] = toml_edit::value("keep");
            if stale_bridge {
                entry["url"] = toml_edit::value("http://127.0.0.1:9999/old");
                entry["enabled"] = toml_edit::value(false);
                entry["http_headers"]["authorization"] = toml_edit::value("Bearer old-fixture");
                entry["env_http_headers"]["AUTHORIZATION"] = toml_edit::value("FIXTURE_ENV");
                entry["env_http_headers"]["X-Fixture-Env"] = toml_edit::value("KEEP_ENV");
            }
            Snapshot::Toml(doc.to_string())
        }
        Snapshot::Json(mut value) => {
            value["timeout"] = json!(125000);
            value["disabledTools"] = json!(["fixture.tool"]);
            value["headers"]["X-Fixture"] = json!("keep");
            if stale_bridge {
                value["url"] = json!("http://127.0.0.1:9999/old");
                value["disabled"] = json!(true);
                value["command"] = json!("fixture-command");
                value["headers"]["authorization"] = json!("Bearer old-fixture");
            }
            Snapshot::Json(value)
        }
    };
    write_snapshot(target, &custom);
}

async fn connect(ctx: &AdapterContext, target: &AdapterTarget) {
    let planned = preview(ctx, target, AdapterOperation::Apply).await.unwrap();
    assert!(planned.can_apply);
    assert!(matches!(planned.operation, AdapterOperation::Apply));
    apply(
        ctx,
        target,
        &planned.status.target_fingerprint,
        endpoint(target.client),
        KEY,
        false,
    )
    .await
    .unwrap();
    assert!(
        bridge_matches(ctx, target, endpoint(target.client), KEY)
            .await
            .unwrap()
    );
}

fn assert_custom_settings(target: &AdapterTarget) {
    let raw = fs::read(target.override_path.as_ref().unwrap()).unwrap();
    if target.client == ClientKind::Codex {
        let doc: toml::Value = toml::from_str(std::str::from_utf8(&raw).unwrap()).unwrap();
        let entry = &doc["mcp_servers"][&target.server_name];
        assert_eq!(entry["tool_timeout_sec"].as_integer(), Some(125));
        assert_eq!(entry["enabled_tools"][0].as_str(), Some("fixture.tool"));
        assert_eq!(entry["http_headers"]["X-Fixture"].as_str(), Some("keep"));
        assert_eq!(doc["model"].as_str(), Some("fixture-model"));
        assert_eq!(
            doc["mcp_servers"]["other"]["command"].as_str(),
            Some("fixture-other")
        );
    } else {
        let doc: Value = serde_json::from_slice(&raw).unwrap();
        let entry = &doc["mcpServers"][&target.server_name];
        assert_eq!(entry["timeout"], 125000);
        assert_eq!(entry["disabledTools"], json!(["fixture.tool"]));
        assert_eq!(entry["headers"]["X-Fixture"], "keep");
        assert_eq!(doc["theme"], "fixture");
        assert_eq!(doc["mcpServers"]["other"]["command"], "fixture-other");
    }
}

#[tokio::test]
async fn user_tool_preferences_allow_read_only_detection_and_explicit_key_repair() {
    for client in [ClientKind::Codex, ClientKind::Workbuddy] {
        let (_directory, ctx, target) = fixture(client);
        connect(&ctx, &target).await;
        customize(&target, false);
        let before = fs::read(target.override_path.as_ref().unwrap()).unwrap();
        assert!(
            bridge_matches(&ctx, &target, endpoint(target.client), KEY)
                .await
                .unwrap()
        );
        assert!(
            !bridge_matches(
                &ctx,
                &target,
                endpoint(target.client),
                "unknown-fixture-key"
            )
            .await
            .unwrap()
        );
        let planned = preview(&ctx, &target, AdapterOperation::Apply)
            .await
            .unwrap();
        assert!(planned.can_apply && planned.status.modified && planned.status.repairable);
        assert!(matches!(planned.operation, AdapterOperation::Repair));
        // Not confirming the preview leaves the client's settings intact.
        assert_eq!(
            fs::read(target.override_path.as_ref().unwrap()).unwrap(),
            before
        );
        apply(
            &ctx,
            &target,
            &planned.status.target_fingerprint,
            endpoint(target.client),
            "replacement-fixture-key-never-used-for-http",
            true,
        )
        .await
        .unwrap();
        assert!(
            bridge_matches(
                &ctx,
                &target,
                endpoint(target.client),
                "replacement-fixture-key-never-used-for-http"
            )
            .await
            .unwrap()
        );
        assert_custom_settings(&target);
        let restore_preview = preview(&ctx, &target, AdapterOperation::Restore)
            .await
            .unwrap();
        assert!(restore_preview.can_apply);
        restore(
            &ctx,
            &target,
            &restore_preview.status.target_fingerprint,
            restore_preview.status.backup_ref.as_deref().unwrap(),
        )
        .await
        .unwrap();
        assert!(
            bridge_matches(&ctx, &target, endpoint(target.client), KEY)
                .await
                .unwrap()
        );
        assert_custom_settings(&target);
        assert!(!inspect(&ctx, &target).await.unwrap().modified);
    }
}

#[tokio::test]
async fn stale_bridge_repairs_keep_settings_and_restore_exact_before_image_across_rename() {
    for client in [ClientKind::Codex, ClientKind::Workbuddy] {
        let (_directory, ctx, mut target) = fixture(client);
        connect(&ctx, &target).await;
        customize(&target, true);
        let before = fingerprint(Some(&snapshot(&target)), true, client).unwrap();
        assert!(
            !bridge_matches(&ctx, &target, endpoint(target.client), KEY)
                .await
                .unwrap()
        );
        target.server_name = "Renamed196".into();
        let planned = preview(&ctx, &target, AdapterOperation::Apply)
            .await
            .unwrap();
        assert!(planned.can_apply && planned.status.repairable);
        assert!(matches!(planned.operation, AdapterOperation::Repair));
        apply(
            &ctx,
            &target,
            &planned.status.target_fingerprint,
            endpoint(target.client),
            KEY,
            true,
        )
        .await
        .unwrap();
        assert!(
            bridge_matches(&ctx, &target, endpoint(target.client), KEY)
                .await
                .unwrap()
        );
        assert_custom_settings(&target);
        let old = AdapterTarget {
            server_name: "MCP196".into(),
            ..target.clone()
        };
        let raw = fs::read(target.override_path.as_ref().unwrap()).unwrap();
        assert!(read_file(&old, &raw).unwrap().is_none());
        let restore_preview = preview(&ctx, &target, AdapterOperation::Restore)
            .await
            .unwrap();
        assert!(restore_preview.can_apply);
        restore(
            &ctx,
            &target,
            &restore_preview.status.target_fingerprint,
            restore_preview.status.backup_ref.as_deref().unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            fingerprint(Some(&snapshot(&old)), true, client).unwrap(),
            before
        );
        assert_custom_settings(&old);
        assert!(!inspect(&ctx, &old).await.unwrap().modified);
    }
}

#[tokio::test]
async fn formatting_is_ignored_for_new_records_and_legacy_records_can_upgrade_on_confirmation() {
    let (_directory, ctx, target) = fixture(ClientKind::Codex);
    connect(&ctx, &target).await;
    let path = target.override_path.as_ref().unwrap();
    let formatted = fs::read_to_string(path).unwrap().replace(
        "tool_timeout_sec = 60",
        "tool_timeout_sec=60 # client formatting",
    );
    fs::write(path, &formatted).unwrap();
    assert!(!inspect(&ctx, &target).await.unwrap().modified);
    assert!(
        bridge_matches(&ctx, &target, endpoint(target.client), KEY)
            .await
            .unwrap()
    );
    let root = metadata_root(&ctx).unwrap();
    let mut ledger = load_ledger(&root).unwrap();
    for record in ledger.entries.values_mut() {
        record.semantic_fingerprint = None;
    }
    save_ledger(&root, &ledger).unwrap();
    let planned = preview(&ctx, &target, AdapterOperation::Apply)
        .await
        .unwrap();
    assert!(planned.status.modified && planned.can_apply);
    apply(
        &ctx,
        &target,
        &planned.status.target_fingerprint,
        endpoint(target.client),
        KEY,
        true,
    )
    .await
    .unwrap();
    assert!(!inspect(&ctx, &target).await.unwrap().modified);
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .contains("# client formatting")
    );
}

#[tokio::test]
async fn concurrent_changes_invalidate_repair_confirmation_without_overwriting() {
    let (_directory, ctx, target) = fixture(ClientKind::Codex);
    connect(&ctx, &target).await;
    customize(&target, true);
    let planned = preview(&ctx, &target, AdapterOperation::Apply)
        .await
        .unwrap();
    let path = target.override_path.as_ref().unwrap();
    let changed = fs::read_to_string(path)
        .unwrap()
        .replace("tool_timeout_sec = 125", "tool_timeout_sec = 151");
    fs::write(path, &changed).unwrap();
    assert!(
        apply(
            &ctx,
            &target,
            &planned.status.target_fingerprint,
            endpoint(target.client),
            KEY,
            true
        )
        .await
        .is_err()
    );
    assert_eq!(fs::read_to_string(path).unwrap(), changed);
}

#[tokio::test]
async fn unmanaged_recreated_removed_pending_and_conflicting_entries_cannot_be_adopted_by_repair() {
    for client in [ClientKind::Codex, ClientKind::Workbuddy] {
        let (_directory, ctx, target) = fixture(client);
        let desired = match client {
            ClientKind::Codex => codex::desired(&target.server_name, endpoint(target.client), KEY),
            _ => workbuddy::desired(endpoint(target.client), KEY),
        };
        write_snapshot(&target, &desired);
        let blocked = preview(&ctx, &target, AdapterOperation::Apply)
            .await
            .unwrap();
        assert!(!blocked.can_apply && !blocked.status.repairable);
        assert!(
            !bridge_matches(&ctx, &target, endpoint(target.client), KEY)
                .await
                .unwrap()
        );
        // Establish a synthetic active owner for this explicitly isolated fixture.
        let location = location(&target).unwrap();
        let current = snapshot(&target);
        let root = metadata_root(&ctx).unwrap();
        let mut ledger = load_ledger(&root).unwrap();
        ledger.entries.insert(
            location.key.clone(),
            ManagedEntry {
                profile_id: target.profile_id.clone(),
                client,
                server_name: target.server_name.clone(),
                target_path: target.override_path.clone(),
                fingerprint: fingerprint(Some(&current), true, client).unwrap(),
                semantic_fingerprint: None,
                backup_ref: "fixture-missing-backup".into(),
                pending: false,
            },
        );
        save_ledger(&root, &ledger).unwrap();
        let conflicting = AdapterTarget {
            server_name: "other".into(),
            ..target.clone()
        };
        let blocked = preview(&ctx, &conflicting, AdapterOperation::Apply)
            .await
            .unwrap();
        assert!(!blocked.can_apply && blocked.status.name_conflict && !blocked.status.repairable);
        ledger.entries.get_mut(&location.key).unwrap().pending = true;
        save_ledger(&root, &ledger).unwrap();
        let blocked = preview(&ctx, &target, AdapterOperation::Apply)
            .await
            .unwrap();
        assert!(!blocked.can_apply && !blocked.status.repairable);
        assert!(
            !bridge_matches(&ctx, &target, endpoint(target.client), KEY)
                .await
                .unwrap()
        );
        let record = ledger.entries.get_mut(&location.key).unwrap();
        record.pending = false;
        record.fingerprint = "missing".into();
        save_ledger(&root, &ledger).unwrap();
        let blocked = preview(&ctx, &target, AdapterOperation::Apply)
            .await
            .unwrap();
        assert!(!blocked.can_apply && !blocked.status.repairable);
        assert!(
            !bridge_matches(&ctx, &target, endpoint(target.client), KEY)
                .await
                .unwrap()
        );
        assert!(
            apply(
                &ctx,
                &target,
                &blocked.status.target_fingerprint,
                endpoint(target.client),
                KEY,
                true
            )
            .await
            .is_err()
        );
        assert_eq!(
            fingerprint(Some(&snapshot(&target)), true, client).unwrap(),
            fingerprint(Some(&current), true, client).unwrap()
        );
    }
}
