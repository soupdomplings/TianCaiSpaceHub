use super::*;
use crate::desktop::models;
use serde_json::json;
use std::fs;

fn fixture() -> tempfile::TempDir {
    // The resolver canonicalizes program paths. Use a canonical temp parent
    // so Windows' ADMINI~1 alias does not make equal locations compare unequal.
    let parent = fs::canonicalize(std::env::temp_dir()).unwrap();
    #[cfg(windows)]
    let parent = {
        let value = parent.to_string_lossy();
        if let Some(value) = value.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{value}"))
        } else {
            PathBuf::from(value.strip_prefix(r"\\?\").unwrap_or(&value))
        }
    };
    tempfile::tempdir_in(parent).unwrap()
}

fn write(path: &Path, value: Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}
fn windows_build(root: &Path, name: &str, product: Value, versioned: bool) -> PathBuf {
    let install = root.join(name);
    let app = if versioned {
        install.join("1.122.1/resources/app")
    } else {
        install.join("resources/app")
    };
    write(&app.join("product.json"), product);
    fs::create_dir_all(install.join("data")).unwrap();
    fs::create_dir_all(install.join("bin")).unwrap();
    let launcher = install.join("bin/code.cmd");
    fs::write(&launcher, "must never be executed").unwrap();
    launcher
}

#[test]
fn installed_default_and_explicit_portable_directory_follow_native_precedence() {
    let dir = fixture();
    let base = dir.path().join("Roaming");
    assert_eq!(
        vscode_user_dir(&base, Platform::Windows, None, &[], &[]),
        base.join("Code/User")
    );
    assert_eq!(
        vscode_user_dir(&base, Platform::Mac, None, &[], &[]),
        base.join("Code/User")
    );
    let portable = dir.path().join("explicit data");
    assert_eq!(
        vscode_user_dir(&base, Platform::Windows, Some(&portable), &[], &[]),
        portable.join("user-data/User")
    );
    assert_eq!(
        vscode_user_dir(
            &base,
            Platform::Windows,
            Some(Path::new("relative")),
            &[],
            &[]
        ),
        base.join("Code/User")
    );
    assert!(!portable.exists());
}

#[test]
fn portable_windows_build_is_found_from_cli_without_environment_override() {
    let dir = fixture();
    let bin = windows_build(
        dir.path(),
        "portable",
        json!({"applicationName":"code"}),
        false,
    );
    let before = fs::read(
        bin.parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("resources/app/product.json"),
    )
    .unwrap();
    let found = vscode_user_dir(
        &dir.path().join("Roaming"),
        Platform::Windows,
        None,
        &[bin],
        &[],
    );
    assert_eq!(found, dir.path().join("portable/data/user-data/User"));
    assert_eq!(
        fs::read(dir.path().join("portable/resources/app/product.json")).unwrap(),
        before
    );
    assert!(!found.exists());
}

#[test]
fn saved_executable_and_versioned_windows_build_resolve_the_same_portable_data() {
    let dir = fixture();
    windows_build(
        dir.path(),
        "portable",
        json!({"applicationName":"code"}),
        true,
    );
    let exe = dir.path().join("portable/Code.exe");
    fs::write(&exe, "must never run").unwrap();
    assert_eq!(
        vscode_user_dir(dir.path(), Platform::Windows, None, &[exe], &[]),
        dir.path().join("portable/data/user-data/User")
    );
}

#[test]
fn installed_build_with_data_folder_does_not_become_portable() {
    let dir = fixture();
    for target in [json!("user"), Value::Null] {
        let bin = windows_build(
            dir.path(),
            "installed",
            json!({"applicationName":"code","target":target}),
            false,
        );
        assert_eq!(
            vscode_user_dir(dir.path(), Platform::Windows, None, &[bin], &[]),
            dir.path().join("Code/User")
        );
    }
}

#[test]
fn fork_called_code_is_skipped_and_selected_installed_build_stops_search() {
    let dir = fixture();
    let fork = windows_build(
        dir.path(),
        "cursor",
        json!({"applicationName":"cursor"}),
        false,
    );
    let portable = windows_build(
        dir.path(),
        "portable",
        json!({"applicationName":"code"}),
        false,
    );
    let installed = windows_build(
        dir.path(),
        "installed",
        json!({"applicationName":"code","target":"user"}),
        false,
    );
    assert_eq!(
        vscode_user_dir(
            dir.path(),
            Platform::Windows,
            None,
            &[fork, portable.clone()],
            &[]
        ),
        dir.path().join("portable/data/user-data/User")
    );
    assert_eq!(
        vscode_user_dir(
            dir.path(),
            Platform::Windows,
            None,
            &[installed, portable],
            &[]
        ),
        dir.path().join("Code/User")
    );
}

#[test]
fn scoop_shim_is_read_as_metadata_and_never_executed() {
    let dir = fixture();
    let real = windows_build(
        dir.path(),
        "portable with spaces",
        json!({"applicationName":"code"}),
        false,
    );
    let shim = dir.path().join("shims/code.cmd");
    fs::create_dir_all(shim.parent().unwrap()).unwrap();
    fs::write(&shim, "must never be executed").unwrap();
    fs::write(
        shim.with_extension("shim"),
        format!("path = \"{}\"\n", real.display()),
    )
    .unwrap();
    assert_eq!(
        vscode_user_dir(dir.path(), Platform::Windows, None, &[shim], &[]),
        dir.path().join("portable with spaces/data/user-data/User")
    );
}

#[test]
fn mac_bundle_portable_name_and_ambiguous_installations_match_native_rules() {
    let dir = fixture();
    let app = dir.path().join("Applications/Visual Studio Code.app");
    write(
        &app.join("Contents/Resources/app/product.json"),
        json!({"applicationName":"code","portable":"my-code-data"}),
    );
    fs::create_dir_all(dir.path().join("Applications/my-code-data")).unwrap();
    let expected = dir.path().join("Applications/my-code-data/user-data/User");
    assert_eq!(
        vscode_user_dir(
            dir.path(),
            Platform::Mac,
            None,
            &[],
            std::slice::from_ref(&app)
        ),
        expected
    );
    let bin = app.join("Contents/Resources/app/bin/code");
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    fs::write(&bin, "must never run").unwrap();
    assert_eq!(
        vscode_user_dir(dir.path(), Platform::Mac, None, &[bin], &[]),
        expected
    );
    let other = dir.path().join("Other/Visual Studio Code.app");
    write(
        &other.join("Contents/Resources/app/product.json"),
        json!({"applicationName":"code"}),
    );
    fs::create_dir_all(dir.path().join("Other/code-portable-data")).unwrap();
    assert_eq!(
        vscode_user_dir(dir.path(), Platform::Mac, None, &[], &[app, other]),
        dir.path().join("Code/User")
    );
}

#[test]
fn only_registered_existing_profiles_are_targets_and_default_model_inheritance_is_kept() {
    // Port of Magpie TestVSCodeProfiles: registered work/shared/gone and an
    // orphan directory; each profile's default flags are authoritative.
    let dir = fixture();
    for name in ["work", "shared", "orphan"] {
        fs::create_dir_all(dir.path().join("profiles").join(name)).unwrap();
    }
    write(
        &dir.path().join("globalStorage/storage.json"),
        json!({"userDataProfiles":[
            {"location":"work","name":"工作","useDefaultFlags":{"settings":true}},
            {"location":"shared","useDefaultFlags":{"languageModels":true}},
            {"location":"gone"}
        ]}),
    );
    let skills = dir.path().join("skills");
    let targets = vscode_targets(dir.path(), &skills, Platform::Windows).unwrap();
    assert_eq!(targets.len(), 3);
    assert_eq!(targets[1].label, "Profile 工作");
    assert!(!targets[1].models_inherited);
    assert_eq!(targets[1].root, dir.path().join("profiles/work"));
    assert!(targets[2].models_inherited);
    assert_eq!(targets[2].models_root(), dir.path());
    assert_eq!(targets[2].mcp, dir.path().join("profiles/shared/mcp.json"));
    assert!(targets.iter().all(|t| t.skills == skills));
    assert!(
        !dir.path()
            .join("profiles/shared/chatLanguageModels.json")
            .exists()
    );
}

#[test]
fn uri_profile_locations_are_read_without_creating_stale_profile_directories() {
    let dir = fixture();
    let external = dir.path().join("external profile");
    fs::create_dir_all(&external).unwrap();
    write(
        &dir.path().join("globalStorage/storage.json"),
        json!({"userDataProfiles":[
            {"location":{"fsPath":external,"path":"ignored"}},
            {"location":{"path":format!("{}{}",if cfg!(windows){"/"}else{""},external.to_string_lossy().replace('\\',"/"))}}
        ]}),
    );
    let targets =
        vscode_targets(dir.path(), &dir.path().join("skills"), Platform::Windows).unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[1].root, external);
    assert!(!dir.path().join("profiles").exists());
}

