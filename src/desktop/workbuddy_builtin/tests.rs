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
    }
}
async fn fixture_service(report: Report) -> Service {
    let lease = Arc::new(Lease {
        token: "fixture-lease".into(),
        bridge_id: "fixture".into(),
        snapshot: StdMutex::new(Some(Snapshot {
            report,
            at: Instant::now(),
        })),
        changed: Notify::new(),
    });
    let (stop, stopped) = oneshot::channel();
    Service {
        gate: Mutex::new(()),
        inner: Mutex::new(Inner {
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
    service.disable().await;
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
