use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use libsm::{
    sm2::{encrypt::EncryptCtx, signature::SigCtx},
    sm3::hash::Sm3Hash,
};
use rand::rngs::OsRng;
use reqwest::{
    Client, RequestBuilder,
    header::{HeaderName, HeaderValue},
};
use rsa::{
    Pkcs1v15Encrypt, RsaPublicKey, pkcs1::DecodeRsaPublicKey, pkcs8::DecodePublicKey,
    traits::PublicKeyParts,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use super::{
    config::{endpoint, validate_profile, validate_url},
    types::{
        AuthMode, AuthOutcome, AuthResult, AuthToken, NvwaProfile, PasswordLoginInput,
        SignatureAlgorithm, TokenExpiryPolicy, VerifiedIdentity,
    },
};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const DEFAULT_FIXED_TOKEN_SECONDS: u64 = 24 * 3600;

#[derive(Clone, Copy)]
enum RequestStage {
    LoginKey,
    PasswordLogin,
    ApplicationCredential,
    VerificationCode,
    Logout,
    TicketExchange,
    ReadIdentity,
    VerifySavedLogin,
}

impl RequestStage {
    fn label(self) -> &'static str {
        match self {
            Self::LoginKey => "获取加密公钥",
            Self::PasswordLogin => "账号密码登录",
            Self::ApplicationCredential => "共享应用申请凭证",
            Self::VerificationCode => "发送验证码",
            Self::Logout => "退出登录",
            Self::TicketExchange => "票据换成连接凭证",
            Self::ReadIdentity => "读取当前账号租户",
            Self::VerifySavedLogin => "验证已保存登录",
        }
    }

    fn unauthorized_hint(self) -> &'static str {
        match self {
            Self::LoginKey => {
                "登录加密公钥请求被拒绝。请核对 NVWA 服务地址，并请管理员检查登录接口的访问配置。"
            }
            Self::PasswordLogin => {
                "账号密码登录请求被拒绝。请核对账号、密码和 NVWA 服务地址；若信息正确，请管理员检查登录接口的访问配置。"
            }
            Self::ApplicationCredential => {
                "认证服务未接受本次应用认证。请核对 ClientID、ClientSecret、认证服务地址和部署使用的签名算法。"
            }
            Self::TicketExchange => {
                "认证服务未接受本次应用认证或票据交换。请核对 ClientID、ClientSecret、认证服务地址和部署使用的签名算法，并重新授权获取票据。"
            }
            Self::ReadIdentity | Self::VerifySavedLogin => {
                "产品未接受当前连接凭证。请核对 NVWA 服务地址、认证头和认证服务与产品之间的信任配置。"
            }
            Self::VerificationCode => {
                "验证码请求被拒绝。请重新开始登录，并请管理员检查验证码接口。"
            }
            Self::Logout => "退出登录请求被拒绝，原登录凭证可能已失效。",
        }
    }
}

#[derive(Clone)]
pub struct AuthClient {
    client: Client,
}

