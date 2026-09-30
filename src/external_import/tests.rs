use super::*;

fn link() -> ImportLink {
    ImportLink {
        origin: OFFICIAL_ORIGIN.into(),
        ticket: "a".repeat(43),
    }
}
fn raw_link(origin: &str) -> String {
    let mut url = Url::parse("tiancaispacehub://import/v1").unwrap();
    url.query_pairs_mut()
        .append_pair("origin", origin)
        .append_pair("ticket", &"a".repeat(43));
    url.into()
}
fn wire(protocol: &str) -> ResolveData {
    ResolveData {
        schema_version: 1,
        target: "tiancaispace-hub".into(),
        source: ImportSource {
            origin: OFFICIAL_ORIGIN.into(),
            key_id: "123".into(),
            site_name: "天才空间".into(),
            key_name: "开发密钥".into(),
        },
        provider: WireProvider {
            name: "天才空间 · 开发密钥".into(),
            protocol: protocol.into(),
            base_url: format!("{OFFICIAL_ORIGIN}/antigravity/v1/"),
            models_url: Some(format!("{OFFICIAL_ORIGIN}/v1/models")),
            api_key: "mock-secret-never-log".into(),
            models: vec!["model-a".into(), "model-b".into()],
            model_aliases: BTreeMap::new(),
        },
    }
}
fn request() -> CommitImport {
    let draft = wire("openai_responses").into_draft(&link()).unwrap();
    CommitImport {
        name: draft.provider.name.clone(),
        models: draft.provider.models.clone(),
        draft,
        update: None,
        enabled: false,
        visible_models: false,
        replace_aliases: false,
    }
}

#[test]
fn parses_canonical_origins_and_redacts_debug() {
    let parsed = ImportLink::parse(&raw_link("https://tiancai.yc99.space/")).unwrap();
    assert_eq!(parsed, link());
    assert!(!format!("{parsed:?}").contains(&parsed.ticket));
    assert!(!format!("{parsed:?}").contains(&parsed.origin));
}

#[test]
fn rejects_malformed_links_without_reflecting_ticket() {
    let valid = raw_link(OFFICIAL_ORIGIN);
    for bad in [
        format!("{valid}&ticket={}", "b".repeat(43)),
        format!("{valid}&unknown=x"),
        format!("{valid}#fragment"),
        valid.replace("/v1?", "/v2?"),
        valid.replace("import/v1", "user@import/v1"),
        valid.replace("import/v1", "import:80/v1"),
        valid.replace("tiancaispacehub:", "https:"),
        format!("{valid}%GG"),
        format!("{valid}\n"),
        format!("{valid}&origin=x"),
    ] {
        let error = ImportLink::parse(&bad).unwrap_err();
        assert!(!error.contains(&link().ticket));
    }
}

#[test]
fn accepts_compatible_http_and_https_sites_without_environment_switches() {
    for origin in [
        "http://localhost:9876",
        "http://127.0.0.1:9876",
        "http://[::1]:9876",
        "http://192.168.1.10:8080",
        "http://sub2api.local:8080",
        "http://example.com",
        "https://other.example",
        OFFICIAL_ORIGIN,
    ] {
        let parsed = ImportLink::parse(&raw_link(origin)).unwrap();
        assert_eq!(parsed.origin, origin);
        let mut data = wire("openai_responses");
        data.source.origin = origin.into();
        data.provider.base_url = format!("{origin}/antigravity/v1");
        data.provider.models_url = Some(format!("{origin}/v1/models"));
        let draft = data.into_draft(&parsed).unwrap();
        validate_draft(&draft).unwrap();
        assert_eq!(
            client::models_endpoint(&draft)
                .unwrap()
                .origin()
                .ascii_serialization(),
            origin
        );
        assert!(!draft.provider.enabled);
    }
}

