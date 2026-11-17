//! Isolated management/loopback tests using temporary client files and synthetic credentials.
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
use tracing::instrument::WithSubscriber;

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
    let http = Arc::new(crate::nvwa::http::NvwaHttpClients::for_test());
    let service = NvwaService {
        inner: Arc::new(ServiceInner {
            store,
            secrets,
            auth: crate::nvwa::auth::AuthClient::new(http.clone()),
            http,
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

#[derive(Clone)]
struct FailingMcpPhase {
    method: &'static str,
    requests: Arc<Mutex<Vec<String>>>,
}

async fn failing_mcp_phase(
    State(fixture): State<FailingMcpPhase>,
    Json(request): Json<Value>,
) -> Response {
    let method = request["method"].as_str().unwrap();
    fixture.requests.lock().await.push(method.to_owned());
    if method == fixture.method {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":"fixture-private-upstream-body"})),
        )
            .into_response();
    }
    match method {
        "initialize" => Json(json!({"jsonrpc":"2.0","id":request["id"],"result":{
            "protocolVersion":bridge::PROTOCOLS[0],"capabilities":{"tools":{}},
            "serverInfo":{"name":"fixture-mcp","version":"1"}
        }}))
        .into_response(),
        "notifications/initialized" => StatusCode::ACCEPTED.into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[tokio::test]
async fn repeated_detection_502_keeps_client_setup_and_existing_sessions() {
    let captured = CapturedLog::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("codexhub=info"))
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    exercise_repeated_detection_502(captured.clone())
        .with_subscriber(subscriber)
        .await;
    let log = captured.text();
    for operation in ["prepare", "apply", "repair"] {
        assert!(log.lines().any(|line| {
            line.contains("event=\"mcp_client_operation\"")
                && line.contains(&format!("operation=\"{operation}\""))
                && line.contains("outcome=\"started\"")
        }));
    }
    assert!(log.lines().any(|line| {
        line.contains("operation=\"prepare\"")
            && line.contains("outcome=\"completed\"")
            && line.contains("ready=false")
            && line.contains("can_apply=true")
            && line.contains("preview_operation=\"apply\"")
            && [
                "configured=",
                "modified=",
                "owned=",
                "requires_client_access=",
            ]
            .iter()
            .all(|field| line.contains(field))
    }));
    assert!(log.lines().any(|line| {
        line.contains("operation=\"repair\"") && line.contains("outcome=\"failed\"")
    }));
    let started: Vec<_> = log
        .lines()
        .filter(|line| {
            line.contains("event=\"mcp_detection\"") && line.contains("outcome=\"started\"")
        })
        .collect();
    assert_eq!(started.len(), 8);
    let mut ids = std::collections::HashSet::new();
    for line in started {
        let id = line
            .split_once("detection_id=")
            .unwrap()
            .1
            .split_whitespace()
            .next()
            .unwrap();
        assert!(uuid::Uuid::parse_str(id).is_ok());
        assert!(ids.insert(id));
        assert!(log.lines().any(|line| {
            line.contains(id)
                && line.contains("event=\"mcp_detection_phase\"")
                && line.contains("outcome=\"failed\"")
                && line.contains("failure=\"upstream_http\"")
                && line.contains("upstream_status=502")
        }));
        assert!(
            log.lines().any(|line| {
                line.contains(id) && line.contains("event=\"mcp_detection_cleanup\"")
            })
        );
    }
    for phase in ["initialize", "initialized", "tools_list"] {
        assert!(log.lines().any(|line| {
            line.contains("event=\"mcp_detection_phase\"")
                && line.contains(&format!("phase=\"{phase}\""))
        }));
    }
}

#[derive(Clone, Default)]
struct CapturedLog(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for CapturedLog {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl CapturedLog {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

async fn exercise_repeated_detection_502(captured: CapturedLog) {
    for client in [ClientKind::Codex, ClientKind::Workbuddy] {
        for method in ["initialize", "tools/list"] {
            let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let bridge_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let (directory, mut service, mut profile) = fixture().await;
            profile.mcp_url = format!("http://{}/mcp", upstream_listener.local_addr().unwrap());
            let mut config = service.inner.store.load().unwrap();
            config.profiles[0] = profile.clone();
            service.inner.store.save(&config, &config.revision).unwrap();
            Arc::get_mut(&mut service.inner)
                .unwrap()
                .management
                .base_url = format!("http://{}", bridge_listener.local_addr().unwrap());

            let requests = Arc::new(Mutex::new(Vec::new()));
            let upstream = Router::new()
                .route("/mcp", post(failing_mcp_phase))
                .with_state(FailingMcpPhase {
                    method,
                    requests: requests.clone(),
                });
            let upstream_task = tokio::spawn(
                async move {
                    axum::serve(upstream_listener, upstream).await.unwrap();
                }
                .with_current_subscriber(),
            );
            let closed_sessions = Arc::new(Mutex::new(Vec::new()));
            let recorded_closes = closed_sessions.clone();
            let local = router(service.clone()).layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let closed = recorded_closes.clone();
                    async move {
                        if request.method() == axum::http::Method::DELETE {
                            let session = request.headers()["mcp-session-id"]
                                .to_str()
                                .unwrap()
                                .to_owned();
                            closed.lock().await.push(session);
                        }
                        next.run(request).await
                    }
                },
            ));
            let bridge_task = tokio::spawn(
                async move {
                    axum::serve(bridge_listener, local).await.unwrap();
                }
                .with_current_subscriber(),
            );

            let path = directory.path().join(if client == ClientKind::Codex {
                "config.toml"
            } else {
                "mcp.json"
            });
            let mut body =
                json!({"profileId":profile.id,"clientKind":client.as_str(),"overridePath":path});
            let preview = client_operation(&service, &profile, "prepare", &body)
                .await
                .unwrap();
            body["expectedFingerprint"] = preview["preview"]["status"]["targetFingerprint"].clone();
            client_operation(&service, &profile, "apply", &body)
                .await
                .unwrap();
            let baseline = client_operation(&service, &profile, "prepare", &body)
                .await
                .unwrap();
            assert_eq!(baseline["readyForDetection"], true);
            let config_before = std::fs::read(&path).unwrap();
            let ledger_path = service.inner.store.root().join("managed.json");
            let ledger_before = std::fs::read(&ledger_path).unwrap();
            let snapshot_key = crate::nvwa::session_key(&profile.id);
            let persisted_before = service.inner.secrets.get(&snapshot_key).unwrap().unwrap();
            let runtime_before = service.inner.runtime.protected_snapshot(&profile.id).await;
            let capability = service
                .inner
                .runtime
                .existing_capability(&profile.id, client)
                .await
                .unwrap();
            let (generation, _) = service.inner.runtime.auth(&profile.id).await.unwrap();
            let existing_session = service
                .inner
                .runtime
                .create_session(
                    &profile.id,
                    client,
                    &capability,
                    &generation,
                    bridge::PROTOCOLS[0],
                )
                .await
                .unwrap();
            let context = adapter_context(&service, client).await.unwrap();
            let target = client_target(&profile, &body).unwrap();
            let mut stale_repair = body.clone();
            stale_repair["expectedFingerprint"] = "fixture-stale-fingerprint".into();
            assert!(
                client_operation(&service, &profile, "repair", &stale_repair)
                    .await
                    .is_err()
            );
            for _ in 0..2 {
                let prepared = client_operation(&service, &profile, "prepare", &body)
                    .await
                    .unwrap();
                assert_eq!(prepared["readyForDetection"], true);
                let local = validated_local_bridge(&service, &profile, &context, &target)
                    .await
                    .unwrap();
                assert!(local.is_some());
                let error = bridge::detect(&service, &profile, local, false)
                    .await
                    .unwrap_err();
                assert!(error.to_string().contains("HTTP 502"), "{error}");
                assert!(!error.to_string().contains("fixture-private-upstream-body"));
                let after = client_operation(&service, &profile, "prepare", &body)
                    .await
                    .unwrap();
                assert_eq!(after["readyForDetection"], true);
                assert_eq!(
                    after["status"]["backupRef"],
                    baseline["status"]["backupRef"]
                );
                assert_eq!(std::fs::read(&path).unwrap(), config_before);
                assert_eq!(std::fs::read(&ledger_path).unwrap(), ledger_before);
                assert_eq!(
                    service.inner.secrets.get(&snapshot_key).unwrap().unwrap(),
                    persisted_before
                );
                assert_eq!(
                    service.inner.runtime.protected_snapshot(&profile.id).await,
                    runtime_before
                );
                assert_eq!(
                    service
                        .inner
                        .runtime
                        .existing_capability(&profile.id, client)
                        .await
                        .as_deref(),
                    Some(capability.as_str())
                );
                service
                    .inner
                    .runtime
                    .admit(
                        &profile.id,
                        client,
                        &capability,
                        Some(&existing_session),
                        "tools/list",
                        None,
                    )
                    .await
                    .unwrap();
            }
            let closed = closed_sessions.lock().await.clone();
            assert_eq!(closed.len(), if method == "initialize" { 0 } else { 2 });
            for session in closed {
                assert_ne!(session, existing_session);
                assert!(
                    service
                        .inner
                        .runtime
                        .admit(
                            &profile.id,
                            client,
                            &capability,
                            Some(&session),
                            "tools/list",
                            None
                        )
                        .await
                        .is_err()
                );
            }
            let expected = if method == "initialize" {
                vec!["initialize", "initialize"]
            } else {
                vec![
                    "initialize",
                    "notifications/initialized",
                    "tools/list",
                    "initialize",
                    "notifications/initialized",
                    "tools/list",
                ]
            };
            assert_eq!(*requests.lock().await, expected);
            let log = captured.text();
            for private in [
                profile.id.as_str(),
                profile.name.as_str(),
                profile.username.as_str(),
                profile.product_base_url.as_str(),
                profile.mcp_url.as_str(),
                service.inner.management.base_url.as_str(),
                service.inner.management.capability.as_str(),
                capability.as_str(),
                existing_session.as_str(),
                "fixture-upstream-token-never-used-for-http",
                "fixture-private-upstream-body",
                "fixture-stale-fingerprint",
                path.to_str().unwrap(),
            ] {
                assert!(
                    !log.contains(private),
                    "private fixture value leaked to diagnostics"
                );
            }
            bridge_task.abort();
            upstream_task.abort();
        }
    }
}