impl AuthClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| anyhow::anyhow!("无法初始化 NVWA 认证网络客户端"))?;
        Ok(Self { client })
    }

    pub async fn password_login(
        &self,
        profile: &NvwaProfile,
        input: &PasswordLoginInput,
    ) -> Result<AuthOutcome> {
        validate_profile(profile)?;
        ensure!(
            profile.auth_mode == AuthMode::Password,
            "此环境未选择账号密码认证"
        );
        ensure!(!profile.username.is_empty(), "请填写 NVWA 登录账号");
        ensure!(
            !input.password.is_empty() && input.password.len() <= 4096,
            "NVWA 密码为空或超出长度限制"
        );
        let key_payload = self
            .request_json(
                self.client.get(endpoint(
                    &profile.product_base_url,
                    "anon/framework/api/encrypt/key",
                )?),
                RequestStage::LoginKey,
            )
            .await?;
        ensure_business_success(&key_payload)?;
        let key = data_object(&key_payload);
        let public_key = key
            .get("text")
            .and_then(Value::as_str)
            .filter(|value| {
                !value.trim().is_empty()
                    && value.len() <= 16 * 1024
                    && !value
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
            })
            .ok_or_else(|| anyhow::anyhow!("NVWA 未返回有效登录加密公钥"))?;
        let alias = match key.get("alias") {
            Some(Value::String(value)) => value.clone(),
            Some(Value::Number(value)) => value.to_string(),
            _ => bail!("NVWA 未返回登录加密算法，无法安全登录"),
        };
        let username = encrypt_login_field(public_key, &alias, &profile.username)?;
        let password = encrypt_login_field(public_key, &alias, &input.password)?;
        let mut extra = Map::new();
        for (name, value) in [
            ("verifyId", &input.verify_id),
            ("verifyCode", &input.verify_code),
            ("twofactorSessionId", &input.twofactor_session_id),
            ("validCode", &input.valid_code),
        ] {
            if let Some(value) = value {
                ensure!(
                    !value.is_empty()
                        && value.len() <= 1024
                        && !value.chars().any(char::is_control),
                    "NVWA 登录挑战字段无效"
                );
                extra.insert(name.to_owned(), Value::String(value.clone()));
            }
        }
        let tenant = if profile.tenant.is_empty() {
            "__default_tenant__"
        } else {
            profile.tenant.as_str()
        };
        let mut body = json!({"username": username, "pwd": password, "tenant": tenant, "encryptType": "3L", "extInfo": extra});
        if let Some(unit) = &profile.login_unit {
            body["loginUnit"] = Value::String(unit.clone());
        }
        let payload = self
            .request_json(
                self.client
                    .post(endpoint(&profile.product_base_url, "nvwa/login")?)
                    .json(&body),
                RequestStage::PasswordLogin,
            )
            .await?;
        let response = data_object(&payload);
        ensure!(
            payload.get("success").and_then(Value::as_bool) != Some(false)
                && response.get("success").and_then(Value::as_bool) != Some(false),
            "NVWA 登录被服务端拒绝"
        );
        let code = business_code(response)
            .or_else(|| business_code(&payload))
            .ok_or_else(|| anyhow::anyhow!("NVWA 登录未返回明确业务状态"))?;
        if !std::ptr::eq(response, &payload) {
            if let Some(outer_code) = business_code(&payload) {
                ensure!(
                    matches!(outer_code, 0 | 200) || outer_code == code,
                    "NVWA 登录外层业务状态失败"
                );
            }
        }
        match code {
            201 | 202 => {
                return Ok(AuthOutcome::PasswordChangeRequired {
                    code,
                    message: if code == 201 {
                        "首次登录需要修改密码，请在 NVWA 页面完成后重新登录".into()
                    } else {
                        "密码已过期，请在 NVWA 页面修改后重新登录".into()
                    },
                });
            }
            204 => {
                let session_id = required_text(
                    response,
                    "twofactorSessionId",
                    "NVWA 双因子响应缺少会话标识",
                )?;
                let channel = match response.get("msg").and_then(Value::as_str) {
                    Some("1") => "email",
                    Some("2") => "sms",
                    _ => "unknown",
                }
                .to_owned();
                return Ok(AuthOutcome::TwoFactorRequired {
                    session_id,
                    channel,
                });
            }
            402 => {
                return Ok(AuthOutcome::CaptchaRequired {
                    message: "产品要求图形验证码；当前 Hub 尚不显示验证码图片，请在 NVWA 产品页面完成登录验证，或选择已配置的浏览器授权".into(),
                });
            }
            0 | 200 | 203 => {}
            _ => bail!("{}", login_error(code)),
        }
        let token = required_text(response, "token", "NVWA 登录未返回认证令牌")?;
        let login_header = match response.get("httpKey").and_then(Value::as_str) {
            Some(value) => allowed_header(value)?,
            None => "Authorization".to_owned(),
        };
        let personal_token = AuthToken {
            value: token.clone(),
            header_name: login_header,
            // The product extends this idle session on authenticated access.
            // Initial response times must not become a permanent local deadline.
            expires_at_ms: None,
            expiry_policy: TokenExpiryPolicy::SlidingIdle,
        };
        let identity = self
            .fetch_identity(profile, &personal_token, RequestStage::ReadIdentity)
            .await?;
        let mcp_token = AuthToken {
            value: token,
            header_name: profile
                .mcp_auth_header
                .as_deref()
                .map(allowed_header)
                .transpose()?
                .unwrap_or_else(|| "Authorization".to_owned()),
            expires_at_ms: None,
            expiry_policy: TokenExpiryPolicy::SlidingIdle,
        };
        Ok(AuthOutcome::Authenticated(AuthResult {
            identity,
            personal_token: Some(personal_token),
            mcp_token,
        }))
    }

    pub async fn application_login(
        &self,
        profile: &NvwaProfile,
        app_secret: &[u8],
    ) -> Result<AuthResult> {
        validate_profile(profile)?;
        ensure!(
            profile.auth_mode == AuthMode::Application,
            "此环境未选择应用代表用户认证"
        );
        validate_application(profile, app_secret)?;
        let secret = std::str::from_utf8(app_secret)
            .map_err(|_| anyhow::anyhow!("NVWA 应用密钥编码无效"))?;
        let now = epoch_ms()?;
        let identity = format!("{}:{now}:{}", profile.client_id, profile.username);
        let (signed_identity, digest) = match profile.signature_algorithm {
            SignatureAlgorithm::Md5 => {
                let digest = format!("{:X}", md5::compute(format!("{identity}:{secret}")));
                (identity, digest)
            }
            SignatureAlgorithm::Sha256 => {
                let identity = format!("{identity}:1");
                let digest = hex::encode(Sha256::digest(format!("{identity}:{secret}").as_bytes()));
                (identity, digest)
            }
            SignatureAlgorithm::Sm3 => {
                let identity = format!("{identity}:2");
                let digest =
                    hex::encode(Sm3Hash::new(format!("{identity}:{secret}").as_bytes()).get_hash());
                (identity, digest)
            }
        };
        let auth = sensitive_header(&STANDARD.encode(format!("{signed_identity}:{digest}")))?;
        let payload = self
            .request_json(
                self.client
                    .get(endpoint(
                        certification_base(profile),
                        "nvwa-certification/v1/ticket/apply",
                    )?)
                    .header("authorization-cer-client", auth),
                RequestStage::ApplicationCredential,
            )
            .await?;
        ensure_business_success(&payload)?;
        let ticket = required_text(data_object(&payload), "id", "NVWA 应用取票未返回票据")?;
        let result = self.exchange_ticket(profile, &ticket, app_secret).await?;
        ensure_requested_identity(profile, &result.identity)?;
        Ok(result)
    }

    pub fn begin_browser(
        &self,
        profile: &NvwaProfile,
        callback_url: &str,
        state: &str,
    ) -> Result<String> {
        validate_profile(profile)?;
        ensure!(
            profile.auth_mode == AuthMode::Browser,
            "此环境未选择浏览器个人授权"
        );
        ensure!(
            !profile.client_id.is_empty() && !profile.client_id.contains(':'),
            "请填写 NVWA 应用 ID"
        );
        ensure!(
            !state.is_empty()
                && state.len() <= 256
                && state
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "浏览器授权事务标识无效"
        );
        let callback =
            Url::parse(callback_url).map_err(|_| anyhow::anyhow!("NVWA 浏览器回调地址无效"))?;
        ensure!(
            callback.scheme() == "http"
                && callback.host_str() == Some("127.0.0.1")
                && callback.username().is_empty()
                && callback.password().is_none()
                && callback.fragment().is_none(),
            "NVWA 浏览器回调必须使用本机回环地址"
        );
        let mut url = validate_url(&profile.product_base_url)?;
        let params = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("response_type", "code")
            .append_pair("client_id", &profile.client_id)
            .append_pair("redirect_uri", callback_url)
            .append_pair("state", state)
            .finish();
        url.set_fragment(Some(&format!("/authorize?{params}")));
        Ok(url.into())
    }

    pub async fn complete_browser(
        &self,
        profile: &NvwaProfile,
        ticket_id: &str,
        app_secret: &[u8],
    ) -> Result<AuthResult> {
        validate_profile(profile)?;
        ensure!(
            profile.auth_mode == AuthMode::Browser,
            "此环境未选择浏览器个人授权"
        );
        self.exchange_ticket(profile, ticket_id, app_secret).await
    }

    pub async fn verify_session(
        &self,
        profile: &NvwaProfile,
        session: &AuthResult,
    ) -> Result<VerifiedIdentity> {
        let identity = self
            .fetch_identity(
                profile,
                session
                    .personal_token
                    .as_ref()
                    .unwrap_or(&session.mcp_token),
                RequestStage::VerifySavedLogin,
            )
            .await?;
        ensure!(
            identity == session.identity,
            "NVWA 当前身份或租户已变化，请重新认证"
        );
        Ok(identity)
    }

    pub async fn send_twofactor(&self, profile: &NvwaProfile, session_id: &str) -> Result<()> {
        validate_profile(profile)?;
        ensure!(
            !session_id.is_empty()
                && session_id.len() <= 4096
                && !session_id.chars().any(char::is_control),
            "NVWA 双因子会话标识无效"
        );
        let payload = self
            .request_json(
                self.client
                    .post(endpoint(
                        &profile.product_base_url,
                        "anon/nvwa-nros/v1/msg/send",
                    )?)
                    .json(&json!({"twofactorSessionId": session_id})),
                RequestStage::VerificationCode,
            )
            .await?;
        ensure_business_success(&payload)
    }

    pub async fn logout(&self, profile: &NvwaProfile, session: &AuthResult) -> Result<()> {
        let Some(token) = &session.personal_token else {
            return Ok(());
        };
        let request = with_token(
            self.client
                .get(endpoint(&profile.product_base_url, "nvwa/logout")?),
            token,
        )?;
        let payload = self.request_json(request, RequestStage::Logout).await?;
        ensure_business_success(&payload)
    }

    async fn exchange_ticket(
        &self,
        profile: &NvwaProfile,
        ticket_id: &str,
        app_secret: &[u8],
    ) -> Result<AuthResult> {
        validate_application(profile, app_secret)?;
        ensure!(
            !ticket_id.is_empty()
                && ticket_id.len() <= 4096
                && !ticket_id.chars().any(char::is_control),
            "NVWA 票据格式无效"
        );
        let mut url = endpoint(certification_base(profile), "nvwa-ticket/v1/ticket")?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("NVWA 认证地址不支持票据路径"))?
            .push(ticket_id);
        let secret = std::str::from_utf8(app_secret)
            .map_err(|_| anyhow::anyhow!("NVWA 应用密钥编码无效"))?;
        let basic = sensitive_header(&STANDARD.encode(format!("{}:{secret}", profile.client_id)))?;
        let payload = self
            .request_json(
                self.client
                    .post(url)
                    .header("authorization-client-basic", basic),
                RequestStage::TicketExchange,
            )
            .await?;
        ensure_business_success(&payload)?;
        let token_object = data_object(&payload);
        let mcp_token = AuthToken {
            value: required_text(token_object, "id", "NVWA 票据交换未返回认证令牌")?,
            header_name: profile
                .mcp_auth_header
                .as_deref()
                .map(allowed_header)
                .transpose()?
                .unwrap_or_else(|| "authorization-ticket-token".into()),
            expires_at_ms: fixed_token_expiry(token_object)?,
            expiry_policy: TokenExpiryPolicy::Fixed,
        };
        let identity = self
            .fetch_identity(profile, &mcp_token, RequestStage::ReadIdentity)
            .await?;
        if !profile.username.is_empty() || !profile.tenant.is_empty() {
            ensure_requested_identity(profile, &identity)?;
        }
        Ok(AuthResult {
            identity,
            personal_token: None,
            mcp_token,
        })
    }

    async fn fetch_identity(
        &self,
        profile: &NvwaProfile,
        token: &AuthToken,
        stage: RequestStage,
    ) -> Result<VerifiedIdentity> {
        let request = with_token(
            self.client
                .get(endpoint(&profile.product_base_url, "nvwa/getLoginContext")?),
            token,
        )?;
        let payload = self.request_json(request, stage).await?;
        ensure_business_success(&payload)?;
        let outer = data_object(&payload);
        let context = outer
            .get("context")
            .filter(|v| v.is_object())
            .unwrap_or(outer);
        let user = context
            .get("contextUser")
            .or_else(|| context.get("conetxtUser"));
        let user_id = text(context.get("id"))
            .or_else(|| user.and_then(|u| text(u.get("id"))))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "NVWA 返回的账号或组织信息不完整（缺少用户标识），请联系管理员检查登录接口"
                )
            })?;
        let identity_id = context
            .get("contextIdentity")
            .and_then(|v| text(v.get("id")))
            .or_else(|| text(context.get("identityId")))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "NVWA 返回的账号或组织信息不完整（缺少身份标识），请联系管理员检查登录接口"
                )
            })?;
        let tenant_id = text(context.get("tenantName"))
            .or_else(|| text(context.get("tenantId")))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "NVWA 返回的账号或组织信息不完整（缺少租户标识），请联系管理员检查登录接口"
                )
            })?;
        let username = text(context.get("username"))
            .or_else(|| user.and_then(|u| text(u.get("name"))))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "NVWA 返回的账号或组织信息不完整（缺少登录账号），请联系管理员检查登录接口"
                )
            })?;
        let identity = VerifiedIdentity {
            user_id,
            identity_id,
            tenant_id,
            username,
        };
        ensure_requested_identity(profile, &identity)?;
        Ok(identity)
    }

    async fn request_json(&self, request: RequestBuilder, stage: RequestStage) -> Result<Value> {
        let mut response = request.send().await.map_err(|error| {
            if error.is_timeout() {
                anyhow::anyhow!("NVWA 认证请求超时")
            } else if error.is_connect() {
                anyhow::anyhow!("无法连接 NVWA 认证服务，请核对地址和网络")
            } else {
                anyhow::anyhow!("NVWA 认证网络请求失败")
            }
        })?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            bail!(
                "NVWA 在“{}”步骤返回 HTTP 401。{}",
                stage.label(),
                stage.unauthorized_hint()
            );
        }
        ensure!(
            response.status().is_success(),
            "NVWA 在“{}”步骤返回 HTTP {}",
            stage.label(),
            response.status().as_u16()
        );
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= MAX_RESPONSE_BYTES as u64),
            "NVWA 认证响应超出大小限制"
        );
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("无法读取 NVWA 认证响应"))?
        {
            ensure!(
                body.len() + chunk.len() <= MAX_RESPONSE_BYTES,
                "NVWA 认证响应超出大小限制"
            );
            body.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| anyhow::anyhow!("NVWA 认证响应不是有效 JSON"))?;
        ensure!(value.is_object(), "NVWA 认证响应结构无效");
        Ok(value)
    }
}