#[test]
fn rejects_invalid_origins_and_endpoint_schemes() {
    for origin in [
        "http://u:p@localhost:9876",
        "https://u:p@example.com",
        "https://example.com/sub",
        "https://example.com/?x=1",
        "file:///tmp",
        "ftp://example.com",
        "https://example.com/#x",
    ] {
        assert!(ImportLink::parse(&raw_link(origin)).is_err());
    }
    for endpoint in [
        "http://user:pass@127.0.0.1:9876/v1",
        "http://localhost:9876/v1?secret=x",
        "http://192.168.1.10:8080/v1#fragment",
        "ftp://example.com/v1/models",
        "file:///tmp/models",
    ] {
        assert!(validate_endpoint(endpoint).is_err());
    }
}

#[test]
fn maps_four_protocols_and_preserves_path_prefix() {
    for (protocol, expected) in [
        ("openai_responses", ProviderType::OpenAiResponses),
        ("anthropic_messages", ProviderType::AnthropicMessages),
        ("chat_completions", ProviderType::ChatCompletions),
        ("grok_responses", ProviderType::GrokResponses),
    ] {
        let draft = wire(protocol).into_draft(&link()).unwrap();
        assert_eq!(draft.provider.provider_type, expected);
        assert_eq!(
            draft.provider.base_url,
            format!("{OFFICIAL_ORIGIN}/antigravity/v1")
        );
        assert!(!draft.provider.enabled);
        if protocol == "chat_completions" {
            assert_eq!(draft.provider.compatibility.as_deref(), Some("openai_chat"));
        }
    }
    assert!(wire("new_protocol").into_draft(&link()).is_err());
}

#[test]
fn rejects_mismatched_source_and_invalid_endpoint() {
    let mut data = wire("openai_responses");
    data.source.origin = "https://other.example".into();
    assert!(data.into_draft(&link()).is_err());
    let mut data = wire("openai_responses");
    data.provider.models_url = Some("https://user:secret@example.com/models".into());
    assert!(data.into_draft(&link()).is_err());
}

#[test]
fn new_import_is_disabled_and_preserves_unrelated_settings() {
    let mut config = AppConfig::default();
    config.language = Some("en-US".into());
    config.ai_gateway.providers.push(ProviderConfig {
        name: "workbuddy".into(),
        api_key: "keep".into(),
        ..Default::default()
    });
    config
        .ai_gateway
        .codex_visible_models
        .push("existing".into());
    merge_import(&mut config, &request()).unwrap();
    assert_eq!(config.ai_gateway.providers.len(), 2);
    assert_eq!(config.ai_gateway.providers[0].api_key, "keep");
    assert!(!config.ai_gateway.providers[1].enabled);
    assert_eq!(config.ai_gateway.codex_visible_models, ["existing"]);
    assert_eq!(config.language.as_deref(), Some("en-US"));
}

#[test]
fn duplicate_names_require_explicit_update_and_source_survives_rename() {
    let mut config = AppConfig::default();
    let mut req = request();
    merge_import(&mut config, &req).unwrap();
    assert!(merge_import(&mut config, &req).is_err());
    let existing = &mut config.ai_gateway.providers[0];
    existing.name = "我的渠道".into();
    existing.weight = 777;
    existing.timeout_secs = 42;
    existing
        .model_aliases
        .insert("alias".into(), "model-a".into());
    req.update = Some(UpdateTarget {
        name: existing.name.clone(),
        fingerprint: provider_fingerprint(existing),
    });
    req.name = existing.name.clone();
    req.enabled = true;
    merge_import(&mut config, &req).unwrap();
    let updated = &config.ai_gateway.providers[0];
    assert_eq!(updated.weight, 777);
    assert_eq!(updated.timeout_secs, 42);
    assert_eq!(updated.model_aliases.get("alias").unwrap(), "model-a");
    assert!(
        updated
            .import_source
            .as_ref()
            .unwrap()
            .same_key(&req.draft.source)
    );
}

