//! Native account API. Browser authorization uses the optional site companion.
use super::transaction;
use crate::nvwa::secrets::SecretStore;
use anyhow::{Context, Result, ensure};
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
mod cache;
mod key_usage;
#[cfg(test)]
mod performance_tests;
#[cfg(test)]
mod assistant_tests {
    use super::*;
    use axum::{
        Router,
        body::Bytes,
        response::IntoResponse,
        routing::{get, post},
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn assistant_raw_transport_keeps_sse_json_and_account_identity_without_balance_calls() {
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = seen.clone();
        let app=Router::new()
            .route("/api/v1/hub/assistant/config",get(|headers:axum::http::HeaderMap|async move{
                assert_eq!(headers["authorization"],"Bearer synthetic-assistant-token");
                axum::Json(json!({"code":0,"data":{"fixture":true}}))}))
            .route("/v1/responses",post(move |headers:axum::http::HeaderMap,body:Bytes|{
                let counter=counter.clone();async move {
                    counter.fetch_add(1,Ordering::SeqCst);
                    assert_eq!(headers["authorization"],"Bearer synthetic-admin-api-key");
                    assert_eq!(headers["x-hub-client"],"genius-buddy");
                    assert!(!String::from_utf8_lossy(&body).contains("synthetic-assistant-token"));
                    let body:Value=serde_json::from_slice(&body).unwrap();
                    if body["stream"]==true {
                        ([("content-type","text/event-stream")],"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n").into_response()
                    } else { axum::Json(json!({"object":"response","output":[]})).into_response() }
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let api: crate::assistant::model::Api = serde_json::from_value(
            json!({"baseUrl":format!("{origin}/v1"),"apiKey":"synthetic-admin-api-key"}),
        )
        .unwrap();
        let squad = Squad::synthetic(
            dir.path().join("squad"),
            Client::builder().no_proxy().build().unwrap(),
            origin,
        )
        .unwrap();
        let expected = squad.assistant_test_identity("one").unwrap();
        assert_eq!(
            squad.assistant_config(&expected).await.unwrap()["fixture"],
            true
        );
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-hub-client", "genius-buddy".parse().unwrap());
        let response = squad
            .assistant_model(
                &expected,
                &api,
                json!({"model":"fixture","stream":true}),
                headers.clone(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        assert!(response.text().await.unwrap().starts_with("data: "));
        let summary = squad
            .assistant_model(
                &expected,
                &api,
                json!({"model":"fixture","input":[],"stream":false}),
                headers,
            )
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        assert_eq!(summary["object"], "response");
        squad.assistant_test_identity("two").unwrap();
        assert!(
            squad
                .assistant_model(&expected, &api, json!({}), Default::default())
                .await
                .is_err()
        );
        assert_eq!(seen.load(Ordering::SeqCst), 2);
        server.abort();
    }
    #[tokio::test]
    async fn assistant_missing_endpoint_is_clear_and_model_failures_are_not_replayed() {
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = seen.clone();
        let app = Router::new()
            .route(
                "/api/v1/hub/assistant/config",
                get(|| async { (StatusCode::NOT_FOUND, "not deployed") }),
            )
            .route(
                "/v1/responses",
                post(move || {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        (
                            StatusCode::SERVICE_UNAVAILABLE,
                            axum::Json(json!({"error":{"message":"synthetic unavailable"}})),
                        )
                            .into_response()
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let api: crate::assistant::model::Api = serde_json::from_value(
            json!({"baseUrl":format!("{origin}/v1"),"apiKey":"synthetic-admin-api-key"}),
        )
        .unwrap();
        let squad = Squad::synthetic(
            dir.path().join("squad"),
            Client::builder().no_proxy().build().unwrap(),
            origin,
        )
        .unwrap();
        let expected = squad.assistant_test_identity("one").unwrap();
        assert!(
            squad
                .assistant_config(&expected)
                .await
                .unwrap_err()
                .to_string()
                .contains("尚未部署")
        );
        assert_eq!(
            squad
                .assistant_model(&expected, &api, json!({}), Default::default())
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        server.abort();
    }
}

/// One service/HTTP connection pool per daemon configuration, replaced on proxy changes.
#[derive(Default)]
pub struct ServiceCache {
    key: Option<(PathBuf, crate::config::OutboundProxyConfig, Option<u16>)>,
    service: Option<Arc<Squad>>,
}
impl ServiceCache {
    pub fn get(&mut self, path: &Path, config: &crate::config::AppConfig) -> Result<Arc<Squad>> {
        let key = (
            path.to_path_buf(),
            config.outbound_proxy.clone(),
            config.local_listen_port(),
        );
        if self.key.as_ref() == Some(&key)
            && let Some(service) = &self.service
        {
            return Ok(service.clone());
        }
        let service = Arc::new(Squad::new(path, config)?);
        self.key = Some(key);
        self.service = Some(service.clone());
        Ok(service)
    }
}

pub const ORIGIN: &str = "https://tiancai.yc99.space";
pub(super) static SESSION_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
pub(super) static WRITE_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    #[serde(default)]
    pub generation: String,
    #[serde(default)]
    pub user_id: i64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedAccount {
    pub user_id: i64,
    pub generation: String,
}
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(super) struct Rejection {
    pub(super) status: StatusCode,
    message: String,
}
pub struct Squad {
    pub(super) root: PathBuf,
    pub(super) secrets: SecretStore,
    pub(super) http: Client,
    pub(super) origin: String,
    cache: Mutex<cache::ReadCache>,
}
impl Squad {
    /// Public account identity without the login JWT or refresh credential.
    pub(crate) fn assistant_identity(&self) -> Result<Option<ExpectedAccount>> {
        Ok(self.session()?.map(|session| ExpectedAccount {
            user_id: session.user_id,
            generation: session.generation,
        }))
    }
    #[cfg(test)]
    pub(crate) fn assistant_test_identity(&self, generation: &str) -> Result<ExpectedAccount> {
        self.save(&Session {
            access_token: "synthetic-assistant-token".into(),
            refresh_token: String::new(),
            expires_at: 0,
            generation: generation.into(),
            user_id: 7,
        })?;
        self.assistant_identity()?.context("测试会话未保存")
    }
    fn assistant_account(&self, expected: &ExpectedAccount) -> Result<Session> {
        let session = self.session()?.context("请先登录天才小分队")?;
        ensure!(
            session.user_id == expected.user_id && session.generation == expected.generation,
            "小分队账户已切换，请重新打开天才搭档"
        );
        Ok(session)
    }
    pub(crate) async fn assistant_config(&self, expected: &ExpectedAccount) -> Result<Value> {
        self.assistant_account(expected)?;
        let result = self
            .authenticated(Method::GET, "/hub/assistant/config", &[], None)
            .await;
        self.assistant_account(expected)?;
        if result
            .as_ref()
            .err()
            .and_then(|e| e.downcast_ref::<Rejection>())
            .is_some_and(|e| e.status == StatusCode::NOT_FOUND)
        {
            anyhow::bail!("小分队尚未部署天才搭档模型接口，请联系管理员完成服务端对接");
        }
        result
    }
    /// The ordinary model API uses its own credential, never the login JWT.
    pub(crate) async fn assistant_model(
        &self,
        expected: &ExpectedAccount,
        api: &crate::assistant::model::Api,
        body: Value,
        protocol_headers: reqwest::header::HeaderMap,
    ) -> Result<reqwest::Response> {
        self.assistant_account(expected)?;
        let response = self
            .http
            .post(api.endpoint())
            .headers(protocol_headers)
            .bearer_auth(&api.api_key)
            .timeout(std::time::Duration::from_secs(1800))
            .json(&body)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("天才搭档模型 API 连接中断，请稍后重试"))?;
        self.assistant_account(expected)?;
        Ok(response)
    }
    pub fn new(path: &Path, config: &crate::config::AppConfig) -> Result<Self> {
        let root = super::state_root(path).join("squad");
        let http = crate::external_import::client::build_client(
            &config.outbound_proxy,
            config.local_listen_port(),
        )
        .map_err(anyhow::Error::msg)?;
        Ok(Self {
            secrets: SecretStore::new(root.join("session"))?,
            root,
            http,
            origin: ORIGIN.into(),
            cache: Default::default(),
        })
    }
    pub(super) fn session(&self) -> Result<Option<Session>> {
        self.secrets
            .get("account-session")?
            .map(|b| serde_json::from_slice(&b).context("账户会话无法读取，请重新登录"))
            .transpose()
    }
    fn save(&self, session: &Session) -> Result<()> {
        self.invalidate();
        self.secrets
            .set("account-session", &serde_json::to_vec(session)?)
    }
    pub fn invalidate(&self) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    async fn public_settings(&self) -> Result<Value> {
        let key = cache::Key::new(None, "/settings/public", &[]);
        let (epoch, hit) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .read(&key, Instant::now());
        if let Some(hit) = hit {
            return Ok(hit);
        }
        let (status, value) = self
            .send(Method::GET, "/settings/public", &[], None, None)
            .await?;
        let value = Self::unwrap(status, value)?;
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(
            key,
            &value,
            epoch,
            Instant::now(),
        );
        Ok(value)
    }
    #[cfg(test)]
    pub(crate) fn synthetic(root: PathBuf, http: Client, origin: String) -> Result<Self> {
        Ok(Self {
            secrets: SecretStore::new(root.join("session"))?,
            root,
            http,
            origin,
            cache: Default::default(),
        })
    }
    pub(super) async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<&Value>,
        token: Option<&str>,
    ) -> Result<(StatusCode, Value)> {
        let import_ticket = method == Method::POST
            && path.starts_with("/keys/")
            && path.ends_with("/external-import");
        let mut request = self
            .http
            .request(method, format!("{}/api/v1{path}", self.origin))
            .header("accept", "application/json")
            .header("accept-language", "zh-CN")
            .header("x-user-ui-request", "true")
            .query(query);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            ensure!(
                serde_json::to_vec(body)?.len() <= 256 * 1024,
                "保存内容过大"
            );
            request = request.json(body);
        }
        // The deployed ticket-creation contract requires its own fixed origin.
        // No caller-supplied origin is forwarded, and redirects remain disabled.
        if import_ticket {
            request = request.header("origin", &self.origin);
        }
        let mut response = request.send().await.map_err(|_| {
            anyhow::anyhow!("天才小分队响应未确认。请重新读取列表或订单，不要重复提交")
        })?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND && path == "/hub/assistant/config" {
            return Err(Rejection {
                status,
                message: "小分队尚未部署天才搭档模型接口".into(),
            }
            .into());
        }
        ensure!(!status.is_redirection(), "站点接口发生跳转，请重新登录");
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("站点响应中断，请重新读取确认结果"))?
        {
            ensure!(
                bytes.len() + chunk.len() <= 8 * 1024 * 1024,
                "站点响应过大，请缩小筛选范围"
            );
            bytes.extend(chunk);
        }
        let data: Value =
            serde_json::from_slice(&bytes).context("站点接口返回格式不兼容，请稍后重试")?;
        Ok((status, data))
    }
    pub(super) fn unwrap(status: StatusCode, value: Value) -> Result<Value> {
        if !(status.is_success() && value["code"].as_i64() == Some(0)) {
            return Err(Rejection {
                status,
                message: value["message"]
                    .as_str()
                    .filter(|v| v.len() <= 600)
                    .unwrap_or(if status == StatusCode::UNAUTHORIZED {
                        "登录已失效，请重新登录"
                    } else if status == StatusCode::FORBIDDEN {
                        "当前账户无此操作权限"
                    } else {
                        "站点操作未完成，请重新读取确认"
                    })
                    .into(),
            }
            .into());
        }
        Ok(value.get("data").cloned().unwrap_or(Value::Null))
    }
    pub async fn login(&self, mut session: Session) -> Result<Value> {
        let _write = WRITE_GATE.lock().await;
        let _guard = SESSION_GATE.lock().await;
        self.secrets.delete("browser-login")?;
        self.accept_session(&mut session).await
    }
    pub(super) async fn accept_session(&self, session: &mut Session) -> Result<Value> {
        ensure!(
            !session.access_token.is_empty()
                && session.access_token.len() <= 16384
                && session.refresh_token.len() <= 16384,
            "登录会话无效，请重新登录"
        );
        let (status, value) = self
            .send(
                Method::GET,
                "/auth/me",
                &[],
                None,
                Some(&session.access_token),
            )
            .await?;
        let user = Self::unwrap(status, value)?;
        ensure!(
            user["id"].as_i64().is_some_and(|id| id > 0),
            "站点账户身份无效"
        );
        session.user_id = user["id"].as_i64().unwrap();
        session.generation = uuid::Uuid::new_v4().to_string();
        self.save(session)?;
        let key = cache::Key::new(Some(session), "/auth/me", &[]);
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let (epoch, _) = cache.read(&key, Instant::now());
        cache.insert(key, &user, epoch, Instant::now());
        Ok(user)
    }
    pub(super) async fn authenticated(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<&Value>,
    ) -> Result<Value> {
        let _guard = SESSION_GATE.lock().await;
        let mut session = self.session()?.context("请先登录天才小分队")?;
        let key = cache::Key::new(Some(&session), path, query);
        let (epoch, hit) = if method == Method::GET {
            self.cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .read(&key, Instant::now())
        } else {
            (0, None)
        };
        if let Some(hit) = hit {
            return Ok(hit);
        }
        let (status, value) = self
            .send(
                method.clone(),
                path,
                query,
                body,
                Some(&session.access_token),
            )
            .await?;
        if status != StatusCode::UNAUTHORIZED {
            let mut value = Self::unwrap(status, value)?;
            if method == Method::GET && path == "/keys" {
                self.enrich_key_usage(&session, &mut value).await;
            }
            if method == Method::GET && value["usageLoadError"].is_null() {
                self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(
                    key,
                    &value,
                    epoch,
                    Instant::now(),
                );
            }
            return Ok(value);
        }
        self.invalidate();
        if session.refresh_token.is_empty() {
            return Err(Rejection {
                status: StatusCode::UNAUTHORIZED,
                message: "登录已失效，请重新登录".into(),
            }
            .into());
        }
        let (status, value) = self
            .send(
                Method::POST,
                "/auth/refresh",
                &[],
                Some(&json!({"refresh_token":session.refresh_token})),
                None,
            )
            .await?;
        let fresh = Self::unwrap(status, value)?;
        session.access_token = fresh["access_token"]
            .as_str()
            .context("站点刷新会话失败，请重新登录")?
            .into();
        session.refresh_token = fresh["refresh_token"]
            .as_str()
            .context("站点刷新会话失败，请重新登录")?
            .into();
        session.expires_at = crate::ai_gateway::request_log::now_ms()
            + fresh["expires_in"].as_i64().unwrap_or(0) * 1000;
        self.save(&session)?;
        // Only an explicit 401 can replay after refresh. Timeouts and 5xx never do.
        let read_keys = method == Method::GET && path == "/keys";
        let (status, value) = self
            .send(method, path, query, body, Some(&session.access_token))
            .await?;
        let mut value = Self::unwrap(status, value)?;
        if read_keys {
            self.enrich_key_usage(&session, &mut value).await;
        }
        Ok(value)
    }
    pub async fn logout(&self) -> Result<Value> {
        let _write = WRITE_GATE.lock().await;
        let _guard = SESSION_GATE.lock().await;
        self.invalidate();
        self.secrets.delete("browser-login")?;
        let old = self.session()?;
        self.secrets.delete("account-session")?;
        let revoked = if let Some(session) = old {
            if session.refresh_token.is_empty() {
                false
            } else {
                self.send(
                    Method::POST,
                    "/auth/logout",
                    &[],
                    Some(&json!({"refresh_token":session.refresh_token})),
                    Some(&session.access_token),
                )
                .await
                .is_ok_and(|(status, value)| status.is_success() && value["code"] == 0)
            }
        } else {
            true
        };
        Ok(json!({"signedOut":true,"serverRevoked":revoked}))
    }
    pub async fn status(&self) -> Result<Value> {
        // Keep user, organization permissions and pending writes from one account generation.
        let _write = WRITE_GATE.lock().await;
        // A signed-out page must open login even if the public API is unavailable.
        if self.session()?.is_none() {
            return Ok(json!({"signedIn":false,"settings":null,"menus":[],"pending":[]}));
        }
        let user = match self.authenticated(Method::GET, "/auth/me", &[], None).await {
            Ok(user) => user,
            Err(error)
                if error
                    .downcast_ref::<Rejection>()
                    .is_some_and(|r| r.status == StatusCode::UNAUTHORIZED) =>
            {
                return Ok(
                    json!({"signedIn":false,"expired":true,"settings":null,"menus":[],"pending":[]}),
                );
            }
            Err(error) => return Err(error),
        };
        let settings = self.public_settings().await?;
        let organization = self
            .authenticated(Method::GET, "/user/organization", &[], None)
            .await;
        let permissions = organization.as_ref().ok().cloned().unwrap_or(Value::Null);
        let menus = menus(&settings, &user, &permissions);
        let money = self.display_money().await;
        let generation = self.session()?.map(|s| s.generation).unwrap_or_default();
        Ok(
            json!({"signedIn":true,"user":user,"accountGeneration":generation,"money":money,"settings":settings,"organization":permissions,"organizationUnknown":organization.is_err(),"menus":menus,"pending":self.pending()?}),
        )
    }
    pub(super) async fn display_money(&self) -> super::squad_money::Money {
        // Presentation never depends on payment metadata or an exchange rate.
        Default::default()
    }
    pub async fn browser_session(&self) -> Result<Value> {
        let _write = WRITE_GATE.lock().await;
        let user = self
            .authenticated(Method::GET, "/auth/me", &[], None)
            .await?;
        let session = self.session()?.context("请重新登录")?;
        Ok(
            json!({"auth_token":session.access_token,"refresh_token":session.refresh_token,"token_expires_at":session.expires_at,"auth_user":user}),
        )
    }
    pub async fn payment_recovery(&self, id: u64) -> Result<Value> {
        ensure!(id > 0, "请选择已有订单");
        let _write = WRITE_GATE.lock().await;
        let current = self
            .authenticated(Method::GET, &format!("/payment/orders/{id}"), &[], None)
            .await?;
        let session = self.session()?.context("请重新登录")?;
        let key = format!("payment-{}-{id}", session.user_id);
        let mut recovery = self
            .secrets
            .get(&key)?
            .map(|b| serde_json::from_slice::<Value>(&b))
            .transpose()?
            .unwrap_or_else(|| current.clone());
        for (k, v) in current.as_object().into_iter().flatten() {
            recovery[k] = v.clone();
        }
        recovery["order_id"] = id.into();
        Ok(recovery)
    }
    pub async fn download(&self, path: &str, destination: &Path) -> Result<Value> {
        ensure!(download_allowed(path), "不支持此下载地址");
        ensure!(destination.is_absolute(), "请选择完整的保存位置");
        let before = transaction::read(destination)?;
        let _write = WRITE_GATE.lock().await;
        let mut response = None;
        for attempt in 0..2 {
            let session = self.session()?.context("请先登录")?;
            let result = self
                .http
                .get(format!("{}/api/v1{path}", self.origin))
                .bearer_auth(&session.access_token)
                .send()
                .await
                .context("下载未完成，请重试")?;
            if result.status() == StatusCode::UNAUTHORIZED && attempt == 0 {
                // A cached profile must never suppress the refresh after an explicit 401.
                self.invalidate();
                self.authenticated(Method::GET, "/auth/me", &[], None)
                    .await?;
                continue;
            }
            ensure!(
                result.status().is_success(),
                "下载未完成，可能没有查看此文件的权限，请重新读取"
            );
            response = Some(result);
            break;
        }
        let mut response = response.context("请重新登录后下载")?;
        let max = transaction::MAX_FILE_BYTES;
        ensure!(
            response.content_length().is_none_or(|v| v <= max as u64),
            "文件超过本次下载上限，请到官网保存"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= max,
                "文件超过本次下载上限，请到官网保存"
            );
            bytes.extend_from_slice(&chunk);
        }
        ensure!(!bytes.is_empty(), "站点返回了空文件");
        let change = transaction::FileChange::plan(
            destination.to_path_buf(),
            before.as_deref(),
            Some(bytes),
        );
        let store = transaction::TransactionStore::new(self.root.join("downloads"))?;
        tokio::task::spawn_blocking(move || store.apply(vec![change])).await??;
        Ok(json!({"saved":true}))
    }
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        query: Vec<(String, String)>,
        body: Option<Value>,
    ) -> Result<Value> {
        self.request_checked(method, path, query, body, None).await
    }
    pub async fn request_checked(
        &self,
        method: &str,
        path: &str,
        query: Vec<(String, String)>,
        body: Option<Value>,
        expected: Option<&ExpectedAccount>,
    ) -> Result<Value> {
        ensure!(allowed(method, path), "不支持此账户操作");
        ensure!(
            query.len() <= 48
                && query.iter().all(|(k, v)| k.len() <= 100
                    && v.len() <= 2000
                    && !k.chars().any(char::is_control)),
            "筛选参数无效"
        );
        let method = Method::from_bytes(method.as_bytes())?;
        let mutation = method != Method::GET;
        let write = requires_confirmation(method.as_str(), path);
        let _write = if method != Method::GET {
            Some(WRITE_GATE.lock().await)
        } else {
            None
        };
        if let Some(expected) = expected {
            ensure!(
                self.session()?.is_some_and(
                    |s| s.user_id == expected.user_id && s.generation == expected.generation
                ),
                "账户已切换，请刷新后重新确认操作"
            );
        }
        if mutation {
            self.invalidate();
        }
        let id = uuid::Uuid::new_v4().to_string();
        if write {
            ensure!(
                !self
                    .pending()?
                    .iter()
                    .any(|p| p["path"] == path && p["method"] == method.as_str()),
                "此操作上次的结果尚未确认，请先在“待确认操作”中核对结果，避免重复提交"
            );
            self.mark_pending(&id, path, method.as_str())?;
        }
        let result = if path == "/settings/public" && self.session()?.is_none() {
            let (status, value) = self.send(method, path, &query, body.as_ref(), None).await?;
            Self::unwrap(status, value)
        } else {
            self.authenticated(method, path, &query, body.as_ref())
                .await
        };
        if path == "/payment/orders" {
            if let Ok(order) = &result {
                if let Some(order_id) = order["order_id"].as_u64() {
                    let session = self.session()?.context("请重新登录")?;
                    let mut saved = order.clone();
                    for key in ["order_type", "payment_type"] {
                        if saved[key].is_null() {
                            saved[key] =
                                body.as_ref().map(|b| b[key].clone()).unwrap_or(Value::Null);
                        }
                    }
                    self.secrets.set(
                        &format!("payment-{}-{order_id}", session.user_id),
                        &serde_json::to_vec(&saved)?,
                    )?;
                }
            }
        }
        let rejected = result
            .as_ref()
            .err()
            .and_then(|e| e.downcast_ref::<Rejection>())
            .is_some_and(|e| {
                (e.status.is_client_error() && e.status != StatusCode::REQUEST_TIMEOUT)
                    || e.status.is_success()
            });
        if (result.is_ok() || rejected) && write {
            let _ = self.clear_pending(&id);
        }
        if mutation {
            self.invalidate();
        }
        result
    }
    pub async fn prepare_import(&self, id: u64) -> Result<crate::external_import::ImportDraft> {
        ensure!(id > 0, "请选择密钥");
        let before = self.session()?.context("请先登录天才小分队")?;
        let same_account = || -> Result<()> {
            ensure!(
                self.session()?.is_some_and(
                    |s| s.user_id == before.user_id && s.generation == before.generation
                ),
                "账户已变化，请重新选择密钥导入"
            );
            Ok(())
        };
        let ticket = self
            .request(
                "POST",
                &format!("/keys/{id}/external-import"),
                vec![],
                Some(json!({"target":"tiancaispace-hub","schema_version":1})),
            )
            .await?;
        same_account()?;
        let link = crate::external_import::ImportLink::parse(
            ticket["deeplink"]
                .as_str()
                .context("站点未返回导入链接，请重新选择密钥导入")?,
        )
        .map_err(anyhow::Error::msg)?;
        ensure!(link.origin == self.origin, "导入来源与天才小分队不一致");
        // Issue and redeem exactly once; all three steps share the same connection pool.
        let mut draft = crate::external_import::client::resolve(&self.http, &link)
            .await
            .map_err(anyhow::Error::msg)?;
        same_account()?;
        ensure!(
            draft.source.key_id == id.to_string(),
            "导入密钥与所选密钥不一致，请重新选择密钥导入"
        );
        crate::external_import::client::fetch_models(&self.http, &mut draft)
            .await
            .map_err(anyhow::Error::msg)?;
        same_account()?;
        Ok(draft)
    }
    fn pending(&self) -> Result<Vec<Value>> {
        let Some(session) = self.session()? else {
            return Ok(vec![]);
        };
        let dir = self.root.join("pending");
        if !dir.is_dir() {
            return Ok(vec![]);
        }
        let mut result = Vec::new();
        for entry in std::fs::read_dir(dir)?.take(128) {
            let entry = entry?;
            if entry.path().extension().is_some_and(|s| s == "json") {
                if let Some(bytes) = transaction::read(&entry.path())? {
                    let value: Value = serde_json::from_slice(&bytes)?;
                    if value["accountId"] == session.user_id {
                        result.push(value);
                    }
                }
            }
        }
        Ok(result)
    }
    fn mark_pending(&self, id: &str, path: &str, method: &str) -> Result<()> {
        let session = self.session()?.context("请先登录")?;
        transaction::atomic_write(
            &self.root.join("pending").join(format!("{id}.json")),
            &serde_json::to_vec(
                &json!({"id":id,"path":path,"method":method,"accountId":session.user_id,"at":crate::ai_gateway::request_log::now_ms()}),
            )?,
        )
    }
    pub fn clear_pending(&self, id: &str) -> Result<()> {
        ensure!(uuid::Uuid::parse_str(id).is_ok(), "操作身份无效");
        let path = self.root.join("pending").join(format!("{id}.json"));
        transaction::reject_links(&path)?;
        if let Some(raw) = transaction::read(&path)? {
            let value: Value = serde_json::from_slice(&raw)?;
            ensure!(
                self.session()?
                    .is_some_and(|s| value["accountId"] == s.user_id),
                "待确认操作不属于当前账户"
            );
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}
pub fn menus(settings: &Value, user: &Value, organization: &Value) -> Vec<Value> {
    let mut out = vec![
        json!({"id":"account","label":"我的账户"}),
        json!({"id":"keys","label":"API 密钥"}),
    ];
    if user["run_mode"] != "simple" {
        out.push(json!({"id":"usage","label":"使用记录"}));
        for (id, label, fields) in [
            (
                "organization",
                "组织与扣费",
                vec!["can_view_organization_billing"],
            ),
            (
                "organization-admin",
                "组织管理",
                vec!["can_manage_organization", "can_manage_department"],
            ),
            (
                "organization-usage",
                "组织用量",
                vec!["can_manage_organization", "can_manage_department"],
            ),
        ] {
            if fields.iter().any(|f| organization[*f] == true) {
                out.push(json!({"id":id,"label":label}));
            }
        }
        if settings["available_channels_enabled"] == true {
            out.push(json!({"id":"channels","label":"可用渠道"}));
        }
        if settings["payment_enabled"] != false {
            out.push(json!({"id":"purchase","label":if settings["subscription_enabled"]==false{"充值"}else if settings["payment_balance_disabled"]==true{"订阅"}else{"充值 / 订阅"}}));
        }
    }
    out.push(json!({"id":"profile","label":"账户设置"}));
    out
}
pub fn allowed(method: &str, path: &str) -> bool {
    if !matches!(method, "GET" | "POST" | "PUT" | "PATCH" | "DELETE")
        || !path.starts_with('/')
        || path.contains(['?', '#', '%', '\\'])
        || path.split('/').any(|v| v == "." || v == "..")
    {
        return false;
    }
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    if parts.iter().any(|p| p.is_empty() || p.len() > 128) {
        return false;
    }
    match parts.as_slice() {
        ["settings", "public"] => method == "GET",
        ["keys"] => matches!(method, "GET" | "POST"),
        ["keys", id] => positive(id) && matches!(method, "GET" | "PUT" | "DELETE"),
        ["keys", id, "external-import"] => positive(id) && method == "POST",
        ["groups", "available" | "rates"] | ["channels", "available"] => method == "GET",
        ["usage", ..] | ["subscriptions", ..] => {
            method == "GET" || (path == "/usage/dashboard/api-keys-usage" && method == "POST")
        }
        ["user"] => method == "PUT",
        [
            "user",
            "profile" | "aff" | "organization" | "platform-quotas" | "settlement-notices",
        ] => method == "GET",
        ["user", "password"] => method == "PUT",
        ["user", "aff", "transfer"] | ["user", "settlement-notices", "read"] => method == "POST",
        [
            "user",
            "organization",
            "active-membership"
            | "personal-fallback"
            | "funding-priorities"
            | "department-funding-priorities",
        ] => method == "PUT",
        ["user", "notify-email", "send-code" | "verify"] => method == "POST",
        ["user", "notify-email", "toggle"] => method == "PUT",
        ["user", "notify-email"] => method == "DELETE",
        ["user", "totp", "status" | "verification-method"] => method == "GET",
        ["user", "passkeys"] => method == "GET",
        ["user", "passkeys", id] => positive(id) && matches!(method, "DELETE" | "PATCH"),
        ["user", "api-keys", id, "usage", "daily"] => positive(id) && method == "GET",
        ["redeem"] => method == "POST",
        ["redeem", "history"] => method == "GET",
        ["payment", "config" | "plans" | "checkout-info" | "limits"] => method == "GET",
        ["payment", "orders"] => method == "POST",
        ["payment", "orders", "my" | "refund-eligible-providers"] => method == "GET",
        ["payment", "orders", "verify"] => method == "POST",
        ["payment", "orders", id] => positive(id) && method == "GET",
        ["payment", "orders", id, "cancel" | "refund-request"] => positive(id) && method == "POST",
        ["organization-admin", "business-units"] => method == "GET",
        ["organization-admin", "business-units", id, tail @ ..] if positive(id) => {
            organization_allowed(method, tail)
        }
        ["organization-audit", "logs"] => method == "GET",
        ["organization-audit", "logs", id] => method == "GET" && positive(id),
        ["invoices"] => matches!(method, "GET" | "POST"),
        ["invoices", "overview"] => method == "GET",
        ["invoices", "orders" | "deliveries", id] => method == "GET" && positive(id),
        ["invoices", id] => positive(id) && matches!(method, "GET" | "PATCH" | "DELETE"),
        _ => false,
    }
}
/// A read target for an uncertain write. Never replay the original mutation.
pub fn result_path(path: &str) -> Option<String> {
    let p = path.trim_matches('/').split('/').collect::<Vec<_>>();
    let out = match p.as_slice() {
        ["keys", ..] => "/keys".into(),
        ["payment", ..] => "/payment/orders/my".into(),
        ["redeem", ..] => "/redeem/history".into(),
        ["invoices", id] if positive(id) => path.into(),
        ["invoices", ..] => "/invoices".into(),
        ["user", "organization", ..] => "/user/organization".into(),
        ["user", "aff", ..] => "/user/aff".into(),
        ["user", ..] => "/user/profile".into(),
        ["organization-admin", "business-units", id, tail @ ..] if positive(id) => {
            let prefix = format!("/organization-admin/business-units/{id}");
            let suffix = match *tail {
                [
                    "members",
                    user,
                    "limits" | "balance" | "menu-access" | "billing-context",
                ] if positive(user) => tail.join("/"),
                ["members", user, "limits", "reset"] if positive(user) => {
                    format!("members/{user}/limits")
                }
                ["members", ..] | ["memberships", ..] => "memberships".into(),
                ["departments", dep, "budget-policy", ..] if positive(dep) => {
                    format!("departments/{dep}/budget-policy")
                }
                ["departments", dep, "groups"] if positive(dep) => {
                    "department-group-authorizations".into()
                }
                ["departments", ..] => "departments".into(),
                ["department-admins", ..] => "department-admins".into(),
                ["settlement", "prepayment", ..] => "settlement/prepayment".into(),
                ["settlement", ..] => "settlement".into(),
                ["budget-policy", ..] => "budget-policy".into(),
                [one]
                    if matches!(
                        one,
                        "budget-mode"
                            | "usage-report"
                            | "budget-alert-settings"
                            | "allocation-policy"
                    ) =>
                {
                    one.into()
                }
                _ => return None,
            };
            format!("{prefix}/{suffix}")
        }
        _ => return None,
    };
    allowed("GET", &out).then_some(out)
}
fn requires_confirmation(method: &str, path: &str) -> bool {
    method != "GET"
        && path != "/payment/orders/verify"
        && path != "/usage/dashboard/api-keys-usage"
        && !path.ends_with("/usage-report/preview")
}
fn organization_allowed(method: &str, parts: &[&str]) -> bool {
    match parts {
        [
            "groups"
            | "department-group-authorizations"
            | "memberships"
            | "member-candidates"
            | "admins"
            | "department-admins"
            | "member-limit-summary"
            | "member-limit-details",
        ] if method == "GET" => true,
        ["groups", id, "accounts"] => positive(id) && method == "GET",
        ["departments"] => matches!(method, "GET" | "POST"),
        ["departments", "reorder"] => method == "PUT",
        ["departments", id] => positive(id) && method == "PUT",
        ["departments", id, "groups"] => positive(id) && method == "PUT",
        ["departments", id, "budget-policy"] => positive(id) && matches!(method, "GET" | "PUT"),
        ["departments", id, "budget-policy", "reset"] => positive(id) && method == "POST",
        ["memberships"] | ["memberships", "batch"] | ["department-admins"] => method == "POST",
        ["memberships", id] => positive(id) && method == "DELETE",
        ["department-admins", department, user] => {
            positive(department) && positive(user) && method == "DELETE"
        }
        [
            "budget-policy"
            | "budget-mode"
            | "usage-report"
            | "budget-alert-settings"
            | "allocation-policy",
        ] => matches!(method, "GET" | "PUT"),
        ["budget-policy", "reset"] | ["usage-report", "preview"] => method == "POST",
        ["settlement"] | ["settlement", "prepayment"] => method == "GET",
        ["settlement", "policy"] => method == "PUT",
        ["settlement", "backfill"] | ["settlement", "prepayment", "repay" | "adjust"] => {
            method == "POST"
        }
        ["settlement", "statements", id] | ["settlement", "statements", id, "pdf"] => {
            positive(id) && method == "GET"
        }
        ["members", "limits", "batch"] | ["members", "menu-access", "batch"] => method == "PUT",
        ["members", "limits", "reset-batch"] | ["members", "balance", "adjust-batch"] => {
            method == "POST"
        }
        [
            "members",
            id,
            "limits" | "balance" | "menu-access" | "billing-context",
        ] => positive(id) && matches!(method, "GET" | "PUT"),
        ["members", id, "limits", "reset"] | ["members", id, "balance", "adjust"] => {
            positive(id) && method == "POST"
        }
        ["members", id, "profile"] => positive(id) && method == "PATCH",
        ["members", id, "billing-context-lock"] => positive(id) && method == "DELETE",
        ["usage", "summary" | "breakdown" | "members" | "details"] => method == "GET",
        _ => false,
    }
}
fn positive(value: &str) -> bool {
    value.parse::<u64>().is_ok_and(|v| v > 0)
}
fn download_allowed(path: &str) -> bool {
    if !allowed("GET", path) {
        return false;
    }
    let parts = path.trim_matches('/').split('/').collect::<Vec<_>>();
    matches!(parts.as_slice(),["invoices","deliveries",id]if positive(id))
        || matches!(parts.as_slice(),["organization-admin","business-units",id,"settlement","statements",statement,"pdf"]if positive(id)&&positive(statement))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    type Calls = Arc<Mutex<Vec<(String, String, String)>>>;
    #[test]
    fn uncertain_mutations_recover_in_the_exact_business_scope() {
        for (write, read) in [
            (
                "/organization-admin/business-units/7/memberships/9",
                "/organization-admin/business-units/7/memberships",
            ),
            (
                "/organization-admin/business-units/7/members/9/limits/reset",
                "/organization-admin/business-units/7/members/9/limits",
            ),
            (
                "/organization-admin/business-units/7/settlement/prepayment/repay",
                "/organization-admin/business-units/7/settlement/prepayment",
            ),
            (
                "/organization-admin/business-units/7/departments/3/groups",
                "/organization-admin/business-units/7/department-group-authorizations",
            ),
            ("/invoices/11", "/invoices/11"),
            ("/redeem", "/redeem/history"),
        ] {
            assert_eq!(result_path(write).as_deref(), Some(read));
            assert!(allowed("GET", read));
        }
        assert!(result_path("/admin/users").is_none());
        assert!(result_path("/organization-admin/business-units/7/unknown").is_none());
    }
    struct Fixture {
        _dir: tempfile::TempDir,
        squad: Squad,
        calls: Calls,
        server: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.abort();
        }
    }
    async fn fixture() -> Fixture {
        use axum::{Router, response::IntoResponse};
        let calls: Calls = Default::default();
        let seen = calls.clone();
        let app = Router::new().fallback(move |request: axum::extract::Request| {
            let seen = seen.clone();
            async move {
                let path = request.uri().path().to_string();
                let method = request.method().as_str().to_string();
                let auth = request.headers().get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                seen.lock().unwrap().push((method.clone(),path.clone(),auth.clone()));
                if path == "/api/v1/auth/refresh" {
                    return axum::Json(json!({"code":0,"data":{"access_token":"fresh","refresh_token":"refresh-next","expires_in":3600}})).into_response();
                }
                if path == "/api/v1/settings/public" { return axum::Json(json!({"code":0,"data":{}})).into_response(); }
                if auth == "Bearer expired" {
                    return (StatusCode::UNAUTHORIZED, axum::Json(json!({"code":401,"message":"fixture expired"}))).into_response();
                }
                if path=="/api/v1/redeem" && method=="POST" {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                if method == "POST" {
                    return (StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({"code":503,"message":"fixture result unconfirmed"}))).into_response();
                }
                let data = match path.as_str() {
                    "/api/v1/auth/me" => json!({"id":1,"username":"fixture"}),
                    "/api/v1/keys" => json!({"items":[]}),
                    "/api/v1/payment/orders/9" => json!({"id":9,"status":"pending"}),
                    _ => json!({}),
                };
                axum::Json(json!({"code":0,"data":data})).into_response()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("squad");
        let squad = Squad {
            secrets: SecretStore::new(root.join("session")).unwrap(),
            root,
            http: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(3))
                .build()
                .unwrap(),
            origin,
            cache: Default::default(),
        };
        squad
            .save(&Session {
                access_token: "expired".into(),
                refresh_token: "refresh".into(),
                expires_at: 0,
                generation: "fixture-one".into(),
                user_id: 1,
            })
            .unwrap();
        Fixture {
            _dir: dir,
            squad,
            calls,
            server,
        }
    }
    #[tokio::test]
    async fn signed_out_status_opens_login_without_waiting_for_public_api() {
        let f = fixture().await;
        f.squad.secrets.delete("account-session").unwrap();
        let status = f.squad.status().await.unwrap();
        assert_eq!(status["signedIn"], false);
        assert_eq!(status["menus"], json!([]));
        assert!(f.calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn explicit_401_refreshes_once_and_missing_refresh_expires_cleanly() {
        let f = fixture().await;
        assert!(f.squad.request("GET", "/keys", vec![], None).await.is_ok());
        let calls = f.calls.lock().unwrap().clone();
        assert_eq!(
            calls
                .iter()
                .filter(|(_, p, _)| p.ends_with("/auth/refresh"))
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|(_, p, _)| p.ends_with("/keys"))
                .count(),
            2
        );
        let mut session = f.squad.session().unwrap().unwrap();
        session.access_token = "expired".into();
        session.refresh_token.clear();
        f.squad.save(&session).unwrap();
        let status = f.squad.status().await.unwrap();
        assert_eq!(status["signedIn"], false);
        assert_eq!(status["expired"], true);
        assert_eq!(
            f.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, p, _)| p.ends_with("/auth/refresh"))
                .count(),
            1
        );
    }
    #[tokio::test]
    async fn uncertain_write_never_replays_and_pending_and_orders_are_account_scoped() {
        let f = fixture().await;
        let mut session = f.squad.session().unwrap().unwrap();
        session.access_token = "fresh".into();
        f.squad.save(&session).unwrap();
        assert!(
            f.squad
                .request("POST", "/keys", vec![], Some(json!({"name":"fixture"})))
                .await
                .is_err()
        );
        let pending = f.squad.pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(
            f.squad
                .request("POST", "/keys", vec![], Some(json!({"name":"fixture"})))
                .await
                .is_err()
        );
        assert_eq!(
            f.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(m, p, _)| m == "POST" && p.ends_with("/keys"))
                .count(),
            1
        );
        f.squad
            .secrets
            .set(
                "payment-1-9",
                br#"{"order_id":9,"resume_token":"fixture-secret"}"#,
            )
            .unwrap();
        assert_eq!(
            f.squad.payment_recovery(9).await.unwrap()["resume_token"],
            "fixture-secret"
        );
        session.user_id = 2;
        session.generation = "fixture-two".into();
        f.squad.save(&session).unwrap();
        assert!(f.squad.pending().unwrap().is_empty());
        assert!(
            f.squad
                .clear_pending(pending[0]["id"].as_str().unwrap())
                .is_err()
        );
        assert!(f.squad.payment_recovery(9).await.unwrap()["resume_token"].is_null());
        for _ in 0..2 {
            assert!(
                f.squad
                    .request(
                        "POST",
                        "/payment/orders/verify",
                        vec![],
                        Some(json!({"order_id":9}))
                    )
                    .await
                    .is_err()
            );
        }
        assert!(f.squad.pending().unwrap().is_empty());
    }
    #[tokio::test]
    async fn timed_out_write_is_not_replayed_and_requires_read_confirmation() {
        let mut f = fixture().await;
        let mut session = f.squad.session().unwrap().unwrap();
        session.access_token = "fresh".into();
        f.squad.save(&session).unwrap();
        f.squad.http = Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_millis(150))
            .build()
            .unwrap();
        for _ in 0..2 {
            assert!(
                f.squad
                    .request("POST", "/redeem", vec![], Some(json!({"code":"synthetic"})))
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            f.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(m, p, _)| m == "POST" && p.ends_with("/redeem"))
                .count(),
            1
        );
        let pending = f.squad.pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(
            result_path(pending[0]["path"].as_str().unwrap()).as_deref(),
            Some("/redeem/history")
        );
        assert!(
            f.squad
                .request("GET", "/redeem/history", vec![], None)
                .await
                .is_ok()
        );
        f.squad
            .clear_pending(pending[0]["id"].as_str().unwrap())
            .unwrap();
        assert!(f.squad.pending().unwrap().is_empty());
    }
    #[test]
    fn account_routes_cannot_escape_origin_or_enter_admin() {
        for path in [
            "https://example.invalid/keys",
            "/../admin",
            "//admin",
            "/keys/1%2f..",
            "/admin/users",
            "/auth/me?token=test",
        ] {
            assert!(!allowed("GET", path));
        }
        assert!(allowed("POST", "/keys/12/external-import"));
        assert!(!allowed("DELETE", "/keys/no"));
        assert!(!allowed("POST", "/payment/orders/12"));
        assert!(!allowed("GET", "/model-plaza"));
    }
    #[test]
    fn organization_menus_require_each_server_permission_and_simple_mode() {
        let settings = json!({"model_plaza_enabled":true,"payment_enabled":false});
        let menus = menus(
            &settings,
            &json!({"role":"admin"}),
            &json!({"can_view_organization_billing":true}),
        );
        assert!(menus.iter().any(|m| m["id"] == "organization"));
        assert!(!menus.iter().any(|m| m["id"] == "models"));
        assert!(
            !menus
                .iter()
                .any(|m| m["id"] == "organization-audit" || m["id"] == "purchase")
        );
        assert_eq!(
            super::menus(
                &settings,
                &json!({"run_mode":"simple"}),
                &json!({"can_view_organization_billing":true})
            )
            .len(),
            3
        );
    }
}