fn certification_base(profile: &NvwaProfile) -> &str {
    if profile.certification_base_url.is_empty() {
        &profile.product_base_url
    } else {
        &profile.certification_base_url
    }
}

fn validate_application(profile: &NvwaProfile, secret: &[u8]) -> Result<()> {
    ensure!(
        !profile.client_id.is_empty() && !profile.client_id.contains(':'),
        "NVWA 应用 ID 无效"
    );
    ensure!(
        !profile.username.contains(':'),
        "NVWA 应用认证登录名不能包含冒号"
    );
    ensure!(
        !secret.is_empty() && secret.len() <= 16 * 1024,
        "请填写 NVWA 应用密钥"
    );
    if profile.auth_mode == AuthMode::Application {
        ensure!(
            !profile.username.is_empty(),
            "应用代表用户认证必须明确登录名"
        );
    }
    Ok(())
}

fn with_token(request: RequestBuilder, token: &AuthToken) -> Result<RequestBuilder> {
    if let Some(expiry) = token.expires_at_ms {
        ensure!(expiry > epoch_ms()?, "NVWA 授权已过期，请重新认证");
    }
    let name = HeaderName::from_bytes(allowed_header(&token.header_name)?.as_bytes())
        .map_err(|_| anyhow::anyhow!("NVWA 认证头无效"))?;
    Ok(request.header(name, sensitive_header(&token.value)?))
}

