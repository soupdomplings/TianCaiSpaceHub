//! User-authorized WorkBuddy requests, independent of the desktop host lifetime.
//! Only this module and the OS-protected vault hold account credentials.
use super::BuiltinBinding;
use crate::{ai_gateway::error::GatewayError, nvwa::secrets::SecretStore};
use anyhow::{Result, ensure};
use reqwest::{
    Client, Request, Response, StatusCode,
    header::{HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{Mutex, watch},
    time::Instant,
};

mod account;

const VAULT_KEY: &str = "workbuddy-builtin:authorization:v1";
const REAUTHORIZE: &str = "WorkBuddy 授权已失效，请重新启用分享并完成一次授权";
const CANCELLED: &str = "WorkBuddy 分享已停用或账户已变化，本次请求已停止";

// Deliberately no Debug: these values must never enter status/config/log output.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Authorization {
    pub account: Account,
    pub auth: Auth,
    pub gateway_endpoint: String,
    pub auth_endpoint: String,
    #[serde(default)]
    pub auth_prefix_path: String,
    pub auth_type: String,
    pub app_version: String,
    #[serde(default)]
    pub cli_version: Option<String>,
    #[serde(default)]
    pub deployment_type: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    // Retain JSON scalar types: V28 hashes the native session's original types.
    pub uid: Value,
    #[serde(default)]
    pub enterprise_id: Option<Value>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Auth {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub refresh_expires_at: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    authorization: Authorization,
    metadata: Value,
}

#[derive(Default)]
struct State {
    stored: Option<Stored>,
    revision: u64,
    lifecycle: u64,
    blocked: bool,
    last_error: Option<String>,
    account_credits: Option<account::CreditsCache>,
}

pub(crate) struct Runtime {
    secrets: SecretStore,
    client: Client,
    state: Mutex<State>,
    refresh_gate: Mutex<()>,
    account_gate: Mutex<()>,
    changed: watch::Sender<u64>,
    conversation_id: String,
    conversation_request_id: String,
    acp_connection_id: String,
}

impl Authorization {
    fn account_id(&self) -> String {
        let identity = json!([
            self.account.uid,
            self.account.enterprise_id.as_ref().unwrap_or(&json!("")),
            self.auth.domain.as_deref().unwrap_or("")
        ]);
        hex::encode(Sha256::digest(identity.to_string().as_bytes()))
    }

    fn validate(&self) -> Result<()> {
        ensure!(valid_identity(&self.account.uid), "WorkBuddy 授权账户无效");
        ensure!(
            self.account
                .enterprise_id
                .as_ref()
                .is_none_or(|v| valid_identity(v) || v.as_str() == Some("")),
            "WorkBuddy 授权组织无效"
        );
        ensure!(
            !self.auth.access_token.is_empty() && self.auth.access_token.len() <= 32 * 1024,
            "WorkBuddy 授权内容无效"
        );
        sensitive(&format!("Bearer {}", self.auth.access_token))
            .map_err(|_| anyhow::anyhow!("WorkBuddy 授权内容无效"))?;
        if let Some(token) = &self.auth.refresh_token {
            ensure!(
                !token.is_empty() && token.len() <= 32 * 1024,
                "WorkBuddy 续期授权无效"
            );
            sensitive(token).map_err(|_| anyhow::anyhow!("WorkBuddy 续期授权无效"))?;
        }
        endpoint(&self.gateway_endpoint)?;
        endpoint(&self.auth_endpoint)?;
        ensure!(
            self.auth_prefix_path.is_empty()
                || (self.auth_prefix_path.starts_with('/')
                    && !self.auth_prefix_path.contains("..")
                    && self
                        .auth_prefix_path
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"/-_".contains(&c))),
            "WorkBuddy 认证路径不兼容"
        );
        ensure!(
            matches!(
                self.auth_type.as_str(),
                "account-scoped-external-link"
                    | "cli-external-link"
                    | "external-link"
                    | "external-link-v2"
            ),
            "WorkBuddy 认证方式暂不兼容独立分享"
        );
        for value in [
            Some(self.app_version.as_str()),
            self.cli_version.as_deref(),
            self.deployment_type.as_deref(),
            self.auth.domain.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            ensure!(
                value.len() <= 512 && HeaderValue::from_str(value).is_ok(),
                "WorkBuddy 请求上下文无效"
            );
        }
        ensure!(!self.app_version.is_empty(), "WorkBuddy 版本信息缺失");
        Ok(())
    }

    fn chat_url(&self) -> Result<url::Url> {
        let mut url = endpoint(&self.gateway_endpoint)?;
        let base = url.path().trim_end_matches('/');
        let path = if base.ends_with("/v2/chat/completions") {
            base.to_owned()
        } else if base.ends_with("/v2") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v2/chat/completions")
        };
        url.set_path(&path);
        Ok(url)
    }

    fn auth_url(&self, suffix: &str) -> Result<url::Url> {
        let mut url = endpoint(&self.auth_endpoint)?;
        let base = url.path().trim_end_matches('/').trim_end_matches("/v2");
        let path = format!(
            "{base}/v2{}{suffix}",
            self.auth_prefix_path.trim_end_matches('/')
        );
        url.set_path(&path);
        Ok(url)
    }

    fn headers(&self, access_token: &str) -> Result<HeaderMap, GatewayError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            sensitive(&format!("Bearer {access_token}"))?,
        );
        for (key, value) in [
            ("content-type", "application/json".to_owned()),
            ("accept", "application/json".to_owned()),
            ("accept-encoding", "identity".to_owned()),
            ("x-requested-with", "XMLHttpRequest".to_owned()),
            ("x-user-id", scalar(&self.account.uid)),
            ("x-ide-type", "WorkBuddy".to_owned()),
            ("x-ide-name", "WorkBuddy".to_owned()),
            ("x-ide-version", self.app_version.clone()),
            (
                "x-product",
                self.deployment_type
                    .clone()
                    .unwrap_or_else(|| "SaaS".into()),
            ),
            ("x-private-data", "true".to_owned()),
            ("x-agent-intent", "craft".to_owned()),
            ("x-agent-purpose", "conversation".to_owned()),
            ("x-codebuddy-request", "1".to_owned()),
        ] {
            insert(&mut headers, key, &value)?;
        }
        if let Some(enterprise) = self
            .account
            .enterprise_id
            .as_ref()
            .map(scalar)
            .filter(|s| !s.is_empty())
        {
            insert(&mut headers, "x-enterprise-id", &enterprise)?;
            insert(&mut headers, "x-tenant-id", &enterprise)?;
        }
        if let Some(domain) = &self.auth.domain {
            insert(&mut headers, "x-domain", domain)?;
        }
        let mut agent = format!(
            "WorkBuddy/{} WorkBuddy/{}",
            self.app_version, self.app_version
        );
        if let Some(version) = &self.cli_version {
            agent.push_str(&format!(" CLI/{version}"));
        }
        insert(&mut headers, "user-agent", &agent)?;
        Ok(headers)
    }
}

