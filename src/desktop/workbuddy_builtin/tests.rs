use super::*;

fn fixture_report(account: &str) -> Report {
    Report {
        bridge_id: "fixture".into(),
        logged_in: true,
        account_id: Some(account.into()),
        account_name: Some("Fixture".into()),
        models: vec![BuiltinModel {
            id: "fixture-model".into(),
            name: "Fixture model".into(),
            context_window: Some(128000),
            supports_tools: Some(true),
            ..Default::default()
        }],
        reference: Some(LocalReference {
            url: "http://127.0.0.1:12345/skills/vlm/chat".into(),
            token: "fixture-local-reference".into(),
            context: "fixture-context".into(),
        }),
        state: "ready".into(),
        stage: false,
        ..Default::default()
    }
}
async fn fixture_service(report: Report) -> Service {
    fixture_service_with_direct(report, None).await
}
async fn fixture_service_with_direct(
    report: Report,
    direct: Option<Arc<direct::Runtime>>,
) -> Service {
    let lease = Arc::new(Lease {
        token: "fixture-lease".into(),
        bridge_id: "fixture".into(),
        snapshot: StdMutex::new(Some(Snapshot {
            report,
            at: Instant::now(),
        })),
        changed: Notify::new(),
        startup: StdMutex::new(String::new()),
        direct: direct.clone(),
        authorization_epoch: Mutex::new(0),
    });
    let (stop, stopped) = oneshot::channel();
    Service {
        gate: Mutex::new(()),
        inner: Mutex::new(Inner {
            direct,
            session: Some(Session {
                lease,
                stop: Some(stop),
                task: tokio::spawn(async {
                    let _ = stopped.await;
                }),
            }),
            ..Default::default()
        }),
    }
}

fn authorization_report(uid: Value, epoch: u64) -> Report {
    use sha2::{Digest, Sha256};
    let account = hex::encode(Sha256::digest(json!([uid, "", ""]).to_string().as_bytes()));
    let mut report = fixture_report(&account);
    report.auth_only = true;
    report.authorization_epoch = Some(epoch);
    report.authorization = Some(serde_json::from_value(json!({
        "account":{"uid":uid},
        "auth":{"accessToken":"fixture-access-never-public","refreshToken":"fixture-refresh-never-public"},
        "gatewayEndpoint":"https://www.workbuddy.cn",
        "authEndpoint":"https://www.workbuddy.cn",
        "authPrefixPath":"/plugin","authType":"account-scoped-external-link","appVersion":"5.9.3"
    })).unwrap());
    report.models[0].billing =
        json!({"unit":"credits","display":"x0.51 credits","multiplier":0.51});
    report
}

fn callback_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer fixture-lease".parse().unwrap());
    headers
}

#[tokio::test]
async fn share_entry_persists_and_hiding_keeps_the_active_authorization_and_models() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("hub.toml");
    let runtime = Arc::new(direct::Runtime::new(&path).unwrap());
    let grant = authorization_report(json!(12345), 1);
    let account = grant.account_id.clone().unwrap();
    let service =
        fixture_service_with_direct(fixture_report(&account), Some(runtime.clone())).await;
    assert!(!service.feature_visible(&path).unwrap());
    let lease = service
        .inner
        .lock()
        .await
        .session
        .as_ref()
        .unwrap()
        .lease
        .clone();
    assert_eq!(
        receive(State(lease), callback_headers(), Json(grant)).await,
        StatusCode::NO_CONTENT
    );
    service.reveal(&path).await.unwrap();
    let restarted = Service::default();
    assert!(restarted.feature_visible(&path).unwrap());
    let dormant = restarted.view(&path, &AppConfig::default()).await.unwrap();
    assert_eq!(dormant["enabled"], false);
    assert_eq!(dormant["account"]["authorization"]["status"], "missing");
    let source = service
        .source(&AppConfig::default(), "fixture-model")
        .await
        .unwrap();
    let mut config = AppConfig::default();
    config.ai_gateway.providers.push(source.clone());
    let before = serde_json::to_string(&config).unwrap();
    service.hide(&path).await.unwrap();
    assert!(!Service::default().feature_visible(&path).unwrap());
    let hidden = service.view(&path, &config).await.unwrap();
    assert_eq!(hidden["featureVisible"], false);
    assert_eq!(hidden["enabled"], true);
    assert_eq!(hidden["ready"], true);
    assert_eq!(hidden["account"]["authorization"]["status"], "available");
    assert!(
        service
            .direct_for(source.workbuddy_builtin.as_ref().unwrap())
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        direct::Runtime::new(&path)
            .unwrap()
            .load()
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(serde_json::to_string(&config).unwrap(), before);
    service.reveal(&path).await.unwrap();
    assert!(service.feature_visible(&path).unwrap());
}