fn sensitive_header(value: &str) -> Result<HeaderValue> {
    let mut header =
        HeaderValue::from_str(value).map_err(|_| anyhow::anyhow!("NVWA 认证材料格式无效"))?;
    header.set_sensitive(true);
    Ok(header)
}

fn allowed_header(value: &str) -> Result<String> {
    match value.to_ascii_lowercase().as_str() {
        "authorization" => Ok("Authorization".into()),
        "authorization-ticket-token" => Ok("authorization-ticket-token".into()),
        _ => bail!("NVWA 返回不支持的认证头"),
    }
}

fn data_object(payload: &Value) -> &Value {
    payload
        .get("data")
        .filter(|v| v.is_object())
        .unwrap_or(payload)
}

fn text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text))
            if !text.trim().is_empty()
                && text.len() <= 16 * 1024
                && !text.chars().any(char::is_control) =>
        {
            Some(text.clone())
        }
        Some(Value::Number(number)) => Some(number.to_string()),
        _ => None,
    }
}

fn required_text(value: &Value, key: &str, message: &str) -> Result<String> {
    text(value.get(key)).ok_or_else(|| anyhow::anyhow!("{message}"))
}

fn business_code(payload: &Value) -> Option<i64> {
    payload.get("code").and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_str().and_then(|v| v.parse().ok()))
    })
}

