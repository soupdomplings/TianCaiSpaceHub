use super::*;
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::State as AxumState,
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::{Barrier, Notify};

#[derive(Default)]
struct Fixture {
    generations: AtomicUsize,
    refreshes: AtomicUsize,
    identities: AtomicUsize,
    wrong_account: AtomicBool,
    reject_refresh: AtomicBool,
    reject_identity: AtomicBool,
    parallel: AtomicBool,
    old_requests: Option<Barrier>,
    bodies: Mutex<Vec<Value>>,
    disconnected: Arc<Notify>,
}

struct Disconnect(Arc<Notify>);
impl Drop for Disconnect {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

async fn chat(
    AxumState(f): AxumState<Arc<Fixture>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> AxumResponse {
    f.generations.fetch_add(1, Ordering::Relaxed);
    f.bodies.lock().await.push(body.clone());
    assert!(!headers.contains_key("origin"));
    assert!(!headers.contains_key("x-upstream-secret"));
    assert!(!headers.contains_key("x-workbuddy-mcp-context"));
    assert_eq!(headers["x-ide-version"], "fixture-5.9.3");
    assert_eq!(headers["x-user-id"], "fixture-user");
    assert_eq!(headers["x-enterprise-id"], "fixture-org");
    assert_eq!(headers["x-tenant-id"], "fixture-org");
    assert_eq!(headers["x-domain"], "www.workbuddy.cn");
    assert!(headers.contains_key("traceparent"));
    if body["need_refresh"] == true && headers["authorization"] == "Bearer fixture-access-initial" {
        if f.parallel.load(Ordering::Relaxed) {
            f.old_requests.as_ref().unwrap().wait().await;
        }
        return (StatusCode::UNAUTHORIZED, "fixture-upstream-private-error").into_response();
    }
    if let Some(status) = body["failure"].as_u64() {
        return (
            StatusCode::from_u16(status as u16).unwrap(),
            "fixture-upstream-private-error",
        )
            .into_response();
    }
    if body["hold"] == true {
        let stream = futures_util::stream::unfold(
            (false, Disconnect(f.disconnected.clone())),
            |(sent, guard)| async move {
                if sent {
                    futures_util::future::pending::<()>().await;
                }
                Some((
                    Ok::<_, std::io::Error>(Bytes::from_static(
                        b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
                    )),
                    (true, guard),
                ))
            },
        );
        return (
            [("content-type", "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response();
    }
    if body["stream"] == true {
        return ([("content-type", "text/event-stream")], "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"fixture-call\",\"type\":\"function\",\"function\":{\"name\":\"fixture_tool\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n").into_response();
    }
    Json(json!({"id":"fixture-response","choices":[{"message":{"role":"assistant","content":"done"},"finish_reason":"stop"}]})).into_response()
}

async fn refresh(
    AxumState(f): AxumState<Arc<Fixture>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> AxumResponse {
    f.refreshes.fetch_add(1, Ordering::Relaxed);
    assert_eq!(headers["x-refresh-token"], "fixture-refresh-initial");
    assert_eq!(headers["x-auth-refresh-source"], "plugin");
    assert_eq!(headers["authorization"], "Bearer fixture-access-initial");
    assert_eq!(body, json!({}));
    if f.reject_refresh.load(Ordering::Relaxed) {
        return (StatusCode::UNAUTHORIZED, "fixture-refresh-secret-echo").into_response();
    }
    Json(json!({"code":0,"data":{"accessToken":"fixture-access-new","refreshToken":"fixture-refresh-new","domain":"www.workbuddy.cn","expiresIn":3600,"refreshExpiresIn":86400}})).into_response()
}

async fn account(AxumState(f): AxumState<Arc<Fixture>>, headers: HeaderMap) -> AxumResponse {
    f.identities.fetch_add(1, Ordering::Relaxed);
    assert_eq!(headers["authorization"], "Bearer fixture-access-new");
    if f.reject_identity.load(Ordering::Relaxed) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "fixture-private-account-error",
        )
            .into_response();
    }
    Json(json!({"code":0,"data":{"uid":if f.wrong_account.load(Ordering::Relaxed) { "different-user" } else { "fixture-user" },"enterpriseId":"fixture-org","type":"enterprise"}})).into_response()
}

async fn server() -> (Arc<Fixture>, String, tokio::task::JoinHandle<()>) {
    let fixture = Arc::new(Fixture {
        old_requests: Some(Barrier::new(2)),
        ..Fixture::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/v2/chat/completions", post(chat))
        .route("/v2/plugin/auth/token/refresh", post(refresh))
        .route("/v2/plugin/account", get(account))
        .with_state(fixture.clone());
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (fixture, endpoint, task)
}

fn authorization(endpoint: &str) -> Authorization {
    Authorization {
        account: Account {
            uid: json!("fixture-user"),
            enterprise_id: Some(json!("fixture-org")),
            kind: Some("enterprise".into()),
        },
        auth: Auth {
            access_token: "fixture-access-initial".into(),
            refresh_token: Some("fixture-refresh-initial".into()),
            domain: Some("www.workbuddy.cn".into()),
            token_type: Some("Bearer".into()),
            expires_at: None,
            refresh_expires_at: None,
        },
        gateway_endpoint: endpoint.into(),
        auth_endpoint: endpoint.into(),
        auth_prefix_path: "/plugin".into(),
        auth_type: "account-scoped-external-link".into(),
        app_version: "fixture-5.9.3".into(),
        cli_version: Some("fixture-2.165.0".into()),
        deployment_type: Some("SaaS".into()),
    }
}
fn metadata(auth: &Authorization) -> Value {
    json!({"accountId":auth.account_id(),"accountName":"Fixture","models":[{"id":"fixture-model","name":"Fixture model","creditMultiplier":2.5}]})
}
fn binding(auth: &Authorization) -> BuiltinBinding {
    BuiltinBinding {
        account_id: auth.account_id(),
        model_id: "fixture-model".into(),
        capabilities: json!({}),
    }
}
fn request(body: &Value) -> Request {
    Client::new()
        .post("http://127.0.0.1/placeholder")
        .header("origin", "https://fixture.invalid")
        .header("x-upstream-secret", "fixture-do-not-forward")
        .header("x-workbuddy-mcp-context", "old-ledger")
        .json(body)
        .build()
        .unwrap()
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[tokio::test]
async fn workbuddy_direct_vault_restore_metadata_update_and_explicit_removal() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("hub.json");
    let auth = authorization("https://www.workbuddy.cn");
    let runtime = Runtime::new(&config).unwrap();
    let public = metadata(&auth);
    runtime.capture(auth.clone(), public.clone()).await.unwrap();
    let mut changed = public.clone();
    changed["models"][0]["creditMultiplier"] = json!(3);
    runtime.update_metadata(changed.clone()).await.unwrap();
    #[cfg(windows)]
    {
        let root = super::super::super::state_root(&config).join("workbuddy-direct/protected");
        for entry in std::fs::read_dir(root).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            for secret in [
                "fixture-access-initial",
                "fixture-refresh-initial",
                "fixture-user",
            ] {
                assert!(!bytes.windows(secret.len()).any(|s| s == secret.as_bytes()));
            }
        }
    }
    drop(runtime); // A real Hub exit drops memory, retaining only the protected vault.
    let restored = Runtime::new(&config).unwrap();
    assert_eq!(restored.load().await.unwrap(), Some(changed.clone()));
    assert_eq!(restored.snapshot().await, Some(changed));
    let status = restored.snapshot().await.unwrap().to_string();
    assert!(!status.contains("fixture-access"));
    assert!(!status.contains("refreshToken"));
    assert!(restored.ready_for(&auth.account_id()).await);
    let mut wrong = public;
    wrong["accountId"] = json!("0".repeat(64));
    assert!(restored.capture(auth.clone(), wrong).await.is_err());
    assert!(restored.ready_for(&auth.account_id()).await);
    restored.clear().await.unwrap();
    let empty = Runtime::new(&config).unwrap();
    assert!(empty.load().await.unwrap().is_none());
}

#[tokio::test]
async fn workbuddy_direct_json_sse_tools_and_restored_authorization_need_no_host() {
    let (fixture, endpoint, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("hub.json");
    let auth = authorization(&endpoint);
    let binding = binding(&auth);
    let runtime = Runtime::new(&config).unwrap();
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    drop(runtime);
    let runtime = Runtime::new(&config).unwrap();
    runtime.load().await.unwrap();
    let first = json!({"model":"fixture-model","stream":true,"messages":[{"role":"user","content":"read"}],"tools":[{"type":"function","function":{"name":"fixture_tool","parameters":{"type":"object"}}}]});
    let response = runtime
        .forward(&binding, request(&first), deadline())
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let text = response.text().await.unwrap();
    assert!(text.contains("fixture-call"));
    assert!(text.ends_with("data: [DONE]\n\n"));
    let second = json!({"model":"fixture-model","stream":false,"messages":[{"role":"assistant","tool_calls":[{"id":"fixture-call","type":"function","function":{"name":"fixture_tool","arguments":"{}"}}]},{"role":"tool","tool_call_id":"fixture-call","content":"42"}]});
    let response = runtime
        .forward(&binding, request(&second), deadline())
        .await
        .unwrap();
    assert_eq!(
        response.json::<Value>().await.unwrap()["choices"][0]["message"]["content"],
        "done"
    );
    assert_eq!(*fixture.bodies.lock().await, vec![first, second]);
    assert_eq!(fixture.refreshes.load(Ordering::Relaxed), 0);
    runtime.clear().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn workbuddy_direct_concurrent_401s_share_one_serial_refresh_and_persist_it() {
    let (fixture, endpoint, task) = server().await;
    fixture.parallel.store(true, Ordering::Relaxed);
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("hub.json");
    let auth = authorization(&endpoint);
    let binding = binding(&auth);
    let runtime = Runtime::new(&config).unwrap();
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    let input = json!({"model":"fixture-model","need_refresh":true});
    let (one, two) = tokio::join!(
        runtime.forward(&binding, request(&input), deadline()),
        runtime.forward(&binding, request(&input), deadline())
    );
    assert!(one.unwrap().status().is_success());
    assert!(two.unwrap().status().is_success());
    assert_eq!(fixture.generations.load(Ordering::Relaxed), 4);
    assert_eq!(fixture.refreshes.load(Ordering::Relaxed), 1);
    assert_eq!(fixture.identities.load(Ordering::Relaxed), 1);
    drop(runtime);
    let restored = Runtime::new(&config).unwrap();
    restored.load().await.unwrap();
    restored
        .forward(&binding, request(&input), deadline())
        .await
        .unwrap();
    assert_eq!(fixture.refreshes.load(Ordering::Relaxed), 1);
    restored.clear().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn workbuddy_direct_expired_access_refreshes_before_generation() {
    let (fixture, endpoint, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let mut auth = authorization(&endpoint);
    auth.auth.expires_at = Some(1);
    let runtime = Runtime::new(&dir.path().join("hub.json")).unwrap();
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    runtime
        .forward(
            &binding(&auth),
            request(&json!({"model":"fixture-model"})),
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(fixture.generations.load(Ordering::Relaxed), 1);
    assert_eq!(fixture.refreshes.load(Ordering::Relaxed), 1);
    assert_eq!(fixture.identities.load(Ordering::Relaxed), 1);
    runtime.clear().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn workbuddy_direct_refuses_refreshed_account_changes_without_replaying() {
    let (fixture, endpoint, task) = server().await;
    fixture.wrong_account.store(true, Ordering::Relaxed);
    let dir = tempfile::tempdir().unwrap();
    let auth = authorization(&endpoint);
    let binding = binding(&auth);
    let runtime = Runtime::new(&dir.path().join("hub.json")).unwrap();
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    let input = json!({"model":"fixture-model","need_refresh":true});
    let error = runtime
        .forward(&binding, request(&input), deadline())
        .await
        .unwrap_err();
    assert!(error.message.contains("账户或组织已变化"));
    assert!(runtime.last_error().await.is_some());
    assert!(runtime.ready_for(&binding.account_id).await); // Do not silently fall back to the stale host.
    runtime
        .forward(&binding, request(&input), deadline())
        .await
        .unwrap_err();
    assert_eq!(fixture.generations.load(Ordering::Relaxed), 1);
    assert_eq!(fixture.refreshes.load(Ordering::Relaxed), 1);
    assert_eq!(fixture.identities.load(Ordering::Relaxed), 1);
    runtime.clear().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn workbuddy_direct_rejected_refresh_and_non_401_errors_do_not_repeat_generation() {
    let (fixture, endpoint, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let auth = authorization(&endpoint);
    let binding = binding(&auth);
    let runtime = Runtime::new(&dir.path().join("hub.json")).unwrap();
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    for status in [403, 500, 302] {
        let error = runtime
            .forward(&binding, request(&json!({"failure":status})), deadline())
            .await
            .unwrap_err();
        assert_eq!(error.status.as_u16(), status as u16);
        assert!(!format!("{error:?}").contains("fixture-upstream-private-error"));
    }
    assert_eq!(fixture.refreshes.load(Ordering::Relaxed), 0);
    fixture.reject_refresh.store(true, Ordering::Relaxed);
    let error = runtime
        .forward(&binding, request(&json!({"need_refresh":true})), deadline())
        .await
        .unwrap_err();
    assert!(!format!("{error:?}").contains("fixture-refresh-secret-echo"));
    runtime
        .forward(&binding, request(&json!({"need_refresh":true})), deadline())
        .await
        .unwrap_err();
    assert_eq!(fixture.generations.load(Ordering::Relaxed), 4);
    assert_eq!(fixture.refreshes.load(Ordering::Relaxed), 1);
    runtime.clear().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn workbuddy_direct_stream_drop_disable_and_deadline_stop_upstream_without_replay() {
    let (fixture, endpoint, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let auth = authorization(&endpoint);
    let binding = binding(&auth);
    let runtime = Runtime::new(&dir.path().join("hub.json")).unwrap();
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    let mut response = runtime
        .forward(&binding, request(&json!({"hold":true})), deadline())
        .await
        .unwrap();
    response.chunk().await.unwrap().unwrap();
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), fixture.disconnected.notified())
        .await
        .unwrap();
    let mut response = runtime
        .forward(&binding, request(&json!({"hold":true})), deadline())
        .await
        .unwrap();
    response.chunk().await.unwrap().unwrap();
    runtime.clear().await.unwrap();
    assert!(response.chunk().await.is_err());
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), fixture.disconnected.notified())
        .await
        .unwrap();
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    let mut response = runtime
        .forward(
            &binding,
            request(&json!({"hold":true})),
            Instant::now() + Duration::from_millis(150),
        )
        .await
        .unwrap();
    response.chunk().await.unwrap().unwrap();
    assert!(response.chunk().await.is_err());
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), fixture.disconnected.notified())
        .await
        .unwrap();
    assert_eq!(fixture.generations.load(Ordering::Relaxed), 3);
    assert_eq!(fixture.refreshes.load(Ordering::Relaxed), 0);
    runtime.clear().await.unwrap();
    task.abort();
}

#[test]
fn workbuddy_direct_preserves_native_identity_scalars_and_rejects_other_origins() {
    let mut auth = authorization("https://www.workbuddy.cn");
    auth.account.uid = json!(123);
    auth.account.enterprise_id = Some(json!(456));
    assert_eq!(
        auth.account_id(),
        hex::encode(Sha256::digest(br#"[123,456,"www.workbuddy.cn"]"#))
    );
    let numeric = auth.account_id();
    auth.account.uid = json!("123");
    assert_ne!(auth.account_id(), numeric);
    for value in [
        "https://attacker.invalid",
        "http://www.workbuddy.cn",
        "https://www.workbuddy.cn@attacker.invalid",
        "https://www.workbuddy.cn/?token=fixture",
        "https://www.workbuddy.cn:8443",
    ] {
        assert!(endpoint(value).is_err());
    }
}
