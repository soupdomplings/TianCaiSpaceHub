//! Explicit browser authorization. Never opens a browser or reads browser profiles here.
use super::squad::{SESSION_GATE, Session, Squad, WRITE_GATE};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize)]
struct Pending {
    id: String,
    verifier: String,
    expires_at: i64,
    generation: Option<String>,
}
fn now() -> i64 {
    crate::ai_gateway::request_log::now_ms()
}
fn verifier() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn challenge(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}
fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
impl Squad {
    pub async fn browser_capability(&self) -> Result<Value> {
        // This endpoint is optional. A missing route (including an HTML 404 or
        // SPA fallback) is normal capability discovery, not a failed login.
        let response = self
            .http
            .get(format!("{}/api/v1/auth/hub/capabilities", self.origin))
            .header("accept", "application/json")
            .timeout(std::time::Duration::from_secs(6))
            .send()
            .await;
        let Ok(mut response) = response else {
            return Ok(
                json!({"state":"unavailable","message":"暂时无法检查浏览器登录，请使用内置登录，或稍后重试"}),
            );
        };
        let status = response.status();
        if matches!(status.as_u16(), 404 | 405 | 410 | 501) {
            return Ok(browser_unsupported());
        }
        if !status.is_success() {
            return Ok(
                json!({"state":"unavailable","message":"站点暂时无法提供浏览器登录，请使用内置登录"}),
            );
        }
        let mut bytes = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if bytes.len() + chunk.len() <= 64 * 1024 => bytes.extend(chunk),
                Ok(None) => break,
                _ => {
                    return Ok(
                        json!({"state":"unavailable","message":"浏览器登录检查未完成，请使用内置登录或重新检查"}),
                    );
                }
            }
        }
        if serde_json::from_slice::<Value>(&bytes)
            .is_ok_and(|v| v["code"] == 0 && v["data"]["version"] == 1)
        {
            Ok(json!({"state":"available"}))
        } else {
            Ok(browser_unsupported())
        }
    }
    pub async fn profile(&self) -> Result<Value> {
        let _write = WRITE_GATE.lock().await;
        if self.session()?.is_none() {
            return Ok(json!({"signedIn":false}));
        }
        match self.authenticated(Method::GET, "/auth/me", &[], None).await {
            Ok(user) => Ok(json!({"signedIn":true,"user":user,"money":self.display_money().await})),
            Err(error)
                if error
                    .downcast_ref::<super::squad::Rejection>()
                    .is_some_and(|r| r.status == reqwest::StatusCode::UNAUTHORIZED) =>
            {
                Ok(json!({"signedIn":false,"expired":true}))
            }
            Err(error) => Err(error),
        }
    }
    pub async fn browser_start(&self) -> Result<Value> {
        let _write = WRITE_GATE.lock().await;
        let capability = self.browser_capability().await?;
        if capability["state"] != "available" {
            return Ok(capability);
        }
        let verifier = verifier();
        let (status, value) = self
            .send(
                Method::POST,
                "/auth/hub/begin",
                &[],
                Some(&json!({"challenge":challenge(&verifier)})),
                None,
            )
            .await?;
        let data = Self::unwrap(status, value)?;
        let id = data["id"]
            .as_str()
            .filter(|id| valid_id(id))
            .context("站点返回的登录请求无效")?;
        let code = data["code"]
            .as_str()
            .filter(|s| s.len() == 6 && s.bytes().all(|b| b.is_ascii_digit()))
            .context("站点未返回登录核对码")?;
        let pending = Pending {
            id: id.into(),
            verifier,
            expires_at: now() + 180_000,
            generation: self.session()?.map(|s| s.generation),
        };
        self.secrets
            .set("browser-login", &serde_json::to_vec(&pending)?)?;
        Ok(
            json!({"state":"pending","url":format!("{}/hub/connect#flow=login&request={id}",self.origin),
            "code":code,"expiresAt":pending.expires_at}),
        )
    }
    pub async fn browser_poll(&self) -> Result<Value> {
        let write = WRITE_GATE.lock().await;
        let Some(bytes) = self.secrets.get("browser-login")? else {
            return Ok(json!({"state":"cancelled"}));
        };
        let pending: Pending =
            serde_json::from_slice(&bytes).context("登录请求已失效，请重新登录")?;
        if now() >= pending.expires_at
            || self.session()?.map(|s| s.generation) != pending.generation
        {
            self.secrets.delete("browser-login")?;
            return Ok(json!({"state":"expired"}));
        }
        // Do not hold the account write gate across a network poll. Cancel/logout/login
        // must be able to invalidate this flow while a remote response is delayed.
        drop(write);
        let (status, value) = self
            .send(
                Method::POST,
                "/auth/hub/complete",
                &[],
                Some(&json!({"id":pending.id,"verifier":pending.verifier,"purpose":"login"})),
                None,
            )
            .await?;
        let _write = WRITE_GATE.lock().await;
        let current = self
            .secrets
            .get("browser-login")?
            .and_then(|bytes| serde_json::from_slice::<Pending>(&bytes).ok());
        if current
            .as_ref()
            .is_none_or(|p| p.id != pending.id || p.verifier != pending.verifier)
            || self.session()?.map(|s| s.generation) != pending.generation
            || now() >= pending.expires_at
        {
            return Ok(json!({"state":"cancelled"}));
        }
        let data = Self::unwrap(status, value)?;
        if data["state"] == "pending" {
            return Ok(json!({"state":"pending"}));
        }
        self.secrets.delete("browser-login")?;
        ensure!(data["state"] == "ready", "登录请求已失效，请重新登录");
        let mut session = Session {
            access_token: data["access_token"].as_str().unwrap_or_default().into(),
            refresh_token: data["refresh_token"].as_str().unwrap_or_default().into(),
            expires_at: now()
                + data["expires_in"]
                    .as_i64()
                    .unwrap_or(0)
                    .clamp(0, 86_400 * 365)
                    * 1000,
            generation: String::new(),
            user_id: 0,
        };
        let _guard = SESSION_GATE.lock().await;
        let user = self.accept_session(&mut session).await?;
        Ok(json!({"state":"ready","user":user}))
    }
    pub async fn browser_cancel(&self) -> Result<Value> {
        let _write = WRITE_GATE.lock().await;
        self.secrets.delete("browser-login")?;
        Ok(json!({"state":"cancelled"}))
    }
    pub async fn website_link(&self) -> Result<Value> {
        let _write = WRITE_GATE.lock().await;
        if self.session()?.is_none() {
            return Ok(json!({"url":format!("{}/login",self.origin),"automatic":false}));
        }
        if self.browser_capability().await?["state"] != "available" {
            return Ok(
                json!({"url":format!("{}/dashboard",self.origin),"automatic":false,
                "notice":"站点尚未启用接续登录，浏览器将使用自己的登录状态"}),
            );
        }
        let verifier = verifier();
        let data = self
            .authenticated(
                Method::POST,
                "/auth/hub/ticket",
                &[],
                Some(&json!({"challenge":challenge(&verifier)})),
            )
            .await?;
        let id = data["id"]
            .as_str()
            .filter(|id| valid_id(id))
            .context("站点返回的接续票据无效")?;
        Ok(
            json!({"url":format!("{}/hub/connect#flow=return&request={id}&verifier={verifier}",self.origin),
            "automatic":true}),
        )
    }
}
fn browser_unsupported() -> Value {
    json!({"state":"unsupported","message":"当前站点尚未启用浏览器登录，请使用内置登录"})
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    struct Fixture {
        _dir: tempfile::TempDir,
        squad: Arc<Squad>,
        approved: Arc<AtomicBool>,
        held: Arc<AtomicBool>,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        completions: Arc<AtomicUsize>,
        capability_mode: Arc<AtomicUsize>,
        starts: Arc<AtomicUsize>,
        server: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }
    async fn fixture() -> Fixture {
        use axum::{Router, response::IntoResponse};
        let approved = Arc::new(AtomicBool::new(false));
        let held = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let completions = Arc::new(AtomicUsize::new(0));
        let capability_mode = Arc::new(AtomicUsize::new(0));
        let starts = Arc::new(AtomicUsize::new(0));
        let proof: Arc<std::sync::Mutex<String>> = Default::default();
        let (a, h, e, r, c, p) = (
            approved.clone(),
            held.clone(),
            entered.clone(),
            release.clone(),
            completions.clone(),
            proof,
        );
        let (mode, starts_in_handler) = (capability_mode.clone(), starts.clone());
        let app = Router::new().fallback(move |req: axum::extract::Request| {
            let (a,h,e,r,c,p,mode,starts) = (a.clone(),h.clone(),e.clone(),r.clone(),c.clone(),p.clone(),mode.clone(),starts_in_handler.clone());
            async move {
                let path = req.uri().path().to_string();
                let bytes = axum::body::to_bytes(req.into_body(), 8192).await.unwrap();
                let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                let data = match path.as_str() {
                    "/api/v1/auth/hub/capabilities" => match mode.load(Ordering::SeqCst) {
                        1 => return (reqwest::StatusCode::NOT_FOUND,"<html>not found</html>").into_response(),
                        2 => return (reqwest::StatusCode::OK,"<html>SPA fallback</html>").into_response(),
                        3 => json!({"version":2}),
                        4 => return (reqwest::StatusCode::SERVICE_UNAVAILABLE,"maintenance").into_response(),
                        _ => json!({"version":1}),
                    },
                    "/api/v1/auth/hub/begin" | "/api/v1/auth/hub/ticket" => {
                        starts.fetch_add(1,Ordering::SeqCst);
                        *p.lock().unwrap() = body["challenge"].as_str().unwrap().into();
                        json!({"id":"a".repeat(64),"code":"123456"})
                    },
                    "/api/v1/auth/hub/complete" => {
                        assert_eq!(challenge(body["verifier"].as_str().unwrap()),*p.lock().unwrap());
                        assert_eq!(body["purpose"],"login");
                        c.fetch_add(1,Ordering::SeqCst);
                        if h.load(Ordering::SeqCst) { e.notify_one(); r.notified().await; }
                        if a.load(Ordering::SeqCst) {
                            json!({"state":"ready","access_token":"fixture-access","refresh_token":"fixture-independent-refresh","expires_in":3600})
                        } else { json!({"state":"pending"}) }
                    },
                    "/api/v1/auth/me" => json!({"id":7,"username":"Synthetic","balance":12.34}),
                    _ => return (reqwest::StatusCode::NOT_FOUND,axum::Json(json!({"code":404}))).into_response(),
                };
                axum::Json(json!({"code":0,"data":data})).into_response()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("squad");
        let squad = Arc::new(
            Squad::synthetic(
                root,
                reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(std::time::Duration::from_secs(5))
                    .build()
                    .unwrap(),
                origin,
            )
            .unwrap(),
        );
        Fixture {
            _dir: dir,
            squad,
            approved,
            held,
            entered,
            release,
            completions,
            capability_mode,
            starts,
            server,
        }
    }
    #[test]
    fn proof_is_s256_and_identifiers_are_bounded() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(valid_id(&"a".repeat(64)));
        assert!(!valid_id(&"a".repeat(65)));
        assert!(!valid_id(&format!("{}&x=1", "a".repeat(60))));
    }
    #[tokio::test]
    async fn optional_browser_route_missing_or_incompatible_never_starts_login() {
        let f = fixture().await;
        for mode in 1..=4 {
            f.capability_mode.store(mode, Ordering::SeqCst);
            let capability = f.squad.browser_capability().await.unwrap();
            assert_eq!(
                capability["state"],
                if mode == 4 {
                    "unavailable"
                } else {
                    "unsupported"
                }
            );
            assert!(capability["message"].as_str().unwrap().contains("内置登录"));
            assert_eq!(f.squad.browser_start().await.unwrap(), capability);
            assert!(f.squad.secrets.get("browser-login").unwrap().is_none());
        }
        assert_eq!(f.starts.load(Ordering::SeqCst), 0);
        assert_eq!(f.completions.load(Ordering::SeqCst), 0);
        f.capability_mode.store(0, Ordering::SeqCst);
        assert_eq!(
            f.squad.browser_capability().await.unwrap()["state"],
            "available"
        );
        assert_eq!(f.squad.browser_start().await.unwrap()["state"], "pending");
        assert_eq!(f.starts.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn browser_authorization_confirms_profile_and_returns_only_one_use_link() {
        let f = fixture().await;
        assert_eq!(f.squad.profile().await.unwrap()["signedIn"], false);
        let start = f.squad.browser_start().await.unwrap();
        assert_eq!(start["code"], "123456");
        assert!(!start.to_string().contains("verifier"));
        assert_eq!(f.squad.browser_poll().await.unwrap()["state"], "pending");
        assert!(f.squad.session().unwrap().is_none());
        f.approved.store(true, Ordering::SeqCst);
        assert_eq!(f.squad.browser_poll().await.unwrap()["state"], "ready");
        let profile = f.squad.profile().await.unwrap();
        assert_eq!(profile["user"]["balance"], 12.34);
        assert!(!profile.to_string().contains("fixture-access"));
        assert_eq!(f.squad.session().unwrap().unwrap().user_id, 7);
        assert_eq!(f.squad.browser_poll().await.unwrap()["state"], "cancelled");
        let link = f.squad.website_link().await.unwrap();
        let url = link["url"].as_str().unwrap();
        assert!(url.contains("#flow=return&request="));
        assert!(!url.contains("fixture-access") && !url.contains("fixture-independent-refresh"));
    }
    #[tokio::test]
    async fn cancellation_invalidates_a_delayed_authorized_reply() {
        let f = fixture().await;
        f.squad.browser_start().await.unwrap();
        f.approved.store(true, Ordering::SeqCst);
        f.held.store(true, Ordering::SeqCst);
        let squad = f.squad.clone();
        let poll = tokio::spawn(async move { squad.browser_poll().await.unwrap() });
        f.entered.notified().await;
        f.squad.browser_cancel().await.unwrap();
        f.release.notify_one();
        assert_eq!(poll.await.unwrap()["state"], "cancelled");
        assert!(f.squad.session().unwrap().is_none());
    }
    #[tokio::test]
    async fn old_generation_and_expiration_never_exchange_or_replace_an_account() {
        let f = fixture().await;
        f.squad.browser_start().await.unwrap();
        let bytes = f.squad.secrets.get("browser-login").unwrap().unwrap();
        let mut pending: Pending = serde_json::from_slice(&bytes).unwrap();
        pending.expires_at = now() - 1;
        f.squad
            .secrets
            .set("browser-login", &serde_json::to_vec(&pending).unwrap())
            .unwrap();
        assert_eq!(f.squad.browser_poll().await.unwrap()["state"], "expired");
        assert_eq!(f.completions.load(Ordering::SeqCst), 0);
        f.squad.browser_start().await.unwrap();
        f.squad
            .login(Session {
                access_token: "fixture-access".into(),
                refresh_token: "other".into(),
                expires_at: 0,
                generation: String::new(),
                user_id: 0,
            })
            .await
            .unwrap();
        let generation = f.squad.session().unwrap().unwrap().generation;
        assert_eq!(f.squad.browser_poll().await.unwrap()["state"], "cancelled");
        assert_eq!(f.squad.session().unwrap().unwrap().generation, generation);
    }
}