fn ensure_business_success(payload: &Value) -> Result<()> {
    ensure!(
        payload.get("success").and_then(Value::as_bool) != Some(false),
        "NVWA 认证操作被服务端拒绝"
    );
    if let Some(code) = business_code(payload) {
        ensure!(
            matches!(code, 0 | 200),
            "NVWA 认证操作失败（业务状态 {code}）"
        );
    }
    let data = data_object(payload);
    if !std::ptr::eq(data, payload) {
        ensure!(
            data.get("success").and_then(Value::as_bool) != Some(false),
            "NVWA 认证操作被服务端拒绝"
        );
        if let Some(code) = business_code(data) {
            ensure!(
                matches!(code, 0 | 200),
                "NVWA 认证操作失败（业务状态 {code}）"
            );
        }
    }
    Ok(())
}

fn ensure_requested_identity(profile: &NvwaProfile, identity: &VerifiedIdentity) -> Result<()> {
    if !profile.username.is_empty() {
        ensure!(
            profile.username.eq_ignore_ascii_case(&identity.username),
            "NVWA 返回的用户与环境账号不一致"
        );
    }
    if !profile.tenant.is_empty() {
        ensure!(
            profile.tenant == identity.tenant_id,
            "NVWA 返回的租户与环境配置不一致"
        );
    }
    Ok(())
}

