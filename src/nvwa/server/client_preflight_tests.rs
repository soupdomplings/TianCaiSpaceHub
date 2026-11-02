//! Isolated management-command tests: no listener, NVWA HTTP or real app state.
use super::*;
use crate::nvwa::{
    ManagementInfo, ServiceInner,
    config::ConfigStore,
    runtime::Runtime,
    secrets::SecretStore,
    types::{AuthToken, TokenExpiryPolicy, VerifiedIdentity},
};
use std::sync::{Arc, Weak, atomic::AtomicBool};
use tokio::sync::{Mutex, Semaphore};

async fn fixture() -> (tempfile::TempDir, NvwaService, NvwaProfile) {
    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("hub.toml");
    let store = ConfigStore::new(&config_path).unwrap();
    let secrets = SecretStore::new(store.root().into()).unwrap();
    let profile = NvwaProfile {
        id: "fixture-profile".into(),
        name: "MCP196".into(),
        product_base_url: "https://fixture.invalid/nvwa".into(),
        username: "fixture-user".into(),
        ..NvwaProfile::default()
    };
    let mut config = store.load().unwrap();
    config.profiles.push(profile.clone());
    store.save(&config, &config.revision).unwrap();
    let runtime = Runtime::default();
    let generation = runtime.begin_auth(&profile.id).await;
    runtime
        .commit_auth(
            &profile.id,
            &generation,
            AuthResult {
                identity: VerifiedIdentity {
                    user_id: "fixture-user-id".into(),
                    identity_id: "fixture-identity-id".into(),
                    tenant_id: "fixture-tenant".into(),
                    username: profile.username.clone(),
                },
                personal_token: None,
                mcp_token: AuthToken {
                    value: "fixture-upstream-token-never-used-for-http".into(),
                    header_name: "Authorization".into(),
                    expires_at_ms: None,
                    expiry_policy: TokenExpiryPolicy::Fixed,
                },
            },
        )
        .await
        .unwrap();
    let service = NvwaService {
        inner: Arc::new(ServiceInner {
            store,
            secrets,
            auth: crate::nvwa::auth::AuthClient::new().unwrap(),
            http: reqwest::Client::new(),
            runtime,
            config_path,
            hub_state: Weak::new(),
            management: ManagementInfo {
                base_url: "http://127.0.0.1:3849".into(),
                capability: "fixture-management-never-used".into(),
                instance_id: "fixture-instance".into(),
            },
            running: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            gates: Mutex::new(HashMap::new()),
            browser: Mutex::new(HashMap::new()),
            login_attempts: Mutex::new(LoginAttempts::default()),
            identity_checks: Mutex::new(HashMap::from([(profile.id.clone(), now_ms())])),
            renewal_failures: Mutex::new(HashMap::new()),
            persistence: Mutex::new(()),
            inflight: Arc::new(Semaphore::new(1)),
        }),
    };
    (directory, service, profile)
}

#[tokio::test]
async fn prepare_only_reads_and_missing_grants_open_confirmed_repair_then_become_ready() {
    for client in [ClientKind::Codex, ClientKind::Workbuddy] {
        let (directory, service, profile) = fixture().await;
        let path = directory.path().join(if client == ClientKind::Codex {
            "config.toml"
        } else {
            "mcp.json"
        });
        let mut body =
            json!({"profileId":profile.id,"clientKind":client.as_str(),"overridePath":path});
        let prepared = client_operation(&service, &profile, "prepare", &body)
            .await
            .unwrap();
        assert_eq!(prepared["readyForDetection"], false);
        assert_eq!(prepared["preview"]["canApply"], true);
        assert_eq!(prepared["preview"]["operation"], "apply");
        assert!(!path.exists());
        assert!(
            service
                .inner
                .runtime
                .existing_capability(&profile.id, client)
                .await
                .is_none()
        );
        body["expectedFingerprint"] = prepared["preview"]["status"]["targetFingerprint"].clone();
        client_operation(&service, &profile, "apply", &body)
            .await
            .unwrap();
        let previous_key = service
            .inner
            .runtime
            .existing_capability(&profile.id, client)
            .await
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let prepared = client_operation(&service, &profile, "prepare", &body)
            .await
            .unwrap();
        assert_eq!(prepared["readyForDetection"], true);
        assert!(prepared["status"]["backupRef"].as_str().is_some());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // Simulate a missing runtime grant without trusting the old key in the client file.
        service
            .inner
            .runtime
            .revoke_client(&profile.id, client)
            .await;
        let prepared = client_operation(&service, &profile, "prepare", &body)
            .await
            .unwrap();
        assert_eq!(prepared["readyForDetection"], false);
        assert_eq!(prepared["preview"]["operation"], "repair");
        assert_eq!(prepared["preview"]["canApply"], true);
        assert!(
            service
                .inner
                .runtime
                .existing_capability(&profile.id, client)
                .await
                .is_none()
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            !serde_json::to_string(&prepared)
                .unwrap()
                .contains(&previous_key)
        );
        assert!(
            !serde_json::to_string(&prepared)
                .unwrap()
                .contains("fixture-upstream-token")
        );
        body["expectedFingerprint"] = prepared["preview"]["status"]["targetFingerprint"].clone();
        client_operation(&service, &profile, "repair", &body)
            .await
            .unwrap();
        assert!(
            service
                .inner
                .runtime
                .existing_capability(&profile.id, client)
                .await
                .is_some_and(|key| key != previous_key)
        );
        let prepared = client_operation(&service, &profile, "prepare", &body)
            .await
            .unwrap();
        assert_eq!(prepared["readyForDetection"], true);
        // A saved environment rename uses the same primary action to confirm migration.
        let mut updated = profile.clone();
        updated.name = "Renamed196".into();
        let mut config = service.inner.store.load().unwrap();
        config.profiles[0] = updated.clone();
        service.inner.store.save(&config, &config.revision).unwrap();
        let prepared = client_operation(&service, &updated, "prepare", &body)
            .await
            .unwrap();
        assert_eq!(prepared["readyForDetection"], false);
        assert_eq!(prepared["preview"]["operation"], "repair");
        assert_eq!(
            prepared["preview"]["status"]["previousServerName"],
            "MCP196"
        );
        assert_eq!(
            prepared["preview"]["status"]["desiredServerName"],
            "Renamed196"
        );
    }
}
