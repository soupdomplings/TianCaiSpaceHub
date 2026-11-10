use super::*;
use serde_json::json;
fn fixture() -> (
    tempfile::TempDir,
    Library,
    Resource,
    ClientTarget,
    ClientTarget,
) {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::new(&dir.path().join("hub.toml")).unwrap();
    let mk = |client, name| {
        let root = dir.path().join(name);
        targets::target(
            client,
            name,
            root.clone(),
            root.join("skills"),
            root.join("mcp.json"),
            McpFormat::McpServers,
            "fixture",
        )
    };
    let a = mk(ClientId::WorkBuddy, "one");
    let b = mk(ClientId::ZCode, "two");
    lib.save_targets(lib.load().unwrap(), vec![a.clone(), b.clone()])
        .unwrap();
    let resource = lib
        .save_mcp(
            None,
            McpServer {
                name: "shared-fixture".into(),
                config: json!({"type":"http","url":"https://fixture.invalid/mcp"}),
            },
            String::new(),
            &lib.load().unwrap().revision,
        )
        .unwrap();
    lib.use_files(&resource.id, &a.id, true, false).unwrap();
    lib.use_files(&resource.id, &b.id, true, false).unwrap();
    (dir, lib, resource, a, b)
}
#[test]
fn external_mcp_deletion_retires_only_that_clients_use_record() {
    let (_dir, lib, resource, a, b) = fixture();
    let other = std::fs::read(&b.mcp).unwrap();
    let raw = transaction::read(&a.mcp).unwrap().unwrap();
    let next = mcp::replace(&raw, a.mcp_format, &resource.name, None).unwrap();
    std::fs::write(&a.mcp, &next).unwrap();
    let view = lib.view().unwrap();
    let row = view
        .resources
        .iter()
        .find(|r| r.resource.id == resource.id)
        .unwrap();
    assert!(
        !row.targets
            .iter()
            .find(|t| t.target_id == a.id)
            .unwrap()
            .managed
    );
    assert!(!row.visible_for_client(a.client, false));
    assert!(row.visible_for_client(a.client, true));
    assert!(
        row.targets
            .iter()
            .find(|t| t.target_id == b.id)
            .unwrap()
            .managed
    );
    assert!(lib.remove(&resource.id, &view.revision).is_err());
    assert_eq!(std::fs::read(&a.mcp).unwrap(), next);
    assert_eq!(std::fs::read(&b.mcp).unwrap(), other);
    lib.use_files(&resource.id, &a.id, true, false).unwrap();
    assert!(lib.is_using(&resource, &a).unwrap());
}
#[test]
fn unreadable_or_edited_mcp_keeps_ownership_and_is_not_treated_as_deleted() {
    let (_dir, lib, resource, a, _b) = fixture();
    for raw in [
        b"{broken".as_slice(),
        br#"{"mcpServers":{"shared-fixture":{"url":"https://changed.invalid/mcp"}}}"#.as_slice(),
    ] {
        std::fs::write(&a.mcp, raw).unwrap();
        let view = lib.view().unwrap();
        let target = view
            .resources
            .iter()
            .find(|r| r.resource.id == resource.id)
            .unwrap()
            .targets
            .iter()
            .find(|t| t.target_id == a.id)
            .unwrap();
        assert!(target.managed && target.using.is_none());
        assert!(lib.use_files(&resource.id, &a.id, false, false).is_err());
        assert_eq!(std::fs::read(&a.mcp).unwrap(), raw);
    }
}
#[test]
fn confirmed_native_absence_clears_only_mcp_metadata_not_files_or_other_clients() {
    let (_dir, lib, resource, a, b) = fixture();
    let before_a = std::fs::read(&a.mcp).unwrap();
    let before_b = std::fs::read(&b.mcp).unwrap();
    lib.reconcile_native_mcp_names(
        &a.id,
        &std::collections::BTreeSet::from([resource.name.clone()]),
        &lib.load().unwrap().revision,
    )
    .unwrap();
    assert!(lib.load().unwrap().uses.iter().any(|u| u.target_id == a.id));
    let stale_revision = lib.load().unwrap().revision;
    lib.save_mcp(
        None,
        McpServer {
            name: "concurrent-entry".into(),
            config: json!({"type":"http","url":"https://fixture.invalid/other"}),
        },
        String::new(),
        &stale_revision,
    )
    .unwrap();
    assert!(
        lib.reconcile_native_mcp_names(&a.id, &std::collections::BTreeSet::new(), &stale_revision)
            .is_err()
    );
    assert!(lib.load().unwrap().uses.iter().any(|u| u.target_id == a.id));
    lib.reconcile_native_mcp_names(
        &a.id,
        &std::collections::BTreeSet::new(),
        &lib.load().unwrap().revision,
    )
    .unwrap();
    assert!(!lib.load().unwrap().uses.iter().any(|u| u.target_id == a.id));
    assert!(lib.load().unwrap().uses.iter().any(|u| u.target_id == b.id));
    assert_eq!(std::fs::read(&a.mcp).unwrap(), before_a);
    assert_eq!(std::fs::read(&b.mcp).unwrap(), before_b);
}
#[test]
fn builtin_assistance_cannot_be_copied_as_a_generic_credentialed_mcp() {
    let (_dir, lib, _resource, _a, _b) = fixture();
    let server = McpServer {
        name: "attempted-copy".into(),
        config: json!({"type":"http","url":"http://127.0.0.1:3210/api/local-assistance/mcp","headers":{"Authorization":"fixture"}}),
    };
    assert!(
        lib.save_mcp(None, server, String::new(), &lib.load().unwrap().revision)
            .unwrap_err()
            .to_string()
            .contains("接入设置")
    );
}
