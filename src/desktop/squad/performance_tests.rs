//! Loopback contracts for request reuse, explicit refresh and native key import.
use super::*;
use axum::{Router, response::IntoResponse};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum TicketReply {
    #[default]
    Valid,
    OtherOrigin,
    OtherKey,
    Used,
}
#[derive(Default)]
struct State {
    calls: Mutex<Vec<(String, String)>>,
    fail_keys: AtomicBool,
    hold_resolve: AtomicBool,
    resolve_entered: tokio::sync::Notify,
    resolve_release: tokio::sync::Notify,
    fallback: AtomicBool,
    reject_fallback: AtomicBool,
    fail_profile: AtomicBool,
}
struct Fixture {
    _dir: tempfile::TempDir,
    squad: Arc<Squad>,
    state: Arc<State>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Fixture {
    fn count(&self, method: &str, path: &str) -> usize {
        self.state
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, p)| m == method && p == path)
            .count()
    }
}
async fn fixture(reply: TicketReply) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(State::default());
    let (server_origin, seen) = (origin.clone(), state.clone());
    let app = Router::new().fallback(move |request: axum::extract::Request| {
        let (origin, seen) = (server_origin.clone(), seen.clone());
        async move {
            let path = request.uri().path().to_string();
            let method = request.method().as_str().to_string();
            let call = {
                let mut calls = seen.calls.lock().unwrap();
                calls.push((method.clone(), path.clone()));
                calls.len()
            };
            let auth = request.headers().get("authorization")
                .and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
            if path == "/api/v1/keys/12/external-import" {
                // Match the site's Origin/Host + target/schema contract, not just a header spy.
                let host = url::Url::parse(&origin).unwrap().authority().to_string();
                let same_origin = request.headers().get("origin").is_some_and(|v| v == origin.as_str())
                    && request.headers().get("host").is_some_and(|v| v == host.as_str());
                let bytes = axum::body::to_bytes(request.into_body(), 4096).await.unwrap();
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                if !same_origin || auth != "Bearer synthetic-session"
                    || body != json!({"target":"tiancaispace-hub","schema_version":1}) {
                    return (StatusCode::BAD_REQUEST, axum::Json(json!({"code":400,"message":"Invalid request origin or import contract"}))).into_response();
                }
                let mut link = url::Url::parse("tiancaispacehub://import/v1").unwrap();
                link.query_pairs_mut()
                    .append_pair("origin", if reply == TicketReply::OtherOrigin { "https://different.invalid" } else { &origin })
                    .append_pair("ticket", &"a".repeat(43));
                return axum::Json(json!({"code":0,"data":{"deeplink":link.as_str()}})).into_response();
            }
            if path == "/api/v1/external-import/resolve" {
                assert!(auth.is_empty(), "account authorization must not be sent to ticket redemption");
                let bytes = axum::body::to_bytes(request.into_body(), 4096).await.unwrap();
                assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(),
                    json!({"target":"tiancaispace-hub","schema_version":1,"ticket":"a".repeat(43)}));
                if seen.hold_resolve.load(Ordering::SeqCst) {
                    seen.resolve_entered.notify_one();
                    seen.resolve_release.notified().await;
                }
                if reply == TicketReply::Used {
                    return (StatusCode::NOT_FOUND, axum::Json(json!({"code":404}))).into_response();
                }
                return axum::Json(json!({"code":0,"data":{
                    "schema_version":1,"target":"tiancaispace-hub",
                    "source":{"origin":origin,"site_name":"合成站点","key_id":if reply == TicketReply::OtherKey {"99"} else {"12"},"key_name":"合成密钥"},
                    "provider":{"name":"合成渠道","protocol":"openai_responses","base_url":format!("{origin}/v1"),"models_url":format!("{origin}/v1/models"),"api_key":"synthetic-model-key","models":["model-a"],"model_aliases":{}}
                }})).into_response();
            }
            if path == "/v1/models" {
                assert_eq!(auth, "Bearer synthetic-model-key");
                return axum::Json(json!({"data":[{"id":"model-a"},{"id":"unrelated-model"}]})).into_response();
            }
            if path == "/api/v1/auth/refresh" {
                return axum::Json(json!({"code":0,"data":{"access_token":"synthetic-session","refresh_token":"","expires_in":3600}})).into_response();
            }
            if auth == "Bearer expired" {
                return (StatusCode::UNAUTHORIZED, axum::Json(json!({"code":401}))).into_response();
            }
            if path == "/api/v1/auth/me" && seen.fail_profile.swap(false,Ordering::SeqCst) {
                return (StatusCode::SERVICE_UNAVAILABLE,axum::Json(json!({"code":503,"message":"synthetic temporarily unavailable"}))).into_response();
            }
            if path == "/api/v1/invoices/deliveries/9" {
                return ([("content-type", "application/pdf")], b"synthetic-pdf".to_vec()).into_response();
            }
            if path == "/api/v1/keys" && method == "GET" && seen.fail_keys.swap(false, Ordering::SeqCst) {
                return (StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({"code":503,"message":"synthetic failure"}))).into_response();
            }
            if path == "/api/v1/user/organization/personal-fallback" && method == "PUT" {
                if seen.reject_fallback.swap(false,Ordering::SeqCst) {
                    return (StatusCode::FORBIDDEN,axum::Json(json!({"code":403,"message":"synthetic permission denied"}))).into_response();
                }
                let bytes=axum::body::to_bytes(request.into_body(),4096).await.unwrap();
                let body:Value=serde_json::from_slice(&bytes).unwrap();
                seen.fallback.store(body["allow"].as_bool().unwrap(),Ordering::SeqCst);
                return axum::Json(json!({"code":0,"data":{"allow_personal_balance_fallback":seen.fallback.load(Ordering::SeqCst)}})).into_response();
            }
            let data = match path.as_str() {
                "/api/v1/settings/public" => json!({"model_plaza_enabled":true}),
                "/api/v1/auth/me" => json!({"id":7,"username":"Synthetic","balance":call}),
                "/api/v1/user/organization" => json!({"can_view_organization_billing":true,"allow_personal_balance_fallback":seen.fallback.load(Ordering::SeqCst)}),
                "/api/v1/payment/checkout-info" => json!({"subscription_usd_to_cny_rate":7.15,"balance_recharge_multiplier":0.14,"methods":{"alipay":{"available":true,"currency":"CNY"}}}),
                "/api/v1/keys" => json!({"items":[{"id":12,"name":"合成密钥"}],"read":call}),
                "/api/v1/usage/dashboard/api-keys-usage" => json!({"stats":{"12":{"api_key_id":12,"today_actual_cost":0.125,"total_actual_cost":3.75}}}),
                "/api/v1/payment/orders/9" => json!({"id":9,"status":"pending","read":call}),
                _ => json!({"read":call}),
            };
            axum::Json(json!({"code":0,"data":data})).into_response()
        }
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let squad = Arc::new(
        Squad::synthetic(
            dir.path().join("squad"),
            Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap(),
            origin,
        )
        .unwrap(),
    );
    squad
        .save(&Session {
            access_token: "synthetic-session".into(),
            refresh_token: String::new(),
            user_id: 7,
            generation: "generation-one".into(),
            expires_at: 0,
        })
        .unwrap();
    Fixture {
        _dir: dir,
        squad,
        state,
        server,
    }
}