#[test]
fn missing_profile_metadata_does_not_guess_from_directory_names() {
    let dir = fixture();
    fs::create_dir_all(dir.path().join("profiles/leftover")).unwrap();
    assert_eq!(
        vscode_targets(dir.path(), dir.path(), Platform::Windows)
            .unwrap()
            .len(),
        1
    );
    fs::create_dir_all(dir.path().join("globalStorage")).unwrap();
    fs::write(dir.path().join("globalStorage/storage.json"), "broken").unwrap();
    assert_eq!(
        vscode_targets(dir.path(), dir.path(), Platform::Windows)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fs::read(dir.path().join("globalStorage/storage.json")).unwrap(),
        b"broken"
    );
}

#[test]
fn inherited_profile_cannot_create_a_model_configuration_that_client_would_ignore() {
    let dir = fixture();
    let mut target = target(
        ClientId::VsCode,
        "shared",
        dir.path().join("profiles/shared"),
        dir.path().join("skills"),
        dir.path().join("profiles/shared/mcp.json"),
        McpFormat::Servers,
        "chat",
    );
    target.models_inherited = true;
    target.model_root = Some(dir.path().into());
    let source = crate::ai_gateway::config::ProviderConfig {
        name: "fixture".into(),
        models: vec!["fixture-model".into()],
        ..Default::default()
    };
    let binding = models::copy(&source, &target, "fixture-model", "")
        .unwrap()
        .desktop_binding
        .unwrap();
    assert!(
        models::plan(&target, &[binding], 3210, true)
            .err()
            .unwrap()
            .to_string()
            .contains("沿用默认")
    );
    assert!(!dir.path().join("chatLanguageModels.json").exists());
    assert!(!target.root.exists());
}