impl Runtime {
    pub(crate) fn new(config_path: &Path) -> Result<Self> {
        let root = super::super::state_root(config_path).join("workbuddy-direct");
        super::super::transaction::reject_links(&root)?;
        let (changed, _) = watch::channel(0);
        Ok(Self {
            secrets: SecretStore::new(root.join("protected"))?,
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .build()
                .map_err(|_| anyhow::anyhow!("无法建立 WorkBuddy 独立分享连接"))?,
            state: Mutex::new(State::default()),
            refresh_gate: Mutex::new(()),
            account_gate: Mutex::new(()),
            changed,
            conversation_id: format!("tiancai-hub-{}", uuid::Uuid::new_v4()),
            conversation_request_id: uuid::Uuid::new_v4().to_string(),
            acp_connection_id: uuid::Uuid::new_v4().to_string(),
        })
    }

    pub(crate) async fn load(&self) -> Result<Option<Value>> {
        let mut state = self.state.lock().await;
        if state.stored.is_none() {
            let Some(bytes) = self
                .secrets
                .get(VAULT_KEY)
                .map_err(|_| anyhow::anyhow!("无法读取受保护的 WorkBuddy 授权，请重新授权"))?
            else {
                return Ok(None);
            };
            let stored: Stored = serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("WorkBuddy 授权记录不兼容，请重新授权"))?;
            ensure!(stored.version == 1, "WorkBuddy 授权记录不兼容，请重新授权");
            stored.authorization.validate()?;
            public_metadata(&stored.authorization, stored.metadata.clone())?;
            state.stored = Some(stored);
            state.revision += 1;
        }
        Ok(state.stored.as_ref().map(|s| s.metadata.clone()))
    }

    pub(crate) async fn capture(
        &self,
        authorization: Authorization,
        metadata: Value,
    ) -> Result<()> {
        authorization.validate()?;
        let metadata = public_metadata(&authorization, metadata)?;
        let mut state = self.state.lock().await;
        let same_auth = state
            .stored
            .as_ref()
            .is_some_and(|s| s.authorization == authorization);
        if same_auth
            && !state.blocked
            && state
                .stored
                .as_ref()
                .is_some_and(|s| s.metadata == metadata)
        {
            return Ok(());
        }
        let identity_changed = state
            .stored
            .as_ref()
            .is_some_and(|s| s.authorization.account_id() != authorization.account_id());
        let stored = Stored {
            version: 1,
            authorization,
            metadata,
        };
        self.persist(&stored)?;
        state.stored = Some(stored);
        state.revision += 1;
        state.blocked = false;
        state.last_error = None;
        if identity_changed {
            state.account_credits = None;
            self.revoke_in_flight(&mut state);
        }
        Ok(())
    }

    pub(crate) async fn update_metadata(&self, metadata: Value) -> Result<()> {
        let mut state = self.state.lock().await;
        let Some(stored) = state.stored.as_ref() else {
            return Ok(());
        };
        let metadata = public_metadata(&stored.authorization, metadata)?;
        if stored.metadata == metadata {
            return Ok(());
        }
        let stored = Stored {
            version: 1,
            authorization: stored.authorization.clone(),
            metadata,
        };
        self.persist(&stored)?;
        state.stored = Some(stored);
        Ok(())
    }

    pub(crate) async fn snapshot(&self) -> Option<Value> {
        self.state
            .lock()
            .await
            .stored
            .as_ref()
            .map(|s| s.metadata.clone())
    }

    // Retaining a rejected binding routes subsequent calls to a useful auth error,
    // never back to a stale desktop ledger. New capture clears the rejection.
    pub(crate) async fn ready_for(&self, account_id: &str) -> bool {
        self.state
            .lock()
            .await
            .stored
            .as_ref()
            .is_some_and(|s| s.authorization.account_id() == account_id)
    }

    pub(crate) async fn last_error(&self) -> Option<String> {
        self.state.lock().await.last_error.clone()
    }

    pub(crate) async fn clear(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        state.stored = None;
        state.revision += 1;
        state.blocked = false;
        state.last_error = None;
        state.account_credits = None;
        self.revoke_in_flight(&mut state);
        self.secrets
            .delete(VAULT_KEY)
            .map_err(|_| anyhow::anyhow!("WorkBuddy 分享已停止，但未能移除受保护授权，请重试停用"))
    }

    fn revoke_in_flight(&self, state: &mut State) {
        state.lifecycle += 1;
        self.changed.send_replace(state.lifecycle);
    }

    fn persist(&self, stored: &Stored) -> Result<()> {
        let bytes =
            serde_json::to_vec(stored).map_err(|_| anyhow::anyhow!("无法保存 WorkBuddy 授权"))?;
        self.secrets
            .set(VAULT_KEY, &bytes)
            .map_err(|_| anyhow::anyhow!("无法保存受保护的 WorkBuddy 授权，请检查本机凭据存储"))
    }

    pub(crate) async fn forward(
        &self,
        binding: &BuiltinBinding,
        request: Request,
        deadline: Instant,
    ) -> Result<Response, GatewayError> {
        let mut changed = self.changed.subscribe();
        let (mut auth, mut revision, lifecycle) = {
            let state = self.state.lock().await;
            let stored = state
                .stored
                .as_ref()
                .ok_or_else(|| GatewayError::bad_request(REAUTHORIZE))?;
            if stored.authorization.account_id() != binding.account_id {
                return Err(GatewayError::bad_request(
                    "WorkBuddy 账户已变化，请重新选择当前账户模型",
                ));
            }
            if state.blocked {
                return Err(GatewayError::bad_request(REAUTHORIZE));
            }
            (
                stored.authorization.clone(),
                state.revision,
                state.lifecycle,
            )
        };
        check_lifecycle(&changed, lifecycle)?;
        let mut refreshed = false;
        if auth.auth.expires_at.is_some_and(|at| at <= now_ms()) {
            (auth, revision) = self
                .refresh(revision, lifecycle, &mut changed, deadline)
                .await?;
            refreshed = true;
        }
        let replay = request.try_clone();
        let mut response = self
            .send(request, &auth, lifecycle, &mut changed, deadline)
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED && !refreshed {
            drop(response);
            let request = replay.ok_or_else(|| {
                GatewayError::upstream(
                    StatusCode::UNAUTHORIZED,
                    "WorkBuddy 登录已过期，本次请求无法安全重发，请重新授权",
                )
            })?;
            (auth, revision) = self
                .refresh(revision, lifecycle, &mut changed, deadline)
                .await?;
            response = self
                .send(request, &auth, lifecycle, &mut changed, deadline)
                .await?;
        }
        let status = response.status();
        if !status.is_success() {
            if status == StatusCode::UNAUTHORIZED {
                self.block_if_current(lifecycle, revision, REAUTHORIZE)
                    .await;
            }
            return Err(upstream_error(status));
        }
        check_lifecycle(&changed, lifecycle)?;
        Ok(cancellable_response(response, changed, lifecycle, deadline))
    }

    async fn send(
        &self,
        mut request: Request,
        auth: &Authorization,
        lifecycle: u64,
        changed: &mut watch::Receiver<u64>,
        deadline: Instant,
    ) -> Result<Response, GatewayError> {
        *request.url_mut() = auth
            .chat_url()
            .map_err(|_| GatewayError::bad_request("WorkBuddy 模型接口地址不兼容"))?;
        *request.headers_mut() = auth.headers(&auth.auth.access_token)?;
        let headers = request.headers_mut();
        insert(headers, "x-conversation-id", &self.conversation_id)?;
        insert(
            headers,
            "x-conversation-request-id",
            &self.conversation_request_id,
        )?;
        insert(headers, "acp-connection-id", &self.acp_connection_id)?;
        for key in ["x-conversation-message-id", "x-request-id"] {
            insert(headers, key, &uuid::Uuid::new_v4().to_string())?;
        }
        let trace = uuid::Uuid::new_v4().simple().to_string();
        let span = uuid::Uuid::new_v4().simple().to_string()[..16].to_owned();
        let parent = uuid::Uuid::new_v4().simple().to_string()[..16].to_owned();
        insert(headers, "traceparent", &format!("00-{trace}-{span}-01"))?;
        insert(headers, "b3", &format!("{trace}-{span}-1-{parent}"))?;
        insert(headers, "x-b3-traceid", &trace)?;
        insert(headers, "x-b3-parentspanid", &parent)?;
        insert(headers, "x-b3-spanid", &span)?;
        insert(headers, "x-b3-sampled", "1")?;
        insert(headers, "x-trace-id", &trace)?;
        *request.timeout_mut() = Some(deadline.saturating_duration_since(Instant::now()));
        self.execute(request, lifecycle, changed, deadline).await
    }

    async fn execute(
        &self,
        request: Request,
        lifecycle: u64,
        changed: &mut watch::Receiver<u64>,
        deadline: Instant,
    ) -> Result<Response, GatewayError> {
        check_lifecycle(changed, lifecycle)?;
        tokio::select! {
            biased;
            _ = changed.changed() => Err(GatewayError::bad_request(CANCELLED)),
            result = tokio::time::timeout_at(deadline, self.client.execute(request)) => result
                .map_err(|_| GatewayError::upstream_timeout())?
                .map_err(|e| if e.is_timeout() { GatewayError::upstream_timeout() } else { GatewayError::upstream(StatusCode::BAD_GATEWAY, "WorkBuddy 连接中断，本次未自动重发，请稍后重试") }),
        }
    }

    async fn refresh(
        &self,
        observed: u64,
        lifecycle: u64,
        changed: &mut watch::Receiver<u64>,
        deadline: Instant,
    ) -> Result<(Authorization, u64), GatewayError> {
        check_lifecycle(changed, lifecycle)?;
        let _guard = tokio::select! {
            biased;
            _ = changed.changed() => return Err(GatewayError::bad_request(CANCELLED)),
            guard = tokio::time::timeout_at(deadline, self.refresh_gate.lock()) => guard.map_err(|_| GatewayError::upstream_timeout())?,
        };
        let (auth, revision) = {
            let state = self.state.lock().await;
            if state.lifecycle != lifecycle {
                return Err(GatewayError::bad_request(CANCELLED));
            }
            let stored = state
                .stored
                .as_ref()
                .ok_or_else(|| GatewayError::bad_request(REAUTHORIZE))?;
            if state.blocked {
                return Err(GatewayError::bad_request(REAUTHORIZE));
            }
            if state.revision != observed {
                return Ok((stored.authorization.clone(), state.revision));
            }
            (stored.authorization.clone(), state.revision)
        };
        let token = auth.auth.refresh_token.as_deref().filter(|t| !t.is_empty());
        if token.is_none()
            || auth
                .auth
                .refresh_expires_at
                .is_some_and(|at| at <= now_ms())
        {
            self.block_if_current(lifecycle, revision, REAUTHORIZE)
                .await;
            return Err(GatewayError::bad_request(REAUTHORIZE));
        }
        let mut headers = auth.headers(&auth.auth.access_token)?;
        headers.insert("x-refresh-token", sensitive(token.unwrap())?);
        headers.insert("x-auth-refresh-source", HeaderValue::from_static("plugin"));
        let request = self
            .client
            .post(
                auth.auth_url("/auth/token/refresh")
                    .map_err(|_| GatewayError::bad_request("WorkBuddy 认证地址不兼容"))?,
            )
            .headers(headers)
            .json(&json!({}))
            .timeout(deadline.saturating_duration_since(Instant::now()))
            .build()
            .map_err(|_| GatewayError::bad_request("无法构建 WorkBuddy 续期请求"))?;
        let response = self.execute(request, lifecycle, changed, deadline).await?;
        if !response.status().is_success() {
            if matches!(response.status().as_u16(), 401 | 403) {
                self.block_if_current(lifecycle, revision, REAUTHORIZE)
                    .await;
            }
            return Err(upstream_error(response.status()));
        }
        let value = read_json(response, changed, lifecycle, deadline).await?;
        let Some(data) = successful_data(&value) else {
            return Err(GatewayError::upstream(
                StatusCode::BAD_GATEWAY,
                "WorkBuddy 续期响应不可用，请重新授权",
            ));
        };
        let mut renewed = auth.clone();
        renewed.auth = renewed_auth(&auth.auth, data)?;
        if renewed.auth.domain != auth.auth.domain {
            self.block_if_current(lifecycle, revision, "WorkBuddy 授权范围已变化，请重新授权")
                .await;
            return Err(GatewayError::bad_request(
                "WorkBuddy 授权范围已变化，请重新授权",
            ));
        }
        let suffix = if auth.auth_type == "account-scoped-external-link" {
            "/account"
        } else {
            "/accounts"
        };
        let request = self
            .client
            .get(
                auth.auth_url(suffix)
                    .map_err(|_| GatewayError::bad_request("WorkBuddy 账户接口不兼容"))?,
            )
            .headers(renewed.headers(&renewed.auth.access_token)?)
            .timeout(deadline.saturating_duration_since(Instant::now()))
            .build()
            .map_err(|_| GatewayError::bad_request("无法核对 WorkBuddy 授权账户"))?;
        let response = self.execute(request, lifecycle, changed, deadline).await?;
        if !response.status().is_success() {
            return Err(upstream_error(response.status()));
        }
        let value = read_json(response, changed, lifecycle, deadline).await?;
        if !same_account(&auth, successful_data(&value), suffix == "/accounts") {
            self.block_if_current(
                lifecycle,
                revision,
                "WorkBuddy 账户或组织已变化，请重新授权",
            )
            .await;
            return Err(GatewayError::bad_request(
                "WorkBuddy 账户或组织已变化，请重新授权",
            ));
        }
        renewed
            .validate()
            .map_err(|_| GatewayError::bad_request("WorkBuddy 续期授权内容不兼容"))?;
        let mut state = self.state.lock().await;
        if state.lifecycle != lifecycle || state.stored.is_none() {
            return Err(GatewayError::bad_request(CANCELLED));
        }
        if state.revision != revision {
            return Ok((
                state.stored.as_ref().unwrap().authorization.clone(),
                state.revision,
            ));
        }
        let stored = Stored {
            version: 1,
            authorization: renewed.clone(),
            metadata: state.stored.as_ref().unwrap().metadata.clone(),
        };
        self.persist(&stored).map_err(|_| {
            GatewayError::bad_request("WorkBuddy 授权已续期，但受保护保存失败，请重新授权")
        })?;
        state.stored = Some(stored);
        state.revision += 1;
        state.last_error = None;
        Ok((renewed, state.revision))
    }

    async fn block_if_current(&self, lifecycle: u64, revision: u64, message: &str) {
        let mut state = self.state.lock().await;
        if state.lifecycle == lifecycle && state.revision == revision {
            state.blocked = true;
            state.last_error = Some(message.into());
        }
    }
}