fn reopen(f: &Fixture) -> Squad {
    Squad::synthetic(
        f.squad.root.clone(),
        f.squad.http.clone(),
        f.squad.origin.clone(),
    )
    .unwrap()
}
#[tokio::test]
async fn reopening_hub_restores_saved_session_and_rotates_expired_access_without_login() {
    let f = fixture(TicketReply::Valid).await;
    let original = f.squad.session().unwrap().unwrap();
    let restored = reopen(&f);
    let status = restored.status().await.unwrap();
    assert_eq!(status["signedIn"], true);
    assert_eq!(status["user"]["id"], 7);
    assert_eq!(status["accountGeneration"], original.generation);
    let mut expired = original.clone();
    expired.access_token = "expired".into();
    expired.refresh_token = "synthetic-refresh".into();
    f.squad.save(&expired).unwrap();
    let restored = reopen(&f);
    let status = restored.status().await.unwrap();
    assert_eq!(status["signedIn"], true);
    assert_eq!(status["accountGeneration"], original.generation);
    assert_eq!(
        restored.session().unwrap().unwrap().access_token,
        "synthetic-session"
    );
    assert_eq!(reopen(&f).profile().await.unwrap()["signedIn"], true);
    let calls = f.state.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|(_, p)| p.starts_with("/api/v1/auth/refresh"))
            .count(),
        1
    );
    assert!(
        !calls
            .iter()
            .any(|(_, p)| p.contains("/login") || p.contains("/hub/begin"))
    );
}
#[tokio::test]
async fn temporary_restore_failure_keeps_persisted_session_for_retry() {
    let f = fixture(TicketReply::Valid).await;
    let before = f.squad.secrets.get("account-session").unwrap();
    f.state.fail_profile.store(true, Ordering::SeqCst);
    let restored = reopen(&f);
    assert!(restored.status().await.is_err());
    assert_eq!(restored.secrets.get("account-session").unwrap(), before);
    assert_eq!(restored.status().await.unwrap()["signedIn"], true);
}
#[tokio::test]
async fn missing_expired_or_explicitly_signed_out_session_never_auto_signs_in() {
    let f = fixture(TicketReply::Valid).await;
    let mut session = f.squad.session().unwrap().unwrap();
    session.access_token = "expired".into();
    session.refresh_token.clear();
    f.squad.save(&session).unwrap();
    let status = reopen(&f).status().await.unwrap();
    assert_eq!(status["signedIn"], false);
    assert_eq!(status["expired"], true);
    assert!(
        !f.state
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(_, p)| p.contains("settings/public"))
    );
    f.squad.logout().await.unwrap();
    let before = f.state.calls.lock().unwrap().len();
    assert_eq!(reopen(&f).status().await.unwrap()["signedIn"], false);
    assert_eq!(f.state.calls.lock().unwrap().len(), before);
}