#[test]
fn changes_to_target_during_preview_are_rejected() {
    let mut config = AppConfig::default();
    let mut req = request();
    merge_import(&mut config, &req).unwrap();
    req.update = Some(UpdateTarget {
        name: req.name.clone(),
        fingerprint: provider_fingerprint(&config.ai_gateway.providers[0]),
    });
    config.ai_gateway.providers[0].weight = 501;
    assert!(merge_import(&mut config, &req).is_err());
    assert_eq!(config.ai_gateway.providers[0].weight, 501);
}

#[test]
fn empty_models_cannot_be_enabled_or_exposed() {
    let mut req = request();
    req.models.clear();
    let mut config = AppConfig::default();
    req.enabled = true;
    assert!(merge_import(&mut config, &req).is_err());
    req.enabled = false;
    req.visible_models = true;
    assert!(merge_import(&mut config, &req).is_err());
    req.visible_models = false;
    merge_import(&mut config, &req).unwrap();
    assert!(config.ai_gateway.providers[0].models.is_empty());
}

#[test]
fn selected_visible_models_append_without_removing_existing() {
    let mut req = request();
    req.models = vec!["model-a".into()];
    req.visible_models = true;
    let mut config = AppConfig::default();
    config.ai_gateway.codex_visible_models = vec!["existing".into(), "model-a".into()];
    merge_import(&mut config, &req).unwrap();
    assert_eq!(
        config.ai_gateway.codex_visible_models,
        ["existing", "model-a"]
    );
}

#[test]
fn stale_whole_configuration_save_cannot_erase_import() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let mut initial = AppConfig::load_or_default(&path).unwrap();
    initial.save(&path).unwrap();
    let mut stale = AppConfig::load_or_default(&path).unwrap();
    let mut fresh = AppConfig::load_or_default(&path).unwrap();
    merge_import(&mut fresh, &request()).unwrap();
    fresh.save(&path).unwrap();
    stale.theme = Some("dark".into());
    assert!(stale.save(&path).is_err());
    assert_eq!(
        AppConfig::load_or_default(&path)
            .unwrap()
            .ai_gateway
            .providers
            .len(),
        1
    );
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("_revision")
    );
    fresh.theme = Some("dark".into());
    fresh.save(&path).unwrap();
}

#[test]
fn empty_import_cannot_be_enabled_later_through_whole_config_save() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let mut config = AppConfig::load_or_default(&path).unwrap();
    let mut req = request();
    req.models.clear();
    merge_import(&mut config, &req).unwrap();
    config.save(&path).unwrap();
    config.ai_gateway.providers[0].enabled = true;
    assert!(config.save(&path).is_err());
    assert!(
        !AppConfig::load_or_default(&path)
            .unwrap()
            .ai_gateway
            .providers[0]
            .enabled
    );
    config.ai_gateway.providers[0].models.push("model-a".into());
    config.save(&path).unwrap();
}

