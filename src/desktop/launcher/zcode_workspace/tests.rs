use super::*;
use crate::desktop::{
    models::{Binding, ManagedFile},
    targets::McpFormat,
    transaction,
};
use futures_util::SinkExt;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

fn fixture() -> (tempfile::TempDir, AppConfig, ClientTarget) {
    let directory = tempfile::tempdir().unwrap();
    let target = ClientTarget {
        id: "fixture-zcode".into(),
        client: ClientId::ZCode,
        label: "fixture".into(),
        root: directory.path().to_owned(),
        skills: directory.path().join("skills"),
        mcp: directory.path().join("cli/config.json"),
        mcp_format: McpFormat::ZCode,
        present: true,
        model_mode: "zcode".into(),
        model_root: None,
        models_inherited: false,
    };
    let file = target.root.join("v2/provider_config.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let raw = serde_json::to_vec(
        &json!({"schemaVersion":1,"config":{"providerConfigRules":{"providerRules":[{
            "providerId":"tiancai-hub-11111111-2222-3333-4444-555555555555","enabled":true,
            "config":{"personalModelIds":["fixture-model"]}
        }]}}}),
    )
    .unwrap();
    std::fs::write(&file, &raw).unwrap();
    let mut config = AppConfig::default();
    config.ai_gateway.enabled = true;
    config
        .ai_gateway
        .providers
        .push(crate::ai_gateway::config::ProviderConfig {
            name: "fixture-zcode-copy".into(),
            enabled: true,
            models: vec!["fixture-model".into()],
            desktop_binding: Some(Binding {
                id: "11111111-2222-3333-4444-555555555555".into(),
                client: ClientId::ZCode,
                target_id: target.id.clone(),
                mode: "zcode".into(),
                model: "fixture-model".into(),
                label: "fixture".into(),
                source_route: "fixture-source".into(),
                source_fingerprint: "fixture".into(),
                local_token: "fixture-local-token".into(),
                wire_protocol: "anthropic-messages".into(),
                capabilities: json!({}),
                native_files: vec![ManagedFile {
                    path: file,
                    fingerprint: transaction::fingerprint(Some(&raw)),
                    owned: None,
                }],
            }),
            ..Default::default()
        });
    (directory, config, target)
}

#[test]
fn only_enabled_native_zcode_copies_enable_workspace_adapter() {
    let (_directory, mut config, target) = fixture();
    let options = select_options(&config, std::slice::from_ref(&target))
        .unwrap()
        .unwrap();
    assert_eq!(
        options.models[0].provider_id,
        "tiancai-hub-11111111-2222-3333-4444-555555555555"
    );
    assert_eq!(options.models[0].model_id, "fixture-model");
    assert!(
        !serde_json::to_string(&options)
            .unwrap()
            .contains("fixture-local-token")
    );
    config.ai_gateway.providers[0].enabled = false;
    assert!(
        select_options(&config, std::slice::from_ref(&target))
            .unwrap()
            .is_none()
    );
    config.ai_gateway.providers[0].enabled = true;
    config.ai_gateway.enabled = false;
    assert!(
        select_options(&config, std::slice::from_ref(&target))
            .unwrap()
            .is_none()
    );
    config.ai_gateway.enabled = true;
    config.ai_gateway.providers[0]
        .desktop_binding
        .as_mut()
        .unwrap()
        .client = ClientId::DeepSeekHarness;
    assert!(
        select_options(&config, std::slice::from_ref(&target))
            .unwrap()
            .is_none()
    );
    config.ai_gateway.providers[0]
        .desktop_binding
        .as_mut()
        .unwrap()
        .client = ClientId::ZCode;
    let file = config.ai_gateway.providers[0]
        .desktop_binding
        .as_ref()
        .unwrap()
        .native_files[0]
        .path
        .clone();
    std::fs::write(&file, b"changed-by-client").unwrap();
    assert!(
        select_options(&config, std::slice::from_ref(&target))
            .unwrap_err()
            .to_string()
            .contains("客户端已独立修改")
    );
    std::fs::remove_file(file).unwrap();
    assert!(select_options(&config, &[target]).unwrap().is_none());
}

