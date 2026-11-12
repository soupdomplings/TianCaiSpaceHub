use super::*;
use axum::{
    Json, Router,
    extract::State as AxumState,
    http::Uri,
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

#[derive(Default)]
struct Fixture {
    credits: AtomicUsize,
    refreshes: AtomicUsize,
    identities: AtomicUsize,
    enterprise: AtomicBool,
    mode: AtomicUsize,
    started: Notify,
    release: Notify,
}

async fn credits(
    AxumState(f): AxumState<Arc<Fixture>>,
    uri: Uri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> AxumResponse {
    f.credits.fetch_add(1, Ordering::Relaxed);
    let enterprise = f.enterprise.load(Ordering::Relaxed);
    assert_eq!(
        uri.path(),
        if enterprise {
            ENTERPRISE_PATH
        } else {
            PERSONAL_PATH
        }
    );
    assert_eq!(body, json!({}));
    assert_eq!(headers["x-user-id"], "fixture-user");
    assert_eq!(headers["accept-language"], "zh");
    assert!(!headers.contains_key("x-workbuddy-mcp-context"));
    if enterprise {
        assert_eq!(headers["x-enterprise-id"], "fixture-org");
    } else {
        assert!(!headers.contains_key("x-enterprise-id"));
    }
    let old = headers["authorization"] == "Bearer fixture-access-initial";
    match f.mode.load(Ordering::Relaxed) {
        1 => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "fixture-secret-server-error",
            )
                .into_response();
        }
        2 if old => return (StatusCode::UNAUTHORIZED, "fixture-access-initial").into_response(),
        3 => return (StatusCode::UNAUTHORIZED, "fixture-refresh-must-not-repeat").into_response(),
        4 if old => {
            return Json(json!({"code":401,"msg":"fixture-secret-auth-error"})).into_response();
        }
        5 => {
            f.started.notify_one();
            f.release.notified().await;
        }
        6 => {
            return Json(json!({"code":0,"data":{"private":"fixture-private-unmapped-response"}}))
                .into_response();
        }
        _ => (),
    }
    if enterprise {
        Json(json!({"code":0,"data":{"limit_num":1500,"used_num":25,"internalSecret":"fixture-private-field"}})).into_response()
    } else {
        Json(json!({"code":0,"data":{"SubscriptionPackageName":"Fixture plan","Packages":[
            {"PackageCode":"subscription","PackageName":"订阅积分","CycleTotalCapacity":"1500","CycleRemainCapacity":"1234.5","CycleUsedCapacity":"265.5","private":"fixture-private-field"},
            {"PackageCode":"bonus","PackageName":"赠送积分","CycleTotalCapacity":100,"CycleRemainCapacity":80,"CycleUsedCapacity":20}
        ],"private":"fixture-private-field"}})).into_response()
    }
}

