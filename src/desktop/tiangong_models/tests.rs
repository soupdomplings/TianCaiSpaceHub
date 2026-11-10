use super::*;
use crate::{
    ai_gateway::{config::provider_route_id, context::GatewayContext, request_log},
    desktop::{
        clients::ClientId,
        models,
        targets::{self, McpFormat},
    },
};

pub(super) fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("electron-data.db");
    let connection = Connection::open(&db).unwrap();
    // No credential column: discovery must not query any application secret.
    connection
        .execute_batch(
            "CREATE TABLE model_configs (model_id TEXT, model_name TEXT, base_url TEXT);",
        )
        .unwrap();
    for (id, model) in [
        (IDS[0].0, "fixture-qwen-external"),
        (IDS[1].0, "fixture-qwen-internal"),
    ] {
        connection
            .execute(
                "INSERT INTO model_configs VALUES (?1,?2,?3)",
                rusqlite::params![id, model, ENDPOINT],
            )
            .unwrap();
    }
    connection.execute("INSERT INTO model_configs VALUES ('tiancaispacehub-other', 'must-not-import', 'https://example.invalid')", []).unwrap();
    drop(connection);
    let config = temp.path().join("hub.toml");
    AppConfig::default().save(&config).unwrap();
    (temp, db, config)
}
fn request(path: &Path, db: &Path) -> EnableRequest {
    let config = AppConfig::load_or_default(&path.to_path_buf()).unwrap();
    EnableRequest {
        revision: config.revision.unwrap(),
        catalog_revision: catalog_revision(&discover_at(db).unwrap()).unwrap(),
        model_ids: IDS.iter().map(|(id, _)| id.to_string()).collect(),
        pat: "fixture-personal-pat".into(),
        user_id: "fixture-user".into(),
    }
}

#[test]
fn discovery_is_read_only_and_never_reads_credentials_or_imports_hub_copies() {
    let (_temp, db, path) = fixture();
    let before = std::fs::read(&db).unwrap();
    let models = discover_at(&db).unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].name, "天工内置-外网");
    assert_eq!(std::fs::read(&db).unwrap(), before);
    let view = view_at(&AppConfig::load_or_default(&path).unwrap(), &db);
    assert!(view["catalogRevision"].is_string());
    assert!(
        view["models"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["configured"] == false)
    );
    assert!(!view.to_string().contains("credential"));
}

#[test]
fn missing_invalid_or_changed_native_catalog_is_actionable_and_does_not_create_database() {
    let (_temp, db, path) = fixture();
    let missing = db.with_extension("missing");
    assert!(discover_at(&missing).is_err());
    assert!(!missing.exists());
    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE model_configs SET base_url='https://other.invalid' WHERE model_id=?1",
            [IDS[0].0],
        )
        .unwrap();
    let view = view_at(&AppConfig::load_or_default(&path).unwrap(), &db);
    assert!(view["error"].as_str().unwrap().contains("地址已变化"));
    assert_eq!(view["models"], json!([]));
}