#[test]
fn zcode_moved_model_data_keeps_home_skill_mcp_and_target_identity() {
    // Port of Magpie TestZCodeMovedDataFolder and appdir.ZCodeDataIn.
    let dir = fixture();
    let user = dir.path().join("home");
    let moved = dir.path().join("ZCodeData");
    let bootstrap = user.join(".zcode/v2/setting.json");
    write(&bootstrap, json!({"locale":"zh-CN","dataBaseDir":moved}));
    let before = fs::read(&bootstrap).unwrap();
    let (root, models_root) = zcode_locations(&user, None, None, Platform::Windows).unwrap();
    assert_eq!(root, user.join(".zcode"));
    assert_eq!(models_root, moved.join(".zcode"));
    let original = target(
        ClientId::ZCode,
        "默认",
        root.clone(),
        root.join("skills"),
        root.join("cli/config.json"),
        McpFormat::ZCode,
        "zcode",
    );
    let mut current = original.clone();
    current.model_root = Some(models_root.clone());
    assert_eq!(original.id, current.id);
    let source = crate::ai_gateway::config::ProviderConfig {
        name: "fixture".into(),
        models: vec!["fixture-model".into()],
        ..Default::default()
    };
    let binding = models::copy(&source, &current, "fixture-model", "")
        .unwrap()
        .desktop_binding
        .unwrap();
    let changes = models::plan(&current, &[binding], 3210, true).unwrap();
    assert_eq!(changes.len(), 2);
    assert!(
        changes
            .iter()
            .all(|c| c.path.parent() == Some(models_root.join("v2").as_path()))
    );
    assert_eq!(current.skills, user.join(".zcode/skills"));
    assert_eq!(current.mcp, user.join(".zcode/cli/config.json"));
    assert_eq!(fs::read(&bootstrap).unwrap(), before);
    assert!(!moved.exists()); // planning and discovery never write
    assert!(!root.join("v2/provider_config.json").exists());
}

#[test]
fn zcode_home_override_and_explicit_hub_root_keep_their_precedence() {
    let dir = fixture();
    let user = dir.path().join("profile");
    let home = dir.path().join("shell home");
    let custom = dir.path().join("custom root");
    assert_eq!(
        zcode_locations(&user, Some(&home), None, Platform::Windows)
            .unwrap()
            .0,
        home.join(".zcode")
    );
    assert_eq!(
        zcode_locations(&user, Some(Path::new("relative")), None, Platform::Windows)
            .unwrap()
            .0,
        user.join(".zcode")
    );
    assert_eq!(
        zcode_locations(&user, Some(&home), Some(&custom), Platform::Windows)
            .unwrap()
            .0,
        custom
    );
    assert_eq!(
        zcode_locations(&user, Some(&home), None, Platform::Mac)
            .unwrap()
            .0,
        user.join(".zcode")
    );
}