async fn refresh(
    AxumState(f): AxumState<Arc<Fixture>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> AxumResponse {
    f.refreshes.fetch_add(1, Ordering::Relaxed);
    assert_eq!(headers["x-refresh-token"], "fixture-refresh-initial");
    assert_eq!(body, json!({}));
    Json(json!({"code":0,"data":{"accessToken":"fixture-access-new","refreshToken":"fixture-refresh-new","expiresIn":3600,"refreshExpiresIn":86400,"domain":"www.workbuddy.cn"}})).into_response()
}

async fn identity(AxumState(f): AxumState<Arc<Fixture>>, headers: HeaderMap) -> AxumResponse {
    f.identities.fetch_add(1, Ordering::Relaxed);
    assert_eq!(headers["authorization"], "Bearer fixture-access-new");
    let mut account = json!({"uid":"fixture-user","type":"personal"});
    if f.enterprise.load(Ordering::Relaxed) {
        account["enterpriseId"] = json!("fixture-org");
        account["type"] = json!("enterprise");
    }
    Json(json!({"code":0,"data":account})).into_response()
}

async fn server(enterprise: bool) -> (Arc<Fixture>, String, tokio::task::JoinHandle<()>) {
    let f = Arc::new(Fixture::default());
    f.enterprise.store(enterprise, Ordering::Relaxed);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route(PERSONAL_PATH, post(credits))
        .route(ENTERPRISE_PATH, post(credits))
        .route("/v2/plugin/auth/token/refresh", post(refresh))
        .route("/v2/plugin/account", get(identity))
        .with_state(f.clone());
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (f, endpoint, task)
}

fn authorization(endpoint: &str, enterprise: bool) -> Authorization {
    Authorization {
        account: Account {
            uid: json!("fixture-user"),
            enterprise_id: enterprise.then(|| json!("fixture-org")),
            kind: Some(if enterprise { "enterprise" } else { "personal" }.into()),
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
        cli_version: None,
        deployment_type: Some("SaaS".into()),
    }
}
fn metadata(auth: &Authorization) -> Value {
    json!({"accountId":auth.account_id(),"accountName":"Fixture account","models":[{"id":"fixture-model","name":"Fixture"}]})
}

#[tokio::test]
async fn workbuddy_account_status_is_local_and_exposes_expiry_without_credentials() {
    let (f, endpoint, task) = server(false).await;
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(&dir.path().join("hub.json")).unwrap();
    assert_eq!(
        runtime.public_account_status().await["authorization"]["status"],
        "missing"
    );
    let mut auth = authorization(&endpoint, false);
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    let status = runtime.public_account_status().await;
    assert_eq!(status["authorization"]["status"], "available");
    assert_eq!(status["credits"]["status"], "unknown");
    assert_eq!(status["accountName"], "Fixture account");
    auth.auth.expires_at = Some(1);
    auth.auth.refresh_expires_at = Some(now_ms() + 60_000);
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    assert_eq!(
        runtime.public_account_status().await["authorization"]["status"],
        "refreshable"
    );
    auth.auth.refresh_expires_at = Some(1);
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    let status = runtime.public_account_status().await;
    assert_eq!(status["authorization"]["status"], "expired");
    assert_eq!(status["authorization"]["expiresAt"], 1);
    assert_eq!(status["authorization"]["refreshExpiresAt"], 1);
    let text = status.to_string();
    for secret in [
        "fixture-access",
        "fixture-refresh",
        "accessToken",
        "refreshToken",
        "fixture-user",
    ] {
        assert!(!text.contains(secret));
    }
    assert_eq!(f.credits.load(Ordering::Relaxed), 0);
    assert_eq!(f.refreshes.load(Ordering::Relaxed), 0);
    assert_eq!(f.identities.load(Ordering::Relaxed), 0);
    runtime.clear().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn workbuddy_account_personal_refresh_keeps_real_buckets_and_cache_only_in_memory() {
    let (f, endpoint, task) = server(false).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hub.json");
    let auth = authorization(&endpoint, false);
    let runtime = Runtime::new(&path).unwrap();
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    let status = runtime.refresh_account().await.unwrap();
    let credits = &status["credits"];
    assert_eq!(credits["status"], "ready");
    assert_eq!(credits["scope"], "personalPackages");
    assert_eq!(credits["remaining"], 1314.5);
    assert_eq!(credits["total"], 1600.0);
    assert_eq!(credits["used"], 285.5);
    assert_eq!(credits["quotaItems"][0]["remaining"], 1234.5);
    assert_eq!(credits["quotaItems"][1]["label"], "赠送积分");
    assert!(credits["updatedAt"].as_u64().unwrap() > 0);
    assert!(!status.to_string().contains("fixture-private"));
    assert_eq!(runtime.public_account_status().await, status);
    assert_eq!(f.credits.load(Ordering::Relaxed), 1);
    drop(runtime);
    let restored = Runtime::new(&path).unwrap();
    restored.load().await.unwrap();
    assert_eq!(
        restored.public_account_status().await["credits"]["status"],
        "unknown"
    );
    assert_eq!(f.credits.load(Ordering::Relaxed), 1);
    restored.clear().await.unwrap();
    task.abort();
}

#[test]
fn workbuddy_account_credit_projection_preserves_unlimited_missing_zero_and_native_units() {
    let enterprise = enterprise_credits(&json!({"limit_num":1500,"used_num":25})).unwrap();
    assert_eq!(enterprise["remaining"], 1475.0);
    assert_eq!(enterprise["remainingBasis"], "limitMinusUsed");
    let unlimited = enterprise_credits(&json!({"limit_num":-1,"used_num":25})).unwrap();
    assert_eq!(unlimited["unlimited"], true);
    assert_eq!(unlimited["used"], 25);
    assert!(unlimited.get("remaining").is_none());
    assert!(unlimited.get("total").is_none());
    let partial = enterprise_credits(&json!({"limit_num":100})).unwrap();
    assert!(partial.get("used").is_none());
    assert!(partial.get("remaining").is_none());
    let personal = personal_credits(&json!({"Packages":[
        {"PackageCode":"paid","CycleTotalCapacity":1000,"CycleRemainCapacity":0,"CycleUsedCapacity":1000},
        {"PackageCode":"bonus","CycleTotalCapacity":100,"CycleRemainCapacity":50}
    ]})).unwrap();
    assert_eq!(personal["remaining"], 50.0);
    assert_eq!(personal["total"], 1100.0);
    assert!(personal.get("used").is_none());
    assert_eq!(personal["quotaItems"][0]["remaining"], 0);
    assert!(personal["quotaItems"][1].get("used").is_none());
    let empty = personal_credits(&json!({"Packages":[]})).unwrap();
    assert!(empty.get("remaining").is_none());
    assert_eq!(empty["status"], "unknown");
    assert!(enterprise_credits(&json!({"limit_num":"unlimited","used_num":"NaN"})).is_err());
    assert!(personal_credits(&json!({"remaining":20})).is_err());
}

#[tokio::test]
async fn workbuddy_account_401_and_business_401_refresh_once_then_check_original_identity() {
    let (f, endpoint, task) = server(true).await;
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(&dir.path().join("hub.json")).unwrap();
    let auth = authorization(&endpoint, true);
    for mode in [2, 4] {
        runtime
            .capture(auth.clone(), metadata(&auth))
            .await
            .unwrap();
        f.mode.store(mode, Ordering::Relaxed);
        let status = runtime.refresh_account().await.unwrap();
        assert_eq!(status["credits"]["remaining"], 1475.0);
    }
    assert_eq!(f.credits.load(Ordering::Relaxed), 4);
    assert_eq!(f.refreshes.load(Ordering::Relaxed), 2);
    assert_eq!(f.identities.load(Ordering::Relaxed), 2);
    runtime.clear().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn workbuddy_account_concurrent_updates_reuse_the_same_serial_token_refresh() {
    let (f, endpoint, task) = server(false).await;
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(&dir.path().join("hub.json")).unwrap();
    let auth = authorization(&endpoint, false);
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    f.mode.store(2, Ordering::Relaxed);
    let (one, two) = tokio::join!(runtime.refresh_account(), runtime.refresh_account());
    assert_eq!(one.unwrap()["credits"]["remaining"], 1314.5);
    assert_eq!(two.unwrap()["credits"]["remaining"], 1314.5);
    assert_eq!(f.credits.load(Ordering::Relaxed), 3);
    assert_eq!(f.refreshes.load(Ordering::Relaxed), 1);
    assert_eq!(f.identities.load(Ordering::Relaxed), 1);
    runtime.clear().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn workbuddy_account_failure_keeps_last_real_values_and_does_not_leak_or_retry() {
    let (f, endpoint, task) = server(false).await;
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(&dir.path().join("hub.json")).unwrap();
    let auth = authorization(&endpoint, false);
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    let first = runtime.refresh_account().await.unwrap();
    f.mode.store(1, Ordering::Relaxed);
    let error = runtime.refresh_account().await.unwrap_err();
    let failed = runtime.public_account_status().await;
    assert_eq!(failed["credits"]["status"], "error");
    assert_eq!(
        failed["credits"]["remaining"],
        first["credits"]["remaining"]
    );
    assert_eq!(
        failed["credits"]["updatedAt"],
        first["credits"]["updatedAt"]
    );
    assert!(!format!("{error:?}{failed}").contains("fixture-secret-server-error"));
    assert_eq!(f.credits.load(Ordering::Relaxed), 2);
    assert_eq!(f.refreshes.load(Ordering::Relaxed), 0);
    f.mode.store(6, Ordering::Relaxed);
    runtime.refresh_account().await.unwrap_err();
    assert!(
        !runtime
            .public_account_status()
            .await
            .to_string()
            .contains("fixture-private-unmapped")
    );
    runtime.clear().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn workbuddy_account_revocation_and_same_account_token_changes_discard_late_results() {
    let (f, endpoint, task) = server(false).await;
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(Runtime::new(&dir.path().join("hub.json")).unwrap());
    let auth = authorization(&endpoint, false);
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    f.mode.store(5, Ordering::Relaxed);
    let worker = {
        let runtime = runtime.clone();
        tokio::spawn(async move { runtime.refresh_account().await })
    };
    f.started.notified().await;
    let mut rotated = auth.clone();
    rotated.auth.access_token = "fixture-new-capture".into();
    runtime
        .capture(rotated.clone(), metadata(&rotated))
        .await
        .unwrap();
    f.release.notify_one();
    assert!(worker.await.unwrap().is_err());
    assert_eq!(
        runtime.public_account_status().await["credits"]["status"],
        "unknown"
    );
    let worker = {
        let runtime = runtime.clone();
        tokio::spawn(async move { runtime.refresh_account().await })
    };
    f.started.notified().await;
    let mut different = rotated;
    different.account.uid = json!("new-account");
    runtime
        .capture(different.clone(), metadata(&different))
        .await
        .unwrap();
    assert!(worker.await.unwrap().is_err());
    f.release.notify_one();
    let status = runtime.public_account_status().await;
    assert_eq!(status["accountId"], different.account_id());
    assert_eq!(status["credits"]["status"], "unknown");
    runtime.clear().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn workbuddy_account_expired_and_rejected_authorization_never_invents_a_zero_balance() {
    let (f, endpoint, task) = server(false).await;
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(&dir.path().join("hub.json")).unwrap();
    let mut auth = authorization(&endpoint, false);
    auth.auth.expires_at = Some(1);
    auth.auth.refresh_expires_at = Some(1);
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    runtime.refresh_account().await.unwrap_err();
    let status = runtime.public_account_status().await;
    assert_eq!(status["credits"]["status"], "error");
    assert!(status["credits"].get("remaining").is_none());
    assert_eq!(status["authorization"]["status"], "rejected");
    assert_eq!(f.credits.load(Ordering::Relaxed), 0);
    assert_eq!(f.refreshes.load(Ordering::Relaxed), 0);
    runtime.clear().await.unwrap();
    auth.auth.expires_at = None;
    auth.auth.refresh_expires_at = None;
    runtime
        .capture(auth.clone(), metadata(&auth))
        .await
        .unwrap();
    f.mode.store(3, Ordering::Relaxed);
    runtime.refresh_account().await.unwrap_err();
    runtime.refresh_account().await.unwrap_err();
    assert_eq!(f.credits.load(Ordering::Relaxed), 2);
    assert_eq!(f.refreshes.load(Ordering::Relaxed), 1);
    runtime.clear().await.unwrap();
    task.abort();
}