#[tokio::test]
async fn independent_authorization_survives_host_exit_and_hub_restart_until_explicit_stop() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("hub.toml");
    let runtime = Arc::new(direct::Runtime::new(&path).unwrap());
    let grant = authorization_report(json!(12345), 1);
    let account = grant.account_id.clone().unwrap();
    let service =
        fixture_service_with_direct(fixture_report(&account), Some(runtime.clone())).await;
    let lease = service
        .inner
        .lock()
        .await
        .session
        .as_ref()
        .unwrap()
        .lease
        .clone();
    assert_eq!(
        receive(State(lease.clone()), callback_headers(), Json(grant)).await,
        StatusCode::NO_CONTENT
    );
    lease.snapshot.lock().unwrap().as_mut().unwrap().at =
        Instant::now() - LEASE_TTL - Duration::from_secs(1);
    let view = service.view(&path, &AppConfig::default()).await.unwrap();
    assert_eq!(view["ready"], true);
    assert_eq!(view["independent"], true);
    assert_eq!(view["hostConnected"], false);
    assert_eq!(view["models"][0]["billing"]["multiplier"], 0.51);
    let source = service
        .source(&AppConfig::default(), "fixture-model")
        .await
        .unwrap();
    let binding = source.workbuddy_builtin.as_ref().unwrap();
    assert!(service.direct_for(binding).await.unwrap().is_some());
    let public = format!("{}{}", view, serde_json::to_string(&source).unwrap());
    assert!(!public.contains("fixture-access-never-public"));
    assert!(!public.contains("fixture-refresh-never-public"));
    let restarted = direct::Runtime::new(&path).unwrap();
    assert_eq!(
        restarted.load().await.unwrap().unwrap()["accountId"],
        account
    );
    service.disable().await.unwrap();
    assert!(
        direct::Runtime::new(&path)
            .unwrap()
            .load()
            .await
            .unwrap()
            .is_none()
    );
    // A delayed callback from the stopped host cannot re-create the vault.
    assert_eq!(
        receive(
            State(lease),
            callback_headers(),
            Json(authorization_report(json!(12345), 2))
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert!(runtime.snapshot().await.is_none());
    assert!(
        direct::Runtime::new(&path)
            .unwrap()
            .load()
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn authorization_epochs_reject_late_grants_and_heartbeats_after_revocation() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(direct::Runtime::new(&root.path().join("hub.toml")).unwrap());
    let grant = authorization_report(json!("first"), 1);
    let first = grant.account_id.clone().unwrap();
    let service = fixture_service_with_direct(fixture_report(&first), Some(runtime.clone())).await;
    let lease = service
        .inner
        .lock()
        .await
        .session
        .as_ref()
        .unwrap()
        .lease
        .clone();
    assert_eq!(
        receive(State(lease.clone()), callback_headers(), Json(grant)).await,
        StatusCode::NO_CONTENT
    );
    let revoke = Report {
        bridge_id: "fixture".into(),
        auth_only: true,
        authorization_epoch: Some(2),
        ..Default::default()
    };
    assert_eq!(
        receive(State(lease.clone()), callback_headers(), Json(revoke)).await,
        StatusCode::NO_CONTENT
    );
    assert!(runtime.snapshot().await.is_none());
    assert!(lease.live().is_none());
    assert_eq!(
        receive(
            State(lease.clone()),
            callback_headers(),
            Json(authorization_report(json!("first"), 1))
        )
        .await,
        StatusCode::NO_CONTENT
    );
    let mut late = fixture_report(&first);
    late.authorization_epoch = Some(1);
    assert_eq!(
        receive(State(lease.clone()), callback_headers(), Json(late)).await,
        StatusCode::NO_CONTENT
    );
    assert!(runtime.snapshot().await.is_none());
    assert!(lease.live().is_none());
    let grant = authorization_report(json!("second"), 3);
    let second = grant.account_id.clone().unwrap();
    assert_eq!(
        receive(State(lease), callback_headers(), Json(grant)).await,
        StatusCode::NO_CONTENT
    );
    let old = BuiltinBinding {
        account_id: first,
        model_id: "fixture-model".into(),
        capabilities: Value::Null,
    };
    assert!(service.direct_for(&old).await.is_err());
    assert_eq!(runtime.snapshot().await.unwrap()["accountId"], second);
    service.disable().await.unwrap();
}

#[tokio::test]
async fn missing_independent_authorization_is_visible_and_cannot_be_shared_as_ready() {
    let root = tempfile::tempdir().unwrap();
    let mut report = fixture_report(&"a".repeat(64));
    report.state = "authorization-unavailable".into();
    let service = fixture_service(report).await;
    let view = service
        .view(&root.path().join("hub.toml"), &AppConfig::default())
        .await
        .unwrap();
    assert_eq!(view["ready"], false);
    assert_eq!(view["independent"], false);
    assert_eq!(view["reauthorizationRequired"], true);
    assert!(
        service
            .source(&AppConfig::default(), "fixture-model")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn workbuddy_binding_is_public_only_and_current_account_directory_controls_execution() {
    let account = "a".repeat(64);
    let service = fixture_service(fixture_report(&account)).await;
    let source = service
        .source(&AppConfig::default(), "fixture-model")
        .await
        .unwrap();
    validate_provider(&source).unwrap();
    let serialized = serde_json::to_string(&source).unwrap();
    assert!(!serialized.contains("fixture-local-reference"));
    assert!(!serialized.contains("fixture-context"));
    assert!(
        service
            .source(&AppConfig::default(), "not-in-directory")
            .await
            .is_err()
    );
    let binding = source.workbuddy_builtin.as_ref().unwrap();
    assert!(service.reference(binding).await.is_ok());
    let fingerprint = super::super::models::fingerprint(&source).unwrap();
    {
        let inner = service.inner.lock().await;
        let mut snapshot = inner
            .session
            .as_ref()
            .unwrap()
            .lease
            .snapshot
            .lock()
            .unwrap();
        snapshot
            .as_mut()
            .unwrap()
            .report
            .reference
            .as_mut()
            .unwrap()
            .token = "rotated-local-token".into();
    }
    assert_eq!(
        fingerprint,
        super::super::models::fingerprint(&source).unwrap()
    );
    {
        let inner = service.inner.lock().await;
        let mut snapshot = inner
            .session
            .as_ref()
            .unwrap()
            .lease
            .snapshot
            .lock()
            .unwrap();
        snapshot.as_mut().unwrap().report = fixture_report(&"b".repeat(64));
    }
    assert!(
        service
            .reference(binding)
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("账户已切换")
    );
    service.disable().await.unwrap();
    assert!(service.reference(binding).await.is_err());
}

#[tokio::test]
async fn workbuddy_expired_lease_and_generic_config_forgery_are_rejected() {
    let service = fixture_service(fixture_report(&"a".repeat(64))).await;
    let source = service
        .source(&AppConfig::default(), "fixture-model")
        .await
        .unwrap();
    let mut previous = AppConfig::default();
    previous.ai_gateway.providers.push(source.clone());
    let mut changed = previous.clone();
    changed.ai_gateway.providers[0].timeout_secs = 500;
    validate_edit(&previous, &changed).unwrap();
    changed.ai_gateway.providers[0].workbuddy_builtin = None;
    assert!(validate_edit(&previous, &changed).is_err());
    assert!(validate_edit(&AppConfig::default(), &previous).is_err());
    changed = previous.clone();
    changed.ai_gateway.providers[0]
        .model_aliases
        .insert("other".into(), "not-in-directory".into());
    assert!(validate_edit(&previous, &changed).is_err());
    {
        let inner = service.inner.lock().await;
        inner
            .session
            .as_ref()
            .unwrap()
            .lease
            .snapshot
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .at = Instant::now() - LEASE_TTL - Duration::from_secs(1);
    }
    assert!(service.source(&previous, "fixture-model").await.is_err());
    assert!(
        service
            .reference(source.workbuddy_builtin.as_ref().unwrap())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn workbuddy_startup_reports_do_not_overwrite_login_or_account_snapshots() {
    let lease = Arc::new(Lease {
        token: "fixture-lease".into(),
        bridge_id: "fixture".into(),
        snapshot: StdMutex::new(None),
        changed: Notify::new(),
        startup: StdMutex::new(String::new()),
        direct: None,
        authorization_epoch: Mutex::new(0),
    });
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer fixture-lease".parse().unwrap());
    for state in ["login-required", "catalog-pending", "ready"] {
        let mut report = fixture_report(&"a".repeat(64));
        report.state = state.into();
        if state != "ready" {
            report.models.clear();
            report.reference = None;
        }
        if state == "login-required" {
            report.logged_in = false;
            report.account_id = None;
        }
        assert_eq!(
            receive(State(lease.clone()), headers.clone(), Json(report)).await,
            StatusCode::NO_CONTENT
        );
        // Native module compilation and HTTP notifications can be reordered.
        for stage in [
            "server-module-ready",
            "main-loader-ready",
            "daemon-preload-ready",
        ] {
            let report = Report {
                bridge_id: "fixture".into(),
                stage: true,
                state: stage.into(),
                ..Default::default()
            };
            assert_eq!(
                receive(State(lease.clone()), headers.clone(), Json(report)).await,
                StatusCode::NO_CONTENT
            );
        }
        assert_eq!(lease.startup_state(), "server-module-ready");
        assert!(!lease.startup_failed());
        // Both missing-login and pending-catalog reports prove a completed
        // bridge handshake, while only ready exposes an executable reference.
        let live = lease.live().unwrap();
        assert_eq!(live.state, state);
        assert_eq!(live.reference.is_some(), state == "ready");
    }
    let failure = Report {
        bridge_id: "fixture".into(),
        stage: true,
        state: "host-attach-failed".into(),
        ..Default::default()
    };
    assert_eq!(
        receive(State(lease.clone()), headers, Json(failure)).await,
        StatusCode::NO_CONTENT
    );
    assert!(lease.startup_failed());
    assert_eq!(lease.live().unwrap().state, "ready");
}