fn epoch_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow::anyhow!("系统时间无效"))?
        .as_millis() as u64)
}

fn fixed_token_expiry(token: &Value) -> Result<Option<u64>> {
    if let Some(expiry) = token.get("expiresAtMs").and_then(Value::as_u64) {
        return Ok(Some(expiry));
    }
    // Both durations are seconds and belong only to this exchanged token's
    // response. Never inherit an earlier one-use ticket's validTime.
    let seconds = token
        .get("validTime")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .or_else(|| {
            token
                .get("tokenValidTime")
                .and_then(Value::as_u64)
                .filter(|seconds| *seconds > 0)
        })
        .unwrap_or(DEFAULT_FIXED_TOKEN_SECONDS);
    ensure!(seconds <= 366 * 24 * 3600, "NVWA 令牌有效期格式无效");
    let Some(created) = token.get("createTime").and_then(Value::as_u64) else {
        // A duration without a product timestamp cannot identify an exact expiry.
        return Ok(None);
    };
    ensure!(created >= 946_684_800_000, "NVWA 令牌有效期格式无效");
    Ok(Some(
        created
            .checked_add(
                seconds
                    .checked_mul(1000)
                    .ok_or_else(|| anyhow::anyhow!("NVWA 令牌有效期溢出"))?,
            )
            .ok_or_else(|| anyhow::anyhow!("NVWA 令牌有效期溢出"))?,
    ))
}