#[test]
fn zcode_invalid_migration_setting_uses_the_native_default_without_rewriting_it() {
    let dir = fixture();
    let bootstrap = dir.path().join(".zcode/v2/setting.json");
    for value in [
        json!("relative"),
        json!(""),
        json!("   "),
        json!(42),
        Value::Null,
    ] {
        write(&bootstrap, json!({"dataBaseDir":value}));
        let before = fs::read(&bootstrap).unwrap();
        let (root, models) = zcode_locations(dir.path(), None, None, Platform::Windows).unwrap();
        assert_eq!(root, models);
        assert_eq!(fs::read(&bootstrap).unwrap(), before);
    }
}

#[test]
fn zcode_unreadable_bootstrap_stops_discovery_instead_of_choosing_an_unconfirmed_model_root() {
    let dir = fixture();
    let bootstrap = dir.path().join(".zcode/v2/setting.json");
    fs::create_dir_all(&bootstrap).unwrap();
    let error = zcode_locations(dir.path(), None, None, Platform::Windows).unwrap_err();
    assert!(error.to_string().contains("无法核对 ZCode 数据迁移设置"));
    assert!(bootstrap.is_dir());
    assert!(!bootstrap.with_file_name("config.json").exists());
    assert!(!bootstrap.with_file_name("provider_config.json").exists());
}

#[test]
fn zcode_unsafe_absolute_migration_never_falls_back_to_the_old_model_directory() {
    let dir = fixture();
    let bootstrap = dir.path().join(".zcode/v2/setting.json");
    let moved = dir.path().join("data/../actual");
    write(&bootstrap, json!({"dataBaseDir":moved}));
    let before = fs::read(&bootstrap).unwrap();
    let error = zcode_locations(dir.path(), None, None, Platform::Windows).unwrap_err();
    assert!(error.to_string().contains("请核对客户端实际数据目录"));
    assert_eq!(fs::read(&bootstrap).unwrap(), before);
    assert!(!bootstrap.with_file_name("config.json").exists());
    assert!(!dir.path().join("actual").exists());
}

#[test]
fn old_target_records_deserialize_without_new_location_fields() {
    let dir = fixture();
    let t = target(
        ClientId::ZCode,
        "默认",
        dir.path().into(),
        dir.path().join("skills"),
        dir.path().join("cli/config.json"),
        McpFormat::ZCode,
        "zcode",
    );
    let mut old = serde_json::to_value(&t).unwrap();
    old.as_object_mut().unwrap().remove("modelsInherited");
    let loaded: ClientTarget = serde_json::from_value(old).unwrap();
    assert_eq!(loaded.id, t.id);
    assert_eq!(loaded.models_root(), t.root);
    assert!(!loaded.models_inherited);
}

#[test]
fn harness_mcp_targets_follow_actual_profiles_instead_of_an_unused_root_patch() {
    let dir = fixture();
    for profile in ["desktop", "web", "custom"] {
        write(
            &dir.path()
                .join("profiles")
                .join(profile)
                .join("cordis.patch.yml"),
            json!([]),
        );
    }
    let targets = harness_targets(dir.path()).unwrap();
    assert_eq!(targets.len(), 3);
    assert_eq!(
        targets[0].mcp,
        dir.path().join("profiles/web/cordis.patch.yml")
    );
    assert!(!targets[0].models_inherited);
    assert!(
        targets[1..]
            .iter()
            .all(|t| t.models_inherited && t.skills == dir.path().join("skills"))
    );
    assert!(targets.iter().all(|t| t.mcp.is_file()));
    assert!(!dir.path().join("cordis.patch.yml").exists());
}

#[test]
fn harness_legacy_and_new_home_pick_real_native_configuration_locations() {
    let dir = fixture();
    assert_eq!(
        harness_targets(dir.path()).unwrap()[0].mcp,
        dir.path().join("profiles/desktop/cordis.patch.yml")
    );
    fs::write(dir.path().join("config.yaml"), "[]\n").unwrap();
    assert_eq!(
        harness_targets(dir.path()).unwrap()[0].mcp,
        dir.path().join("config.yaml")
    );
    assert!(!dir.path().join("profiles").exists());
}