#[test]
fn stale_hub_revision_native_revision_invalid_selection_and_missing_pat_preserve_config() {
    let (_temp, db, path) = fixture();
    let original = std::fs::read(&path).unwrap();
    for kind in 0..6 {
        let mut req = request(&path, &db);
        match kind {
            0 => req.revision = "old".into(),
            1 => req.catalog_revision = "old".into(),
            2 => req.model_ids = vec![],
            3 => req.model_ids = vec!["not-builtin".into()],
            4 => req.model_ids = vec![IDS[0].0.into(), IDS[0].0.into()],
            _ => req.pat.clear(),
        }
        assert!(enable_at(&path, &db, req).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}

#[test]
fn enable_protects_authorization_and_is_idempotent_without_touching_native_models() {
    let (_temp, db, path) = fixture();
    let native_before = std::fs::read(&db).unwrap();
    enable_at(&path, &db, request(&path, &db)).unwrap();
    let config = AppConfig::load_or_default(&path).unwrap();
    assert_eq!(config.ai_gateway.providers.len(), 2);
    assert_eq!(std::fs::read(&db).unwrap(), native_before);
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("fixture-personal-pat") && !raw.contains("fixture-user"));
    for p in &config.ai_gateway.providers {
        assert!(p.api_key.is_empty());
        validate_provider(p).unwrap();
    }
    let route = provider_route_id(&config.ai_gateway.providers[0]);
    let key = config.ai_gateway.providers[0]
        .tiangong_builtin
        .as_ref()
        .unwrap()
        .credential_ref
        .clone();
    let mut again = request(&path, &db);
    again.pat.clear();
    again.user_id.clear();
    enable_at(&path, &db, again).unwrap();
    let second = AppConfig::load_or_default(&path).unwrap();
    assert_eq!(second.ai_gateway.providers.len(), 2);
    assert_eq!(provider_route_id(&second.ai_gateway.providers[0]), route);
    assert_eq!(
        second.ai_gateway.providers[0]
            .tiangong_builtin
            .as_ref()
            .unwrap()
            .credential_ref,
        key
    );
    let public = models::public(&second.ai_gateway).to_string();
    assert!(!public.contains("fixture-personal-pat") && !public.contains(&key));
}

#[test]
fn collision_and_invalid_pat_never_save_or_echo_credentials() {
    let (_temp, db, path) = fixture();
    let mut config = AppConfig::load_or_default(&path).unwrap();
    config.ai_gateway.providers.push(ProviderConfig {
        name: "天工内置 · 外网".into(),
        ..Default::default()
    });
    config.save(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let error = enable_at(&path, &db, request(&path, &db))
        .unwrap_err()
        .to_string();
    assert!(error.contains("同名渠道"));
    assert!(!error.contains("fixture-personal-pat"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    for bad in ["", "bad\npat", "bad pat", "中文"] {
        assert!(validate_credential(bad, "").is_err());
    }
    assert!(validate_credential("fixture-personal-pat", "bad\ruser").is_err());
}

#[test]
fn cross_client_copies_preserve_routing_and_independent_parameters_but_share_protected_auth() {
    let (_temp, db, path) = fixture();
    enable_at(&path, &db, request(&path, &db)).unwrap();
    let mut config = AppConfig::load_or_default(&path).unwrap();
    let source = config.ai_gateway.providers[0].clone();
    let mut copies = Vec::new();
    for client in [
        ClientId::VsCode,
        ClientId::DeepSeekHarness,
        ClientId::ZCode,
        ClientId::WebStorm,
    ] {
        let root = path.parent().unwrap().join(client.key());
        let target = targets::target(
            client,
            "fixture",
            root.clone(),
            root.join("skills"),
            root.join("mcp.json"),
            McpFormat::Servers,
            "gateway",
        );
        let mut copy = models::copy(&source, &target, IDS[0].1, "test").unwrap();
        copy.desktop_binding.as_mut().unwrap().capabilities = json!({"contextWindow":32768});
        assert_eq!(copy.tiangong_builtin, source.tiangong_builtin);
        assert_ne!(provider_route_id(&copy), provider_route_id(&source));
        assert!(copy.api_key.is_empty());
        copies.push(copy);
    }
    config.ai_gateway.providers.extend(copies);
    config.save(&path).unwrap();
    let mut update = request(&path, &db);
    update.pat = "fixture-new-pat".into();
    enable_at(&path, &db, update).unwrap();
    let updated = AppConfig::load_or_default(&path).unwrap();
    let key = &updated.ai_gateway.providers[0]
        .tiangong_builtin
        .as_ref()
        .unwrap()
        .credential_ref;
    for p in &updated.ai_gateway.providers {
        assert_eq!(&p.tiangong_builtin.as_ref().unwrap().credential_ref, key);
        if let Some(b) = &p.desktop_binding {
            assert_eq!(b.capabilities["contextWindow"], 32768);
        }
    }
}

#[test]
fn authorization_is_bound_to_exact_endpoint_and_builtin_identity() {
    let (_temp, db, path) = fixture();
    enable_at(&path, &db, request(&path, &db)).unwrap();
    let provider = AppConfig::load_or_default(&path)
        .unwrap()
        .ai_gateway
        .providers
        .remove(0);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut req = client
        .post(ENDPOINT)
        .header("Authorization", "Bearer untrusted")
        .build()
        .unwrap();
    authorize(&path, &mut req, &provider).unwrap();
    assert_eq!(
        req.headers()["authorization-cer-pat"],
        "fixture-personal-pat"
    );
    assert_eq!(req.headers()["x-tiangong-end-user-id"], "fixture-user");
    assert!(!req.headers().contains_key("authorization"));
    assert!(req.headers()["authorization-cer-pat"].is_sensitive());
    for endpoint in [
        "https://other.invalid/api/v2/tg/chat/completions",
        "https://tiangong.jiuqi.com.cn/api/v2/tg/chat/completions?extra=1",
        "http://127.0.0.1:9/chat/completions",
    ] {
        let mut req = client.post(endpoint).build().unwrap();
        assert!(authorize(&path, &mut req, &provider).is_err());
        assert!(!req.headers().contains_key("authorization-cer-pat"));
    }
    let mut bad = provider.clone();
    bad.provider_type = ProviderType::OpenAiResponses;
    assert!(validate_provider(&bad).is_err());
    bad = provider.clone();
    bad.model_aliases
        .insert("wrong".into(), "other-model".into());
    assert!(validate_provider(&bad).is_err());
    bad = provider.clone();
    bad.tiangong_builtin.as_mut().unwrap().credential_ref = "../secret".into();
    assert!(validate_provider(&bad).is_err());
    let mut req = client.post(ENDPOINT).build().unwrap();
    secrets(&path)
        .unwrap()
        .delete(&provider.tiangong_builtin.as_ref().unwrap().credential_ref)
        .unwrap();
    assert!(
        authorize(&path, &mut req, &provider)
            .unwrap_err()
            .to_string()
            .contains("重新登录")
    );
}

#[test]
fn protected_headers_cannot_be_supplied_by_clients_or_exposed_in_any_log_mode() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("authorization-cer-pat", "fixture-pat".parse().unwrap());
    headers.insert("x-tiangong-end-user-id", "fixture-user".parse().unwrap());
    headers.insert("x-tiangong-end-user-source", "spoofed".parse().unwrap());
    headers.insert("x-safe-client-header", "keep-me".parse().unwrap());
    let context = GatewayContext::extract(&headers, None);
    assert!(
        !context
            .upstream_headers
            .contains_key("authorization-cer-pat")
    );
    assert!(
        !context
            .upstream_headers
            .contains_key("x-tiangong-end-user-id")
    );
    assert!(
        !context
            .upstream_headers
            .contains_key("x-tiangong-end-user-source")
    );
    assert_eq!(context.upstream_headers["x-safe-client-header"], "keep-me");
    for log in [
        request_log::headers_to_json(&headers),
        request_log::headers_to_redacted_json(&headers),
    ] {
        let text = log.unwrap();
        assert!(!text.contains("fixture-pat") && !text.contains("fixture-user"));
    }
}