#[test]
fn service_reuses_connection_pool_until_network_configuration_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = crate::config::AppConfig::default();
    config.outbound_proxy.mode = crate::config::OutboundProxyMode::Direct;
    let mut cache = ServiceCache::default();
    let first = cache.get(&path, &config).unwrap();
    assert!(Arc::ptr_eq(&first, &cache.get(&path, &config).unwrap()));
    config.theme = Some("dark".into());
    assert!(Arc::ptr_eq(&first, &cache.get(&path, &config).unwrap()));
    config.bind = "127.0.0.1:49120".into();
    let next = cache.get(&path, &config).unwrap();
    assert!(!Arc::ptr_eq(&first, &next));
    config.outbound_proxy.mode = crate::config::OutboundProxyMode::System;
    let proxy = cache.get(&path, &config).unwrap();
    assert!(!Arc::ptr_eq(&next, &proxy));
    assert!(!Arc::ptr_eq(
        &proxy,
        &cache.get(&dir.path().join("other.toml"), &config).unwrap()
    ));
}

#[tokio::test]
async fn repeat_navigation_reuses_metadata_and_query_cache_but_explicit_refresh_is_live() {
    let f = fixture(TicketReply::Valid).await;
    for _ in 0..3 {
        assert_eq!(f.squad.status().await.unwrap()["signedIn"], true);
        f.squad
            .request("GET", "/keys", vec![("page".into(), "1".into())], None)
            .await
            .unwrap();
    }
    for path in [
        "/api/v1/settings/public",
        "/api/v1/auth/me",
        "/api/v1/user/organization",
        "/api/v1/keys",
    ] {
        assert_eq!(f.count("GET", path), 1, "{path}");
    }
    f.squad
        .request("GET", "/keys", vec![("page".into(), "2".into())], None)
        .await
        .unwrap();
    assert_eq!(f.count("GET", "/api/v1/keys"), 2);
    f.squad.invalidate();
    f.squad.status().await.unwrap();
    f.squad
        .request("GET", "/keys", vec![("page".into(), "1".into())], None)
        .await
        .unwrap();
    assert_eq!(f.count("GET", "/api/v1/auth/me"), 2);
    assert_eq!(f.count("GET", "/api/v1/keys"), 3);
}

#[tokio::test]
async fn errors_are_not_cached_saves_refresh_lists_and_order_verification_stays_live() {
    let f = fixture(TicketReply::Valid).await;
    f.state.fail_keys.store(true, Ordering::SeqCst);
    assert!(f.squad.request("GET", "/keys", vec![], None).await.is_err());
    let before = f.squad.request("GET", "/keys", vec![], None).await.unwrap();
    assert_eq!(
        f.squad.request("GET", "/keys", vec![], None).await.unwrap(),
        before
    );
    assert_eq!(f.count("GET", "/api/v1/keys"), 2);
    f.squad
        .request("PUT", "/keys/12", vec![], Some(json!({"name":"新名称"})))
        .await
        .unwrap();
    let after = f.squad.request("GET", "/keys", vec![], None).await.unwrap();
    assert_ne!(before["read"], after["read"]);
    for _ in 0..2 {
        f.squad.payment_recovery(9).await.unwrap();
    }
    assert_eq!(f.count("GET", "/api/v1/payment/orders/9"), 2);
}