#[tokio::test]
async fn commit_api_merges_latest_settings_and_rejects_stale_updates() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let mut config = AppConfig::load_or_default(&path).unwrap();
    config.state_path = temp.path().join("state.json");
    config.save(&path).unwrap();
    let state = crate::app_state::AppState::new(path.clone(), config, None, None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = crate::web::router(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let stale: AppConfig = client
        .get(format!("{origin}/api/config"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut latest = AppConfig::load_or_default(&path).unwrap();
    latest.theme = Some("dark".into());
    latest.save(&path).unwrap();
    let mut req = request();
    req.draft.source.origin = "http://192.168.1.10:8080".into();
    req.draft.provider.import_source = Some(req.draft.source.clone());
    req.draft.provider.base_url = "http://192.168.1.10:8080/v1".into();
    req.draft.provider.models_url = Some("http://192.168.1.10:8080/v1/models".into());
    assert_eq!(
        client
            .post(format!("{origin}/api/external-import/commit"))
            .json(&req)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::OK
    );
    let saved = AppConfig::load_or_default(&path).unwrap();
    assert_eq!(saved.theme.as_deref(), Some("dark"));
    assert_eq!(saved.ai_gateway.providers.len(), 1);
    assert_eq!(
        client
            .post(format!("{origin}/api/config"))
            .json(&stale)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::CONFLICT
    );
    req.update = Some(UpdateTarget {
        name: req.name.clone(),
        fingerprint: provider_fingerprint(&saved.ai_gateway.providers[0]),
    });
    let mut changed = saved;
    changed.ai_gateway.providers[0].weight = 700;
    changed.save(&path).unwrap();
    assert_eq!(
        client
            .post(format!("{origin}/api/external-import/commit"))
            .json(&req)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::CONFLICT
    );
    let current = AppConfig::load_or_default(&path).unwrap();
    assert_eq!(current.ai_gateway.providers[0].weight, 700);
    server.abort();
}

#[tokio::test]
async fn upstream_errors_never_reflect_response_credentials() {
    use axum::{Router, routing::post};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/api/v1/external-import/resolve",
        post(|| async {
            (
                axum::http::StatusCode::NOT_FOUND,
                "secret-key-and-ticket-must-not-appear",
            )
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = client::build_client(
        &crate::config::OutboundProxyConfig {
            mode: crate::config::OutboundProxyMode::Direct,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let error = client::resolve(
        &client,
        &ImportLink {
            origin,
            ticket: "a".repeat(43),
        },
    )
    .await
    .err()
    .unwrap();
    assert!(error.contains("Ticket expired"));
    assert!(!error.contains("secret-key"));
    assert!(!error.contains(&"a".repeat(43)));
    server.abort();
}

#[tokio::test]
async fn resolves_ticket_once_discovers_models_and_does_not_follow_redirects() {
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let remote_origin = origin.clone();
    let count = calls.clone();
    let app = Router::new().route("/api/v1/external-import/resolve", post(move |Json(body): Json<serde_json::Value>| {
        let origin = remote_origin.clone(); let count = count.clone(); async move {
            assert_eq!(body["ticket"], "a".repeat(43)); count.fetch_add(1, Ordering::SeqCst);
            Json(serde_json::json!({"code":0,"data":{"schema_version":1,"target":"tiancaispace-hub","source":{"origin":origin,"site_name":"Mock","key_id":"1","key_name":"测试"},"provider":{"name":"Mock","protocol":"chat_completions","base_url":format!("{origin}/prefix/v1"),"models_url":format!("{origin}/v1/models"),"api_key":"mock-key","models":["model-a"],"model_aliases":{}}}}))
        }
    })).route("/v1/models", get(|headers: axum::http::HeaderMap| async move {
        assert_eq!(headers["authorization"], "Bearer mock-key");
        Json(serde_json::json!({"data":[{"id":"model-a"},{"id":"unrelated-protocol-model"}]}))
    })).route("/redirect", get(|| async { axum::response::Redirect::temporary("/v1/models") }));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = client::build_client(
        &crate::config::OutboundProxyConfig {
            mode: crate::config::OutboundProxyMode::Direct,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let mut draft = client::resolve(
        &client,
        &ImportLink {
            origin: origin.clone(),
            ticket: "a".repeat(43),
        },
    )
    .await
    .unwrap();
    client::fetch_models(&client, &mut draft).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(draft.provider.models, ["model-a"]);
    draft.provider.models_url = Some(format!("{origin}/redirect"));
    assert!(client::fetch_models(&client, &mut draft).await.is_err());
    server.abort();
}

#[cfg(windows)]
#[test]
fn registration_quotes_spaced_paths_and_url_argument() {
    assert_eq!(
        registration::command_for(std::path::Path::new(
            r"C:\Program Files\Hub\TianCaiSpace Hub.exe"
        ))
        .unwrap(),
        "\"C:\\Program Files\\Hub\\TianCaiSpace Hub.exe\" import-url \"%1\""
    );
}