fn valid_identity(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
        || value
            .as_i64()
            .is_some_and(|v| v.unsigned_abs() <= 9_007_199_254_740_991)
        || value.as_u64().is_some_and(|v| v <= 9_007_199_254_740_991)
}
fn scalar(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn endpoint(value: &str) -> Result<url::Url> {
    let url = url::Url::parse(value).map_err(|_| anyhow::anyhow!("WorkBuddy 接口地址无效"))?;
    let official = url.scheme() == "https"
        && url.host_str() == Some("www.workbuddy.cn")
        && url.port().is_none();
    #[cfg(test)]
    let official = official
        || (url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.port().is_some());
    ensure!(
        official
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "WorkBuddy 独立分享暂只支持官方服务地址"
    );
    Ok(url)
}

fn public_metadata(auth: &Authorization, metadata: Value) -> Result<Value> {
    ensure!(
        metadata.get("accountId").and_then(Value::as_str) == Some(auth.account_id().as_str()),
        "WorkBuddy 授权账户与模型目录不一致"
    );
    ensure!(
        metadata.get("models").is_some_and(Value::is_array),
        "WorkBuddy 模型目录格式无效"
    );
    Ok(
        json!({"accountId":metadata["accountId"],"accountName":metadata.get("accountName"),"models":metadata["models"]}),
    )
}

fn insert(headers: &mut HeaderMap, key: &'static str, value: &str) -> Result<(), GatewayError> {
    let value = HeaderValue::from_str(value)
        .map_err(|_| GatewayError::bad_request("WorkBuddy 请求上下文无效"))?;
    headers.insert(key, value);
    Ok(())
}
fn sensitive(value: &str) -> Result<HeaderValue, GatewayError> {
    let mut header = HeaderValue::from_str(value)
        .map_err(|_| GatewayError::bad_request("WorkBuddy 授权内容无效"))?;
    header.set_sensitive(true);
    Ok(header)
}
fn check_lifecycle(changed: &watch::Receiver<u64>, lifecycle: u64) -> Result<(), GatewayError> {
    if *changed.borrow() != lifecycle {
        return Err(GatewayError::bad_request(CANCELLED));
    }
    Ok(())
}
fn upstream_error(status: StatusCode) -> GatewayError {
    let message = match status.as_u16() {
        401 | 403 => REAUTHORIZE,
        429 => "WorkBuddy 当前账户调用受限，请稍后重试或检查账户/组织额度",
        301..=399 => "WorkBuddy 接口发生跳转，已停止转发，请重新核对客户端版本",
        404 => "WorkBuddy 当前模型或接口不可用，请重新读取并选择模型",
        _ => "WorkBuddy 模型调用未完成，本次未自动重发，请稍后重试",
    };
    GatewayError::upstream(status, message)
}
fn successful_data(value: &Value) -> Option<&Value> {
    if value
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code != 0 && code != 200)
    {
        return None;
    }
    value.get("data").filter(|v| v.is_object())
}
fn renewed_auth(previous: &Auth, value: &Value) -> Result<Auth, GatewayError> {
    let access_token = value
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| GatewayError::bad_request("WorkBuddy 续期没有返回可用授权，请重新授权"))?;
    let expires = |absolute: &str, relative: &str| {
        value.get(absolute).and_then(Value::as_u64).or_else(|| {
            value
                .get(relative)
                .and_then(Value::as_u64)
                .and_then(|seconds| seconds.checked_mul(1000))
                .and_then(|ms| now_ms().checked_add(ms))
        })
    };
    Ok(Auth {
        access_token: access_token.into(),
        refresh_token: value
            .get("refreshToken")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| previous.refresh_token.clone()),
        domain: value
            .get("domain")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| previous.domain.clone()),
        token_type: value
            .get("tokenType")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| previous.token_type.clone()),
        expires_at: expires("expiresAt", "expiresIn"),
        refresh_expires_at: expires("refreshExpiresAt", "refreshExpiresIn")
            .or(previous.refresh_expires_at),
    })
}
fn same_account(auth: &Authorization, value: Option<&Value>, list: bool) -> bool {
    let matches = |account: &Value| {
        account.get("uid") == Some(&auth.account.uid)
            && account
                .get("enterpriseId")
                .filter(|v| !v.is_null())
                .filter(|v| v.as_str() != Some(""))
                == auth
                    .account
                    .enterprise_id
                    .as_ref()
                    .filter(|v| !v.is_null())
                    .filter(|v| v.as_str() != Some(""))
            && auth.account.kind.as_deref().is_none_or(|kind| {
                account
                    .get("type")
                    .and_then(Value::as_str)
                    .is_none_or(|actual| actual.eq_ignore_ascii_case(kind))
            })
    };
    value.is_some_and(|v| {
        if list {
            v.get("accounts")
                .and_then(Value::as_array)
                .is_some_and(|accounts| accounts.iter().any(matches))
        } else {
            matches(v)
        }
    })
}