#[tokio::test]
async fn switching_account_or_login_generation_never_reuses_account_data() {
    let f = fixture(TicketReply::Valid).await;
    let first = f.squad.request("GET", "/keys", vec![], None).await.unwrap();
    let mut session = f.squad.session().unwrap().unwrap();
    session.generation = "generation-two".into();
    // Another service can update the persisted session without clearing this instance's cache.
    f.squad
        .secrets
        .set("account-session", &serde_json::to_vec(&session).unwrap())
        .unwrap();
    let next = f.squad.request("GET", "/keys", vec![], None).await.unwrap();
    assert_ne!(first["read"], next["read"]);
    session.user_id = 8;
    f.squad
        .secrets
        .set("account-session", &serde_json::to_vec(&session).unwrap())
        .unwrap();
    let other = f.squad.request("GET", "/keys", vec![], None).await.unwrap();
    assert_ne!(next["read"], other["read"]);
    f.squad.logout().await.unwrap();
    assert!(f.squad.request("GET", "/keys", vec![], None).await.is_err());
}

#[tokio::test]
async fn native_import_creates_and_redeems_once_with_site_contract_and_keeps_protocol_scope() {
    let f = fixture(TicketReply::Valid).await;
    let draft = f.squad.prepare_import(12).await.unwrap();
    assert_eq!(draft.source.key_id, "12");
    assert_eq!(draft.source.origin, f.squad.origin);
    assert_eq!(draft.provider.models, ["model-a"]);
    assert!(!draft.provider.enabled);
    assert_eq!(f.count("POST", "/api/v1/keys/12/external-import"), 1);
    assert_eq!(f.count("POST", "/api/v1/external-import/resolve"), 1);
    assert_eq!(f.count("GET", "/v1/models"), 1);
}

#[tokio::test]
async fn mismatched_or_used_ticket_stops_without_discovery_or_automatic_replay() {
    for (reply, resolve_count) in [
        (TicketReply::OtherOrigin, 0),
        (TicketReply::OtherKey, 1),
        (TicketReply::Used, 1),
    ] {
        let f = fixture(reply).await;
        assert!(f.squad.prepare_import(12).await.is_err());
        assert_eq!(f.count("POST", "/api/v1/keys/12/external-import"), 1);
        assert_eq!(
            f.count("POST", "/api/v1/external-import/resolve"),
            resolve_count
        );
        assert_eq!(f.count("GET", "/v1/models"), 0);
    }
}

#[tokio::test]
async fn account_changed_during_redemption_discards_draft_without_discovery() {
    let f = fixture(TicketReply::Valid).await;
    f.state.hold_resolve.store(true, Ordering::SeqCst);
    let (result, ()) = tokio::join!(f.squad.prepare_import(12), async {
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            f.state.resolve_entered.notified(),
        )
        .await
        .unwrap();
        let mut session = f.squad.session().unwrap().unwrap();
        session.generation = "different-login".into();
        f.squad.save(&session).unwrap();
        f.state.resolve_release.notify_one();
    });
    assert!(result.err().unwrap().to_string().contains("账户已变化"));
    assert_eq!(f.count("GET", "/v1/models"), 0);
}

#[tokio::test]
async fn download_401_refreshes_even_when_previous_profile_is_cached() {
    let f = fixture(TicketReply::Valid).await;
    f.squad.status().await.unwrap();
    let mut session = f.squad.session().unwrap().unwrap();
    session.access_token = "expired".into();
    session.refresh_token = "synthetic-refresh".into();
    f.squad
        .secrets
        .set("account-session", &serde_json::to_vec(&session).unwrap())
        .unwrap();
    let destination = f._dir.path().join("synthetic.pdf");
    f.squad
        .download("/invoices/deliveries/9", &destination)
        .await
        .unwrap();
    assert_eq!(std::fs::read(destination).unwrap(), b"synthetic-pdf");
    assert_eq!(f.count("POST", "/api/v1/auth/refresh"), 1);
    assert_eq!(f.count("GET", "/api/v1/invoices/deliveries/9"), 2);
}

#[test]
fn removed_account_features_stay_hidden_even_with_all_site_permissions_enabled() {
    let settings = json!({"model_plaza_enabled":true,"available_channels_enabled":true,"payment_enabled":true,
        "subscription_enabled":true,"affiliate_enabled":true,"channel_monitor_enabled":true,
        "custom_menu_items":[{"id":"one","visibility":"user","title":"站内工具"}]});
    let organization = json!({"can_view_organization_billing":true,"can_manage_organization":true,"can_manage_department":true,
        "can_view_organization_accounts":true,"can_view_organization_structure":true,"can_view_organization_audit":true});
    let ids = menus(&settings, &json!({"role":"admin"}), &organization)
        .into_iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "account",
            "keys",
            "usage",
            "organization",
            "organization-admin",
            "organization-usage",
            "channels",
            "purchase",
            "profile"
        ]
    );
    assert!(menus(&settings, &json!({"run_mode":"simple"}), &organization).len() == 3);
}