#[test]
fn startup_protects_existing_workspace_input_and_oauth() {
    let empty_login =
        json!({"workspaceOpen":false,"loginOpen":true,"hasInput":false,"oauthPending":false});
    assert!(safe_to_reload(&empty_login));
    for key in ["workspaceOpen", "hasInput", "oauthPending"] {
        let mut probe = empty_login.clone();
        probe[key] = true.into();
        assert!(!safe_to_reload(&probe));
    }
    let mut probe = empty_login;
    probe["loginOpen"] = false.into();
    assert!(!safe_to_reload(&probe));
    assert!(!safe_to_reload(&json!({})));
}

#[test]
fn renderer_targets_stay_in_installation_and_loopback() {
    let expected =
        std::env::temp_dir().join("ZCode-fixture/resources/app.asar/out/renderer/index.html");
    let page = Url::from_file_path(&expected).unwrap();
    assert!(same_page(
        &format!("{page}?locale=zh-CN&restoreSession=true"),
        &expected
    ));
    assert!(!same_page(
        &format!("{page}?windowKind=update-status"),
        &expected
    ));
    assert!(!same_page(&format!("{page}#other"), &expected));
    assert!(!same_page("https://example.invalid/index.html", &expected));
    let module = page.join("assets/styles-fixture.js").unwrap();
    assert!(same_renderer_module(&json!(page.as_str()), module.as_str()));
    for value in [
        "../main/styles-other.js",
        "assets/index-other.js",
        "assets/styles-fixture.js?x=1",
    ] {
        assert!(!same_renderer_module(
            &json!(page.as_str()),
            page.join(value).unwrap().as_str()
        ));
    }
    let target = |socket: &str| {
        serde_json::from_value::<Target>(json!({
            "id":"fixture","type":"page","url":page.as_str(),"webSocketDebuggerUrl":socket
        }))
        .unwrap()
    };
    assert!(
        select_target(
            vec![target("ws://127.0.0.1:18771/devtools/page/fixture")],
            &expected,
            18771
        )
        .is_ok()
    );
    for socket in [
        "ws://127.0.0.1:18772/devtools/page/fixture",
        "ws://example.invalid:18771/devtools/page/fixture",
        "ws://127.0.0.1:18771/devtools/page/other",
    ] {
        assert!(select_target(vec![target(socket)], &expected, 18771).is_err());
    }
}

async fn mock_cdp(replies: Vec<(&'static str, Value)>) -> (Cdp, JoinHandle<Vec<Value>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut requests = Vec::new();
        for (method, result) in replies {
            let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
                panic!("expected text");
            };
            let request: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(request["method"], method);
            socket
                .send(Message::Text(
                    json!({"id":request["id"],"result":result}).to_string(),
                ))
                .await
                .unwrap();
            requests.push(request);
        }
        requests
    });
    (
        Cdp::connect(&Url::parse(&format!("ws://{address}")).unwrap(), "ZCode")
            .await
            .unwrap(),
        task,
    )
}

#[tokio::test]
async fn script_adaptation_releases_incompatible_response_and_preserves_valid_headers() {
    let options = WorkspaceOptions {
        models: vec![ModelIdentity {
            provider_id: "tiancai-hub-11111111-2222-3333-4444-555555555555".into(),
            model_id: "fixture-model".into(),
        }],
    };
    for compatible in [false, true] {
        let (mut cdp, server) = mock_cdp(vec![
            (
                "Fetch.getResponseBody",
                json!({"body":"fixture original","base64Encoded":false}),
            ),
            (
                "Runtime.evaluate",
                json!({"result":{"value":{"adapted":compatible,"source":"fixture adapted"}}}),
            ),
            (
                if compatible {
                    "Fetch.fulfillRequest"
                } else {
                    "Fetch.continueRequest"
                },
                json!({}),
            ),
        ])
        .await;
        let result = adapt_response(&mut cdp, &json!({"requestId":"fixture","responseStatusCode":200,
            "responseHeaders":[{"name":"Content-Type","value":"text/javascript"},{"name":"Content-Length","value":"16"},{"name":"ETag","value":"old"}]}), &options).await;
        assert_eq!(result.is_ok(), compatible);
        let requests = server.await.unwrap();
        if compatible {
            assert_eq!(
                requests[2]["params"]["responseHeaders"],
                json!([{"name":"Content-Type","value":"text/javascript"}])
            );
            assert_eq!(
                base64::engine::general_purpose::STANDARD
                    .decode(requests[2]["params"]["body"].as_str().unwrap())
                    .unwrap(),
                b"fixture adapted"
            );
        }
    }
}