async fn read_json(
    response: Response,
    changed: &mut watch::Receiver<u64>,
    lifecycle: u64,
    deadline: Instant,
) -> Result<Value, GatewayError> {
    check_lifecycle(changed, lifecycle)?;
    tokio::select! {
        biased;
        _ = changed.changed() => Err(GatewayError::bad_request(CANCELLED)),
        result = tokio::time::timeout_at(deadline, response.json::<Value>()) => result.map_err(|_| GatewayError::upstream_timeout())?
            .map_err(|_| GatewayError::upstream(StatusCode::BAD_GATEWAY, "WorkBuddy 认证响应不兼容，请重新授权")),
    }
}

fn cancellable_response(
    response: Response,
    mut changed: watch::Receiver<u64>,
    lifecycle: u64,
    deadline: Instant,
) -> Response {
    use futures_util::StreamExt;
    let status = response.status();
    let version = response.version();
    let headers = response.headers().clone();
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    // Keep stop/deadline active even if a slow client is not polling the body.
    // A bounded channel preserves backpressure and dropping the body stops fetch.
    tokio::spawn(async move {
        let mut upstream = response.bytes_stream();
        loop {
            let item = if *changed.borrow() != lifecycle {
                Err(std::io::Error::other(CANCELLED))
            } else {
                tokio::select! {
                    biased;
                    _ = sender.closed() => return,
                    _ = changed.changed() => Err(std::io::Error::other(CANCELLED)),
                    chunk = tokio::time::timeout_at(deadline, upstream.next()) => match chunk {
                        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "WorkBuddy 请求超时")),
                        Ok(None) => return,
                        Ok(Some(Ok(bytes))) => Ok(bytes),
                        Ok(Some(Err(_))) => Err(std::io::Error::other("WorkBuddy 响应中断，本次未自动重发")),
                    }
                }
            };
            if item.is_err() {
                drop(upstream);
                let _ = sender.try_send(item);
                return;
            }
            tokio::select! {
                biased;
                _ = changed.changed() => return,
                _ = tokio::time::sleep_until(deadline) => return,
                result = sender.send(item) => if result.is_err() { return; },
            }
        }
    });
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    let mut raw = axum::http::Response::new(reqwest::Body::wrap_stream(stream));
    *raw.status_mut() = status;
    *raw.version_mut() = version;
    *raw.headers_mut() = headers;
    Response::from(raw)
}

#[cfg(test)]
mod tests;