#[tokio::test]
async fn currency_label_needs_no_checkout_and_payment_confirmation_remains_live() {
    let f = fixture(TicketReply::Valid).await;
    let state = f.squad.status().await.unwrap();
    assert_eq!(state["money"], json!({}));
    assert!(
        state["accountGeneration"]
            .as_str()
            .is_some_and(|v| !v.is_empty())
    );
    f.squad.profile().await.unwrap();
    f.squad.status().await.unwrap();
    assert_eq!(f.count("GET", "/api/v1/payment/checkout-info"), 0);
    for _ in 0..2 {
        f.squad
            .request("GET", "/payment/checkout-info", vec![], None)
            .await
            .unwrap();
    }
    assert_eq!(f.count("GET", "/api/v1/payment/checkout-info"), 2);
    f.squad.invalidate();
    f.squad.profile().await.unwrap();
    assert_eq!(f.count("GET", "/api/v1/payment/checkout-info"), 2);
}

#[tokio::test]
async fn removed_model_plaza_cannot_be_fetched_through_old_account_entry() {
    let f = fixture(TicketReply::Valid).await;
    let error = f
        .squad
        .request("GET", "/model-plaza", vec![], None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("不支持此账户操作"));
    assert_eq!(f.count("GET", "/api/v1/model-plaza"), 0);
    assert_eq!(f.count("GET", "/api/v1/auth/me"), 0);
    assert!(
        !menus(
            &json!({"model_plaza_enabled":true,"available_channels_enabled":true}),
            &json!({}),
            &json!({})
        )
        .iter()
        .any(|menu| menu["id"] == "models")
    );
    assert!(
        menus(
            &json!({"available_channels_enabled":true}),
            &json!({}),
            &json!({})
        )
        .iter()
        .any(|menu| menu["id"] == "channels")
    );
}
#[tokio::test]
async fn stale_account_draft_cannot_create_order_or_change_fallback() {
    let f = fixture(TicketReply::Valid).await;
    let session = f.squad.session().unwrap().unwrap();
    for (id, generation) in [
        (session.user_id + 1, session.generation.clone()),
        (session.user_id, "stale-generation".into()),
    ] {
        let expected = ExpectedAccount {
            user_id: id,
            generation,
        };
        for (method, path, body) in [
            ("POST", "/payment/orders", json!({"amount":100})),
            (
                "PUT",
                "/user/organization/personal-fallback",
                json!({"allow":true}),
            ),
        ] {
            let error = f
                .squad
                .request_checked(method, path, vec![], Some(body), Some(&expected))
                .await
                .unwrap_err();
            assert!(error.to_string().contains("账户已切换"));
            assert_eq!(f.count(method, &format!("/api/v1{path}")), 0);
        }
    }
    assert!(f.squad.pending().unwrap().is_empty());
}
#[tokio::test]
async fn fallback_save_only_changes_after_confirmed_success_and_permission_error_is_not_replayed() {
    let f = fixture(TicketReply::Valid).await;
    let s = f.squad.session().unwrap().unwrap();
    let expected = ExpectedAccount {
        user_id: s.user_id,
        generation: s.generation,
    };
    f.state.reject_fallback.store(true, Ordering::SeqCst);
    assert!(
        f.squad
            .request_checked(
                "PUT",
                "/user/organization/personal-fallback",
                vec![],
                Some(json!({"allow":true})),
                Some(&expected)
            )
            .await
            .is_err()
    );
    assert!(!f.state.fallback.load(Ordering::SeqCst));
    assert_eq!(
        f.count("PUT", "/api/v1/user/organization/personal-fallback"),
        1
    );
    let result = f
        .squad
        .request_checked(
            "PUT",
            "/user/organization/personal-fallback",
            vec![],
            Some(json!({"allow":true})),
            Some(&expected),
        )
        .await
        .unwrap();
    assert_eq!(result["allow_personal_balance_fallback"], true);
    assert_eq!(
        f.squad.status().await.unwrap()["organization"]["allow_personal_balance_fallback"],
        true
    );
    f.squad
        .request_checked(
            "PUT",
            "/user/organization/personal-fallback",
            vec![],
            Some(json!({"allow":false})),
            Some(&expected),
        )
        .await
        .unwrap();
    assert_eq!(
        f.squad.status().await.unwrap()["organization"]["allow_personal_balance_fallback"],
        false
    );
    assert_eq!(
        f.count("PUT", "/api/v1/user/organization/personal-fallback"),
        3
    );
}