fn encrypt_login_field(key: &str, alias: &str, value: &str) -> Result<String> {
    match alias {
        "3" => {
            let public = if key.contains("-----BEGIN") {
                RsaPublicKey::from_public_key_pem(key)
                    .or_else(|_| RsaPublicKey::from_pkcs1_pem(key))
                    .map_err(|_| anyhow::anyhow!("NVWA RSA 登录公钥格式无效"))?
            } else {
                let der = STANDARD
                    .decode(
                        key.chars()
                            .filter(|c| !c.is_whitespace())
                            .collect::<String>(),
                    )
                    .map_err(|_| anyhow::anyhow!("NVWA RSA 登录公钥编码无效"))?;
                RsaPublicKey::from_public_key_der(&der)
                    .or_else(|_| RsaPublicKey::from_pkcs1_der(&der))
                    .map_err(|_| anyhow::anyhow!("NVWA RSA 登录公钥格式无效"))?
            };
            let encoded = STANDARD.encode(value.as_bytes());
            ensure!(
                (512..=8192).contains(&public.n().bits()),
                "NVWA RSA 登录公钥长度不受支持"
            );
            let mut result = String::new();
            for chunk in encoded.as_bytes().chunks(50) {
                let encrypted = public
                    .encrypt(&mut OsRng, Pkcs1v15Encrypt, chunk)
                    .map_err(|_| anyhow::anyhow!("NVWA RSA 登录加密失败"))?;
                result.push_str(&STANDARD.encode(encrypted));
            }
            Ok(result)
        }
        "2" => {
            let key = key.trim();
            let key = if key.len() == 128 {
                format!("04{key}")
            } else {
                key.to_owned()
            };
            let bytes =
                hex::decode(&key).map_err(|_| anyhow::anyhow!("NVWA SM2 登录公钥编码无效"))?;
            ensure!(
                bytes.len() == 65 && bytes[0] == 4,
                "NVWA SM2 登录公钥格式无效"
            );
            let point = SigCtx::new()
                .load_pubkey(&bytes)
                .map_err(|_| anyhow::anyhow!("NVWA SM2 登录公钥无效"))?;
            let encrypted = EncryptCtx::new(value.len(), point)
                .encrypt(value.as_bytes())
                .map_err(|_| anyhow::anyhow!("NVWA SM2 登录加密失败"))?;
            ensure!(
                encrypted.len() == 65 + value.len() + 32,
                "NVWA SM2 登录加密结果无效"
            );
            // sm-crypto mode 1 omits the SEC1 04 prefix and uses C1 || C3 || C2.
            let mut result = Vec::with_capacity(encrypted.len() - 1);
            result.extend_from_slice(&encrypted[1..65]);
            result.extend_from_slice(&encrypted[65 + value.len()..]);
            result.extend_from_slice(&encrypted[65..65 + value.len()]);
            Ok(hex::encode(result))
        }
        _ => bail!("NVWA 登录加密算法不受支持，无法安全登录"),
    }
}

fn login_error(code: i64) -> &'static str {
    match code {
        403 => "NVWA 账号或密码错误",
        404 => "NVWA 账号已锁定",
        405 => "NVWA 账号已停用",
        406 => "NVWA 账号状态异常",
        407 => "NVWA 在线用户数已达到限制",
        408 => "NVWA 多终端登录受到限制",
        409 => "NVWA 登录失败，请注意剩余尝试次数",
        410 => "NVWA 密码错误次数达到锁定限制",
        411 => "NVWA 初始密码已失效",
        412 => "NVWA 账号已过期",
        503 => "NVWA 正在维护",
        _ => "NVWA 登录未完成，请在产品登录页面核对所需认证步骤",
    }
}
